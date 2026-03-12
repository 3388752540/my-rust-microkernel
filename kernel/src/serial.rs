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

pub fn init_serial_hardware() {
    use x86_64::instructions::port::Port;
    unsafe {
        let base = 0x3F8;
        // 1. 开启接收数据中断 (IER)
        Port::new(base + 1).write(0x01u8);
        
        // 2. 配置 FIFO (FCR): 开启并清空收发缓冲区
        // 0xC7 = 11000111 (14字节触发阈值, 清空, 开启)
        Port::new(base + 2).write(0xC7u8);
        
        // 3. 配置控制位 (MCR): 必须开启 Bit 3 (OUT2) 才能把中断信号发给 PIC
        // 0x0B = 00001011 (OUT2, RTS, DTR)
        Port::new(base + 4).write(0x0Bu8);
    }
}