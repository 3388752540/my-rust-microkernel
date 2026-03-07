use x86_64::registers::model_specific::{LStar, Star, SFMask, Efer, EferFlags};
use x86_64::VirtAddr;
use crate::gdt;
use core::arch::naked_asm;

/// 初始化系统调用硬件配置 (MSR 寄存器设置)
pub fn init() {
    unsafe {
        // 1. LSTAR: 设置执行 syscall 指令后，CPU 跳转到的内核代码入口地址
        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));

        // 2. STAR: 设置内核与用户态的选择子基址
        // 该寄存器规定了 sysret 返回用户态时使用的段选择子偏移
        let selectors = gdt::get_selectors();
        Star::write(
            selectors.user_code_selector,
            selectors.user_data_selector,
            selectors.kernel_code_selector,
            selectors.kernel_data_selector,
        ).unwrap();

        // 3. SFMASK: 系统调用发生时自动屏蔽的标志位
        // 必须屏蔽 Interrupt Flag (0x200)，防止在系统调用入口处被中断打断现场保护
        SFMask::write(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG);

        // 4. EFER: 开启 SCE (System Call Extensions) 扩展位
        // 这是 x86_64 处理器支持 syscall/sysret 指令的总开关
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
}

/// 系统调用的底层汇编入口点
/// 遵循 Rust 2024 规范：使用 #[unsafe(naked)] 和 naked_asm!
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    
        naked_asm!(
            // --- 1. 寻找内核栈 ---
            // 此时 RSP 指向的是用户态栈，我们需要切换到内核栈
            // 使用 swapgs 将用户态 GS 与 TSS 中定义的内核 GS 交换
            "swapgs",
            
            // 为了简化实现，这里暂时假设用户栈依然可用且安全
            // 在生产级微内核中，此时应从 TSS 读取 RSP0 并切换栈指针

            // --- 2. 保护现场 ---
            // syscall 指令会破坏两个寄存器：
            // RCX: 存储用户态跳转前的 RIP (返回地址)
            // R11: 存储用户态跳转前的 RFLAGS (状态标志)
            "push rcx",
            "push r11",
            
            // 按照 SysV ABI 保存其他可能被 Rust 函数破坏的调用者保存寄存器
            "push rdi",
            "push rsi",
            "push rdx",
            "push r10",
            "push r8",
            "push r9",

            // --- 3. 传递参数并调用 Rust 逻辑 ---
            // 根据 x86_64 约定，rax 是调用号
            // 我们将其移动到第一个参数寄存器 rdi 中传给 handle_syscall
            "mov rdi, rax",
            "call {handle_syscall}",

            // --- 4. 恢复现场 ---
            "pop r9",
            "pop r8",
            "pop r10",
            "pop rdx",
            "pop rsi",
            "pop rdi",
            "pop r11",
            "pop rcx",

            // 还原 GS 寄存器，回到用户态上下文
            "swapgs",

            // --- 5. 返回用户态 ---
            // sysretq 会将 RCX 弹回 RIP，将 R11 弹回 RFLAGS，并将权限降回 Ring 3
            "sysretq",
            handle_syscall = sym handle_syscall,
        );
    
}

/// 真正的系统调用业务逻辑分发器
/// id: 由用户态通过 RAX 寄存器传入
extern "C" fn handle_syscall(id: u64) {
    match id {
        // 系统调用号 1: 基础打印功能
        1 => {
            crate::print!("\n\x1b[36;1m[SYSCALL]\x1b[0m Hello from Ring 3 via Syscall!");
        },
        // 系统调用号 2: 退出控制 (可选)
        2 => {
            crate::println!("\n[SYSCALL] User program requested exit.");
            // 可以在这里实现进程销毁逻辑
        },
        _ => {
            crate::println!("\n\x1b[31m[SYSCALL ERROR]\x1b[0m Unknown ID: {}", id);
        }
    }
}