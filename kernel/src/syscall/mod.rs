use x86_64::registers::model_specific::{LStar, Star, SFMask, Efer, EferFlags};
use x86_64::VirtAddr;
use crate::gdt;


/// 初始化系统调用硬件配置
pub fn init() {
    unsafe {
        // 1. LSTAR: 设置 syscall 跳转目标
        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));

        // 2. STAR: 设置内核与用户态的选择子基址
        let selectors = gdt::get_selectors();
        // 注意：x86_64 硬件要求 GDT 顺序必须是：内核CS, 内核SS, 用户SS, 用户CS
        Star::write(
            selectors.user_code_selector,
            selectors.user_data_selector,
            selectors.kernel_code_selector,
            selectors.kernel_data_selector,
        ).unwrap();

        // 3. SFMASK: 进入内核时自动关闭中断 (IF 位)
        SFMask::write(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG);

        // 4. EFER: 开启系统调用扩展
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
}

/// 系统调用底层入口
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    
        core::arch::naked_asm!(
            // 1. 强行把所有寄存器存入栈中，包括被硬件占用的 RCX 和 R11
            "push r11",
            "push rcx",
            "push rbp",
            "push rbx",
            "push rdi",
            "push rsi",
            "push rdx",
            "push r8",
            "push r9",
            "push r10",
            "push r12",
            "push r13",
            "push r14",
            "push r15",

            // 2. 准备参数并调用 Rust 逻辑
            // 用户: rax=ID, rdi=arg1
            // Rust: rdi=ID, rsi=arg1
            "mov rsi, rdi",
            "mov rdi, rax",
            "call {handler}",

            // 3. 彻底还原现场
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop r10",
            "pop r9",
            "pop r8",
            "pop rdx",
            "pop rsi",
            "pop rdi",
            "pop rbx",
            "pop rbp",
            "pop rcx", // 还原用户程序的返回地址 (RIP)
            "pop r11", // 还原用户程序的状态 (RFLAGS)

            // 4. 返回
            "sysretq",
            handler = sym handle_syscall,
        );
    }

/// 系统调用业务逻辑
/// id: 调用号 (由 rax 传入)
/// arg1: 参数 (由 rdi 传入，比如要打印的字符)
#[unsafe(no_mangle)]
extern "C" fn handle_syscall(id: u64, arg1: u64) {
    match id {
        // SYS_PRINT: 打印单个字符
        1 => {
            let c = arg1 as u8 as char;
            crate::print!("{}", c);
        },
        // SYS_EXIT: 演示退出
        2 => {
            crate::println!("\n[SYSCALL] Task requested exit.");
        },
        _ => {
            // 如果看到 ID 为 0 的报错，通常是因为用户程序的 RAX 被意外清零了
            // crate::println!("\n\x1b[31m[SYSCALL ERROR]\x1b[0m Unknown ID: {}", id);
        }
    }
}