#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(old_rsp: *mut u64, new_rsp: u64) {
    core::arch::naked_asm!(
        // --- 1. 保存当前任务的现场 ---
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // 保存当前的栈指针到 old_rsp 指向的内存
        "mov [rdi], rsp",

        // --- 2. 切换到新任务的现场 ---
        // 加载新任务的栈指针
        "mov rsp, rsi",

        // 弹出新任务之前保存的寄存器
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // 执行 ret，此时会弹出新任务栈上的 RIP，实现跳转
        "ret",
    );
}