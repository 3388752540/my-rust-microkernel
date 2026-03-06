use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::gdt;           
use crate::task::keyboard;   
use lazy_static::lazy_static;
use pic8259::ChainedPics;    
use spin;
use core::arch::{naked_asm, asm}; // 必须引入内联汇编支持

// ==========================================
// 1. 硬件中断配置 (PIC)
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
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { self as usize }
}

// ==========================================
// 2. 汇编包装器宏 (解决 iretq 错位的核心)
// ==========================================

/// 该宏生成一个裸函数（Naked Function），手动保存寄存器现场
/// 并在调用 Rust 逻辑后手动执行 iretq。
macro_rules! handler_wrapper {
    ($name:ident, $inner:ident) => {
        // 修改 1: 在 Rust 2024 中，naked 属性必须写成 #[unsafe(naked)]
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() {
            // 修改 2: Rust 2024 规定即使在 unsafe fn 内部，
            // 调用汇编也必须包裹在 unsafe { ... } 块中
             {
                // 修改 3: 裸函数内部严禁使用普通的 asm!，必须使用 naked_asm!
                naked_asm!(
                    // 1. 保存现场
                    "push rax", "push rbx", "push rcx", "push rdx",
                    "push rdi", "push rsi", "push rbp", "push r8",
                    "push r9", "push r10", "push r11", "push r12",
                    "push r13", "push r14", "push r15",

                    // 2. 传递栈指针并调用 Rust 函数
                    "mov rdi, rsp",
                    "call {inner}",

                    // 3. 恢复现场
                    "pop r15", "pop r14", "pop r13", "pop r12",
                    "pop r11", "pop r10", "pop r9", "pop r8",
                    "pop rbp", "pop rsi", "pop rdi", "pop rdx",
                    "pop rcx", "pop rbx", "pop rax",

                    // 4. 返回
                    "iretq",
                    inner = sym $inner,
                );
            }
        }
    };
}

// ==========================================
// 3. IDT 定义与初始化
// ==========================================

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // --- 注册异常 ---
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);

        // --- 注册硬件中断 (使用汇编包装器地址) ---
        unsafe {
            idt[InterruptIndex::Timer.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(timer_handler as u64));
            idt[InterruptIndex::Keyboard.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(keyboard_handler as u64));
        }
        
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

// ==========================================
// 4. 中断处理函数逻辑 (Inner Handlers)
// ==========================================

// 生成汇编入口点
handler_wrapper!(timer_handler, timer_handler_inner);
handler_wrapper!(keyboard_handler, keyboard_handler_inner);

/// 时钟中断 Rust 逻辑
extern "C" fn timer_handler_inner(_stack_frame: *const InterruptStackFrame) {
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

/// 键盘中断 Rust 逻辑
extern "C" fn keyboard_handler_inner(_stack_frame: *const InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    keyboard::add_scancode(scancode);

    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

// ==========================================
// 5. 异常处理函数 (保留，因为它们通常工作正常)
// ==========================================

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    // 现在串口锁是安全的，可以打印了
    // println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> ! 
{
    panic!("[FATAL] DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    panic!(
        "EXCEPTION: PAGE FAULT\nAccessed Address: {:?}\nError Code: {:?}\n{:#?}",
        Cr2::read(),
        error_code,
        stack_frame
    );
}