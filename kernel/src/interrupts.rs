use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::gdt;           
use crate::syscall::{Message, INIT_MAILBOX}; 
use crate::task::executor::KERNEL_EXECUTOR; // 引入全局执行器
use lazy_static::lazy_static;
use pic8259::ChainedPics;    
use spin;
use core::arch::{naked_asm};

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

// ==========================================
// 2. 汇编包装器宏 (Naked Wrapper)
// ==========================================
macro_rules! handler_wrapper {
    ($name:ident, $inner:ident) => {
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() {
            unsafe {
                naked_asm!(
                    "push rax", "push rbx", "push rcx", "push rdx",
                    "push rdi", "push rsi", "push rbp", "push r8",
                    "push r9", "push r10", "push r11", "push r12",
                    "push r13", "push r14", "push r15",
                    "mov rdi, rsp",
                    "call {inner}",
                    "pop r15", "pop r14", "pop r13", "pop r12",
                    "pop r9", "pop r8", "pop r10", "pop rdx",
                    "pop rsi", "pop rdi", "pop rbx", "pop rbp",
                    "pop rcx", "pop r11", "pop rax",
                    "iretq",
                    inner = sym $inner,
                );
            }
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
// 4. 业务逻辑 (并存架构的核心)
// ==========================================

/// 辅助函数：在中断期间运行一轮内核异步任务
fn poll_kernel_tasks() {
    // try_lock 防止由于异常导致的嵌套死锁
    if let Some(mut executor) = KERNEL_EXECUTOR.try_lock() {
        executor.run_ready_tasks();
    }
}

extern "C" fn timer_handler_inner(_stack_frame: *const InterruptStackFrame) {
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
    
    // 【并存机制】利用时钟中断的“心跳”，顺便跑一下内核异步任务
    poll_kernel_tasks();
}

extern "C" fn keyboard_handler_inner(_stack_frame: *const InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    let msg = Message { from: 0, to: 1, label: 2, payload: [scancode as u64, 0] };
    *INIT_MAILBOX.lock() = Some(msg);

    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    
    // 输入事件后立即轮询，减少回显延迟
    poll_kernel_tasks();
}

extern "C" fn serial_handler_inner(_stack_frame: *const InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x3F8);
    let data: u8 = unsafe { port.read() };

    let msg = Message { from: 0, to: 1, label: 3, payload: [data as u64, 0] };
    *INIT_MAILBOX.lock() = Some(msg);

    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial.as_u8()); }
    
    // 输入事件后立即轮询
    poll_kernel_tasks();
}

// ==========================================
// 5. 异常处理
// ==========================================
extern "x86-interrupt" fn breakpoint_handler(_sf: InterruptStackFrame) {}
extern "x86-interrupt" fn double_fault_handler(sf: InterruptStackFrame, _ec: u64) -> ! {
    panic!("[FATAL] DOUBLE FAULT\n{:#?}", sf);
}
extern "x86-interrupt" fn page_fault_handler(sf: InterruptStackFrame, ec: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    panic!("PAGE FAULT at {:?}\nError Code: {:?}\n{:#?}", Cr2::read(), ec, sf);
}