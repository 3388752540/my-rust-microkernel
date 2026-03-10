#![no_std]
// 这里以后放系统调用号和共享结构体
pub fn syscall_print(c: char) {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,      // 系统调用号
            in("rdi") c as u64,
            out("rcx") _,     // syscall 会破坏 rcx 和 r11
            out("r11") _,
        );
    }
}