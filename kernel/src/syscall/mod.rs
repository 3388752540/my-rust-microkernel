use x86_64::registers::model_specific::{LStar, Star, SFMask, Efer, EferFlags};
use x86_64::VirtAddr;
use crate::gdt;
 pub use crate::task::{TaskId, Message, TaskState, TASK_REGISTRY, CURRENT_TASK_ID};
use core::sync::atomic::Ordering;
use spin::Mutex;

pub static INIT_MAILBOX: Mutex<Option<Message>> = Mutex::new(None);
/// 初始化系统调用硬件配置
pub fn init() {
    unsafe {
        // 1. LSTAR: 设置 syscall 跳转目标
        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));

        // 2. STAR: 设置基准选择子
        let selectors = gdt::get_selectors();
        Star::write(
            selectors.user_code_selector,
            selectors.user_data_selector,
            selectors.kernel_code_selector,
            selectors.kernel_data_selector,
        ).unwrap();

        // 3. SFMASK: 进入内核时自动关闭中断 (IF) 以保护内核栈操作
        SFMask::write(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG);

        // 4. EFER: 开启 SCE
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
}

/// 系统调用底层入口 (汇编)
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    
        core::arch::naked_asm!(
            // 1. 保护所有用户态寄存器
            "push r11", "push rcx", "push rbp", "push rbx",
            "push rdi", "push rsi", "push rdx",
            "push r8", "push r9", "push r10", "push r12", "push r13", "push r14", "push r15",

            // 2. 映射参数到 Rust C-ABI (rdi, rsi, rdx)
            // 用户: rax(ID), rdi(arg1), rsi(arg2)
            "mov rdx, rsi", // arg2 -> rdx
            "mov rsi, rdi", // arg1 -> rsi
            "mov rdi, rax", // ID   -> rdi
            
            "call {handler}",

            // 3. 恢复现场
            "pop r15", "pop r14", "pop r13", "pop r12",
            "pop r10", "pop r9", "pop r8",
            "pop rdx", "pop rsi", "pop rdi",
            "pop rbx", "pop rbp", "pop rcx", "pop r11",

            "sysretq",
            handler = sym handle_syscall,
        );
    
}

/// 系统调用业务分发器
#[unsafe(no_mangle)]
extern "C" fn handle_syscall(id: u64, arg1: u64, arg2: u64) -> u64 {
    match id {
        // -----------------------------------------------------
        // 1. 基础打印 (SYS_PRINT)
        // -----------------------------------------------------
        1 => {
            crate::print!("{}", arg1 as u8 as char);
            0
        },

        // -----------------------------------------------------
        // 2. 发送消息 (SYS_SEND) - 目标 PID 在 arg1, 消息指针在 arg2
        // -----------------------------------------------------
        10 => {
            let target_id = TaskId(arg1);
            let msg_ptr = arg2 as *const Message;
            let msg = unsafe { *msg_ptr }; 

            let registry = TASK_REGISTRY.read();
            if let Some(target_task) = registry.get(&target_id) {
                // 将消息存入目标信箱
                *target_task.mailbox.lock() = Some(msg);

                // 如果目标正在阻塞，将其唤醒
                let mut state = target_task.state.lock();
                if *state == TaskState::Blocked {
                    *state = TaskState::Ready;
                    if let Some(waker) = target_task.waker.lock().take() {
                        waker.wake();
                    }
                }
                0 // 成功
            } else {
                1 // 错误：目标进程不存在
            }
        },

        // -----------------------------------------------------
        // 3. 接收消息 (SYS_RECV) - 存储地址在 arg1
        // -----------------------------------------------------
        // SYS_RECV
        11 => {
            let msg_ptr = arg1 as *mut Message;
            // 尝试获取信箱锁
            if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
                if let Some(msg) = mailbox.take() {
                    unsafe { core::ptr::write_volatile(msg_ptr, msg); }
                    1 // 成功拿到信
                } else {
                    0 // 信箱空的
                }
            } else {
                0 // 锁被占用了，当作没收到（下次循环再试）
            }
        },

        // -----------------------------------------------------
        // 4. 让出 CPU (SYS_YIELD)
        // -----------------------------------------------------
        12 => {
            // 在单核环境下，yield 就是执行 hlt 等待下一个中断
            // 这会让 Executor 有机会去 Poll 其他任务
            x86_64::instructions::interrupts::enable_and_hlt();
            0
        },

        _ => u64::MAX
    }
}