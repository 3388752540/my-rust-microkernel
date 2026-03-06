use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::gdt;           // 导入 GDT 模块以获取 IST 索引
use lazy_static::lazy_static;
use pic8259::ChainedPics; // 需要在 Cargo.toml 添加 pic8259 依赖
use spin;                  // 需要在 Cargo.toml 添加 spin 依赖

// ==========================================
// 1. 硬件中断配置 (PIC)
// ==========================================

// 0-31 号中断被 CPU 异常占用。硬件中断从 32 (0x20) 开始映射。
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// 初始化可编程中断控制器 (PIC)
/// 我们使用 Mutex 包装它，因为在发送 EOI 信号时需要修改其内部状态
pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// 定义中断索引，方便在 IDT 中注册
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,        // 时钟中断：IRQ 0
    Keyboard = PIC_1_OFFSET + 1, // 键盘中断：IRQ 1
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { self as usize }
}

// ==========================================
// 2. IDT 定义与初始化
// ==========================================

lazy_static! {
    /// 中断描述符表 (IDT)
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // --- CPU 异常注册 ---
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);

        // --- 硬件中断注册 ---
        // 注册时钟中断
        idt[InterruptIndex::Timer.as_usize()]
            .set_handler_fn(timer_interrupt_handler);
        // 注册键盘中断
        idt[InterruptIndex::Keyboard.as_usize()]
            .set_handler_fn(keyboard_interrupt_handler);
        
        idt
    };
}

/// 将 IDT 加载到 CPU 寄存器
pub fn init_idt() {
    IDT.load();
}

// ==========================================
// 3. 中断处理函数实现 (Handlers)
// ==========================================

/// 断点异常处理 (Breakpoint)
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("[EXCEPTION] BREAKPOINT hit!\n{:#?}", stack_frame);
}

/// 双重故障处理 (Double Fault) - 致命错误
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> ! 
{
    panic!("[FATAL] DOUBLE FAULT occurred!\n{:#?}", stack_frame);
}

/// 缺页异常处理 (Page Fault)
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    println!("\n[EXCEPTION] PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    loop { x86_64::instructions::hlt(); }
}

/// 时钟中断处理 (Timer)
/// 作用：这是抢占式调度的基础。目前先打印 "."
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // serial_print!("."); // 每秒会跳动几十次，建议调试时注释掉

    // 必须发送 EOI (End of Interrupt) 信号
    // 否则 PIC 会认为当前中断没处理完，永远不会发送下一个时钟信号
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

/// 键盘中断处理 (Keyboard)
/// 作用：读取扫描码并解析
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    // 键盘控制器的 I/O 端口是 0x60
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    // 基础演示：打印扫描码
    println!("[KEYBOARD] Scancode: {:#x}", scancode);

    // 发送 EOI 信号以接收下一次按键
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}