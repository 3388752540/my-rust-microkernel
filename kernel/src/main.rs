#![no_std]
#![no_main]

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use uart_16550::SerialPort;
use x86_64::instructions::port::Port; // <--- 补上这一行
use core::fmt::Write; // 为了使用 write! 和 writeln! 宏

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    // 1. 初始化串口 (标准 COM1 端口是 0x3F8)
    // unsafe 是因为硬件地址是裸指针，但在驱动内部已经封装好了 IO 指令
    let mut serial_port = unsafe { SerialPort::new(0x3F8) };
    serial_port.init();

    

    // 2. 像写 C 语言一样打印！
    // 这次我们用 writeln! 宏，它会自动处理格式化
    writeln!(serial_port, "Hello, World!").unwrap();
    writeln!(serial_port, "This is my Rust Microkernel running on QEMU!").unwrap();


    unsafe {
        // 向 0xF4 端口写入任何数据，QEMU 就会退出
        let mut port = Port::new(0xf4);
        port.write(0u32);
    }
    // ================
    
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}