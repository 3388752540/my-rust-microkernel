// user/init/src/main.rs

#![no_std]
#![no_main]

use core::panic::PanicInfo;

fn sys_print_char(c: u8) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,
            in("rdi") c as u64,
            out("rcx") _,
            out("r11") _,
            lateout("rax") _, // 【关键修复】告诉编译器，调用结束后 rax 的值变成了垃圾，不要再用了
        );
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let msg = "Hello from Rust Init Process via Syscall!\n";
    
    for b in msg.as_bytes() {
        sys_print_char(*b);
    }

    // 打印完毕后死循环
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}