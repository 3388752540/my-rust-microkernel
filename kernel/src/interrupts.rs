use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::gdt;           
use crate::syscall::{Message, INIT_MAILBOX}; 
use crate::task::executor::KERNEL_EXECUTOR;
use crate::task::{TaskState, CURRENT_TASK_ID};
use crate::arch::x86_64::switch::switch_context; 
use lazy_static::lazy_static;
use pic8259::ChainedPics;    
use spin;
use core::arch::naked_asm;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::sync::Arc;
use alloc::vec::Vec;

// ==========================================
// 1. 硬件中断配置
// ==========================================
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,           
    Keyboard = PIC_1_OFFSET + 1,    
    Serial = PIC_1_OFFSET + 4,      
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { self as usize }
}

static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

// ==========================================
// 2. 汇编包装器 (处理抢占与异常重调度的关键)
// ==========================================
macro_rules! handler_wrapper {
    ($name:ident, $inner:ident) => {
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() {
            
                naked_asm!(
                    // 保存当前现场
                    "push rax", "push rbx", "push rcx", "push rdx",
                    "push rdi", "push rsi", "push rbp", "push r8",
                    "push r9", "push r10", "push r11", "push r12",
                    "push r13", "push r14", "push r15",
                    "mov rdi, rsp", 
                    "call {inner}",
                    "mov rsp, rax", // 切换到 Rust 处理函数返回的新栈指针 (可能是原栈，也可能是新任务栈)
                    "pop r15", "pop r14", "pop r13", "pop r12",
                    "pop r11", "pop r10", "pop r9", "pop r8",
                    "pop rbp", "pop rsi", "pop rdi", "pop rdx",
                    "pop rcx", "pop rbx", "pop rax",
                    "iretq",
                    inner = sym $inner,
                );
            
        }
    };
}

// 注册所有需要支持“重调度”的处理器
handler_wrapper!(timer_handler, timer_handler_inner);
handler_wrapper!(keyboard_handler, keyboard_handler_inner);
handler_wrapper!(serial_handler, serial_handler_inner);
handler_wrapper!(gpf_handler_wrapper, gpf_handler_inner);
handler_wrapper!(page_fault_handler_wrapper, page_fault_handler_inner);

// ==========================================
// 3. IDT 初始化
// ==========================================
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // 基础异常
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }

        // --- 核心：将异常处理器也接入 Naked Wrapper 以便在报错时直接切走 ---
        unsafe {
            // 设置 GPF (13号)
            idt.general_protection_fault.set_handler_addr(
                x86_64::VirtAddr::new(gpf_handler_wrapper as *const () as u64)
            );
            // 设置 Page Fault (14号)
            idt.page_fault.set_handler_addr(
                x86_64::VirtAddr::new(page_fault_handler_wrapper as *const () as u64)
            );

            // 设置硬件中断
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
// 4. 辅助调度逻辑 (内核/用户混合调度的核心)
// ==========================================

pub static TASK_LIST: spin::Mutex<Vec<Arc<crate::task::Task>>> = 
    spin::Mutex::new(Vec::new());

pub static CURRENT_TASK_IDX: AtomicU64 = AtomicU64::new(0);

fn poll_kernel_tasks() {
    if let Some(mut executor) = KERNEL_EXECUTOR.try_lock() {
        executor.run_ready_tasks();
    }
}

/// 统一调度函数：决定当前中断返回后 CPU 去往哪个栈
fn schedule_next(current_rsp: u64, force_switch: bool) -> u64 {
    let mut next_rsp = current_rsp;

    if let Some(tasks) = TASK_LIST.try_lock() {
        if tasks.len() >= 1 {
            let old_idx = CURRENT_TASK_IDX.load(Ordering::SeqCst) as usize;
            let old_task = &tasks[old_idx];
            
            let is_terminated = *old_task.state.lock() == TaskState::Terminated;
            let priority = old_task.priority.load(Ordering::Relaxed).max(1);
            let ticks = TICK_COUNT.load(Ordering::Relaxed);

            // 如果时间片到期、任务已死、或强制切换（异常触发）
            if ticks % priority == 0 || is_terminated || force_switch {
                let mut next_idx = (old_idx + 1) % tasks.len();
                
                // 工业级：寻找下一个存活的进程
                for _ in 0..tasks.len() {
                    if *tasks[next_idx].state.lock() == TaskState::Ready {
                        break;
                    }
                    next_idx = (next_idx + 1) % tasks.len();
                }

                let next_task = &tasks[next_idx];

                // 如果确实需要切，或者当前任务已经没了
                if next_idx != old_idx || is_terminated {
                    CURRENT_TASK_IDX.store(next_idx as u64, Ordering::SeqCst);
                    CURRENT_TASK_ID.store(next_task.id.0, Ordering::SeqCst);

                    // 保存旧现场，载入新环境
                    old_task.kernel_rsp.store(current_rsp, Ordering::SeqCst);
                    unsafe {
                        gdt::set_interrupt_stack(x86_64::VirtAddr::new(next_task.kernel_stack_top));
                    }
                    next_rsp = next_task.kernel_rsp.load(Ordering::SeqCst);
                }
            }
        }
    }
    next_rsp
}

// ==========================================
// 5. 中断/异常业务逻辑
// ==========================================

extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
    poll_kernel_tasks();
    TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    
    schedule_next(current_rsp, false)
}

/// GPF 业务逻辑：处决坏进程，并立刻切到下一个进程
extern "C" fn gpf_handler_inner(current_rsp: u64) -> u64 {
    if let Some(tasks) = TASK_LIST.try_lock() {
        let idx = CURRENT_TASK_IDX.load(Ordering::Relaxed) as usize;
        let bad_pid = tasks[idx].id.0;
        crate::println!("\n\x1b[31;1m[SECURITY] GPF! PID {} killed for illegal instruction.\x1b[0m", bad_pid);
        *tasks[idx].state.lock() = TaskState::Terminated;
    }
    // 强制执行切换，绝不返回报错现场
    schedule_next(current_rsp, true)
}

extern "C" fn page_fault_handler_inner(current_rsp: u64) -> u64 {
    use x86_64::registers::control::Cr2;
    if let Some(tasks) = TASK_LIST.try_lock() {
        let idx = CURRENT_TASK_IDX.load(Ordering::Relaxed) as usize;
        crate::println!("\n\x1b[31;1m[SECURITY] PAGE FAULT! PID {} accessed {:?}\x1b[0m", tasks[idx].id.0, Cr2::read());
        *tasks[idx].state.lock() = TaskState::Terminated;
    }
    schedule_next(current_rsp, true)
}

extern "C" fn keyboard_handler_inner(rsp: u64) -> u64 {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x60);
    let data:u8 = unsafe { port.read() };
    if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
        *mailbox = Some(Message { from: 0, to: 1, label: 2, payload: [data as u64, 0] });
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    poll_kernel_tasks();
    rsp 
}

extern "C" fn serial_handler_inner(rsp: u64) -> u64 {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x3F8);
    let data:u8  = unsafe { port.read() };
    if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
        *mailbox = Some(Message { from: 0, to: 1, label: 3, payload: [data as u64, 0] });
    }
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial.as_u8()); }
    poll_kernel_tasks();
    rsp
}

// ==========================================
// 6. 基础支撑函数
// ==========================================

extern "x86-interrupt" fn breakpoint_handler(sf: InterruptStackFrame) {
    crate::println!("[EXCEPTION] BREAKPOINT hit at {:?}\n", sf.instruction_pointer);
}

extern "x86-interrupt" fn double_fault_handler(sf: InterruptStackFrame, _ec: u64) -> ! {
    panic!("[FATAL] DOUBLE FAULT - Potential Stack Mash or TSS Corruption\n{:#?}", sf);
}

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