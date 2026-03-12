use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::gdt;           
use crate::syscall::{Message, INIT_MAILBOX}; 
use crate::task::executor::KERNEL_EXECUTOR;
use lazy_static::lazy_static;
use pic8259::ChainedPics;    
use spin;
use core::arch::naked_asm;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::sync::Arc;
use alloc::vec::Vec;

// ==========================================
// 1. 硬件中断配置与屏蔽位管理
// ==========================================
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,           // IRQ 0
    Keyboard = PIC_1_OFFSET + 1,    // IRQ 1
    Serial = PIC_1_OFFSET + 4,      // IRQ 4
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { self as usize }
    
    /// 将 IRQ 编号转换为 PIC 的屏蔽位偏移
    fn irq_bit(self) -> u8 {
        self.as_u8() - PIC_1_OFFSET
    }
}

/// 工业级：动态开启指定的硬件中断线路 (IRQ)
pub unsafe fn enable_irq(index: InterruptIndex) {
    use x86_64::instructions::port::Port;
    let bit = index.irq_bit();
    let mut port = if bit < 8 {
        Port::<u8>::new(0x21) // 主片屏蔽寄存器
    } else {
        Port::<u8>::new(0xA1) // 从片屏蔽寄存器
    };
    let mask = unsafe { port.read() };
    unsafe { port.write(mask & !(1 << bit)) }; // 清零对应位以开启中断
}

static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

// ==========================================
// 2. 汇编包装器：实现“无缝上下文切换”
// ==========================================
macro_rules! handler_wrapper {
    ($name:ident, $inner:ident) => {
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() {
                naked_asm!(
                    // 1. 压入所有通用寄存器。注意：此时 RSP 还在旧任务的内核栈上
                    "push rax", "push rbx", "push rcx", "push rdx",
                    "push rdi", "push rsi", "push rbp", "push r8",
                    "push r9", "push r10", "push r11", "push r12",
                    "push r13", "push r14", "push r15",
                    
                    // 2. 调用 Rust 处理逻辑。传入 RDI = 当前栈指针
                    "mov rdi, rsp", 
                    "call {inner}",
                    
                    // 3. 【核心机制】：根据 Rust 函数返回值 RAX 切换栈指针
                    // 如果 Rust 返回了新任务的栈，RSP 就会瞬间跳到新任务的栈顶
                    "mov rsp, rax",
                    
                    // 4. 从新栈中恢复现场。这 15 个 pop 弹出的是新进程的寄存器
                    "pop r15", "pop r14", "pop r13", "pop r12",
                    "pop r11", "pop r10", "pop r9", "pop r8",
                    "pop rbp", "pop rsi", "pop rdi", "pop rdx",
                    "pop rcx", "pop r11", "pop rax",
                    
                    // 5. 退出中断。硬件会根据栈上的 CS 段选择子自动降权回 Ring 3
                    "iretq",
                    inner = sym $inner,
                );
            
        }
    };
}

handler_wrapper!(timer_handler, timer_handler_inner);
handler_wrapper!(keyboard_handler, keyboard_handler_inner);
handler_wrapper!(serial_handler, serial_handler_inner);

// ==========================================
// 3. IDT 初始化
// ==========================================
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);

        unsafe {
            idt[InterruptIndex::Timer.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(timer_handler as *const () as u64));
            idt[InterruptIndex::Keyboard.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(keyboard_handler as *const () as u64));
            idt[InterruptIndex::Serial.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(serial_handler as *const () as u64));
        }
        idt
    };
}

pub fn init_idt() { IDT.load(); }

// ==========================================
// 4. 抢占调度核心逻辑
// ==========================================

pub static TASK_LIST: spin::Mutex<Vec<Arc<crate::task::Task>>> = 
    spin::Mutex::new(Vec::new());

pub static CURRENT_TASK_IDX: AtomicU64 = AtomicU64::new(0);

fn poll_kernel_tasks() {
    if let Some(mut executor) = KERNEL_EXECUTOR.try_lock() {
        executor.run_ready_tasks();
    }
}

/// 时钟中断处理：实现 Ring 3 进程的公平时间片抢占
extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
    
    poll_kernel_tasks();

    let ticks = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut next_rsp = current_rsp;

    // 每 5 个滴答执行一次进程切换
    if ticks % 5 == 0 {
        if let Some(tasks) = TASK_LIST.try_lock() {
            if tasks.len() >= 2 {
                let old_idx = CURRENT_TASK_IDX.load(Ordering::SeqCst) as usize;
                let next_idx = (old_idx + 1) % tasks.len();
                
                let old_task = Arc::clone(&tasks[old_idx]);
                let next_task = Arc::clone(&tasks[next_idx]);
                
                // 1. 更新调度状态
                CURRENT_TASK_IDX.store(next_idx as u64, Ordering::SeqCst);
                crate::task::CURRENT_TASK_ID.store(next_task.id.0, Ordering::SeqCst);

                // 2. 【状态固化】：保存当前 CPU 的栈指针到旧进程的 TCB 中
                old_task.kernel_rsp.store(current_rsp, Ordering::SeqCst);

                // 3. 【硬件同步】：更新 TSS 的内核栈顶。
                // 确保下次无论发生什么中断，硬件都能跳入新进程的内核栈，而非踩踏旧栈。
                unsafe {
                    gdt::set_interrupt_stack(x86_64::VirtAddr::new(next_task.kernel_stack_top));
                }

                // 4. 【灵魂置换】：返回新进程上次被挂起时的栈指针
                next_rsp = next_task.kernel_rsp.load(Ordering::SeqCst);
            }
        }
    }
    next_rsp // 此值将通过汇编 Wrapper 赋给 RSP
}

// ==========================================
// 5. 外部交互与异常
// ==========================================

extern "C" fn keyboard_handler_inner(rsp: u64) -> u64 {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
        *mailbox = Some(Message { from: 0, to: 1, label: 2, payload: [scancode as u64, 0] });
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    poll_kernel_tasks();
    rsp 
}

extern "C" fn serial_handler_inner(rsp: u64) -> u64 {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x3F8);
    let data: u8 = unsafe { port.read() };
    // 发送探测符表示中断已到达内核
    // unsafe { port.write(b'!'); } 

    if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
        *mailbox = Some(Message { from: 0, to: 1, label: 3, payload: [data as u64, 0] });
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial.as_u8()); }
    poll_kernel_tasks();
    rsp
}

extern "x86-interrupt" fn breakpoint_handler(sf: InterruptStackFrame) {
    crate::println!("[EXCEPTION] BREAKPOINT hit at {:?}\n", sf.instruction_pointer);
}

extern "x86-interrupt" fn double_fault_handler(sf: InterruptStackFrame, _ec: u64) -> ! {
    panic!("[FATAL] DOUBLE FAULT - Potential Stack Mash or TSS Corruption\n{:#?}", sf);
}

extern "x86-interrupt" fn page_fault_handler(sf: InterruptStackFrame, ec: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    panic!("PAGE FAULT at {:?}\nError Code: {:?}\n{:#?}", Cr2::read(), ec, sf);
}

/// 进程引导中转站：用于将新进程第一次拉进运行轨道
#[unsafe(naked)]
pub unsafe extern "C" fn fork_ret() {
    
        naked_asm!(
            "pop r15", "pop r14", "pop r13", "pop r12",
            "pop r11", "pop r10", "pop r9", "pop r8",
            "pop rbp", "pop rsi", "pop rdi", "pop rdx",
            "pop rcx", "pop rbx", "pop rax",
            "iretq",
        );
    
}