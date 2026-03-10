// kernel/tests/basic_boot.rs

#![no_std]
#![no_main]
// 【核心修改】彻底去除了所有与 custom_test_frameworks 相关的宏
// 放弃让编译器帮我们生成 test_main，我们自己掌控入口！

use core::panic::PanicInfo;
use kernel::{exit_qemu, QemuExitCode};
use bootloader_api::{entry_point, BootInfo};

entry_point!(main);

fn main(_boot_info: &'static mut BootInfo) -> ! {
    // 1. 初始化内核环境，确保 GDT/IDT 都已经就绪
    kernel::init();
    
    // 2. 打印测试头部信息
    kernel::println!("\n\x1b[33;1m[TEST]\x1b[0m Running integration test: basic_boot");

    // ==========================================
    // 3. 【核心修改】直接手动调用测试函数！
    // ==========================================
    test_kernel_boot_sequence();
    // 以后如果你写了新的测试，比如 test_memory()，直接在这下面加一行调用即可

    // 4. 所有测试执行完毕，安全关机
    kernel::println!("\x1b[32m[All tests passed!]\x1b[0m");
    exit_qemu(QemuExitCode::Success);
    
    loop { x86_64::instructions::hlt(); }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // 依然使用内核中定义的标准测试 Panic 处理器
    kernel::test_panic_handler(info)
}

// ==========================================
// 集成测试用例
// ==========================================

// 【核心修改】不再需要 #[test_case] 宏，它就是一个普通的函数
fn test_kernel_boot_sequence() {
    // 模拟之前测试框架的打印格式
    kernel::print!("basic_boot::test_kernel_boot_sequence...\t");
    
    // 具体的测试断言逻辑
    assert_eq!(1, 1);
    
    // 如果没 panic，打印绿色的[ok]
    kernel::println!("\x1b[32m[ok]\x1b[0m");
}