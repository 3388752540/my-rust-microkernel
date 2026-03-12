// kernel/src/lib.rs
#![no_std]
#![cfg_attr(test, no_main)]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
// 指定测试运行器，并将生成的入口命名为 test_main
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

#[macro_use]
pub mod serial;
pub mod interrupts;
pub mod gdt;
pub mod mem;
pub mod allocator;
pub mod task;
pub mod syscall;
pub mod arch;

use core::panic::PanicInfo;

// ==========================================
// 工业级公共 API：一键初始化硬件
// ==========================================
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize(); }
    syscall::init();
    //x86_64::instructions::interrupts::enable();//在所有基础资源（内存、任务、同步原语）完全就绪之前，绝对不能开启硬件中断。
}

// ==========================================
// 工业级测试支撑框架 (供所有集成测试调用)
// ==========================================
pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T where T: Fn() {
    fn run(&self) {
        print!("{}...\t", core::any::type_name::<T>());
        self();
        println!("\x1b[32m[ok]\x1b[0m");
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    println!("\n\x1b[33;1m[TEST]\x1b[0m Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    println!("\x1b[31;1m[FAILED]\x1b[0m");
    println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    loop { x86_64::instructions::hlt(); }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

// ==========================================
// lib.rs 自身的单元测试入口
// ==========================================
#[cfg(test)]
use bootloader_api::{entry_point, BootInfo};

#[cfg(test)]
entry_point!(test_kernel_main);

#[cfg(test)]
fn test_kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    init();
    // 能够直接调用 test_main 的前提是：下方必须有 #[test_case]
    test_main();
    loop { x86_64::instructions::hlt(); }
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}

// 【关键】为了让 lib.rs 成功生成 test_main，这里必须放一个保底测试！
#[cfg(test)]
#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}