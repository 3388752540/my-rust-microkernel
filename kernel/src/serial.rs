use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    // 使用 Mutex（自旋锁）包装串口，确保多核/中断下访问安全
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    // 锁定串口并写入数据
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}

/// 仿照标准库实现的 print! 宏
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// 仿照标准库实现的 println! 宏
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    // 这里去掉 format_args!，直接传递参数给 print!
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}