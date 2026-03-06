use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    // 核心修复：整个锁定和写入过程必须在“禁止中断”的环境下进行
    interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

/// 仿照标准库实现的 print! 宏
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

/// 仿照标准库实现的 println! 宏
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    // 修正宏展开：直接透传参数给 print!
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}