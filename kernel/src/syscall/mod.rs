use x86_64::registers::model_specific::{LStar, Star, SFMask, Efer, EferFlags};
use x86_64::VirtAddr;
use crate::gdt;
pub use crate::task::{TaskId, Message, TaskState, TASK_REGISTRY, CURRENT_TASK_ID};
use core::sync::atomic::Ordering;
use spin::{Mutex, RwLock};
use alloc::collections::BTreeMap;
use lazy_static::lazy_static;

// ==========================================
// 1. 全局服务注册表 (Name Server / Registry)
// 工业级微内核的核心：解耦 PID，通过服务名（u64）通信
// ==========================================
lazy_static! {
    static ref SERVICE_REGISTRY: RwLock<BTreeMap<u64, TaskId>> = 
        RwLock::new(BTreeMap::new());
}

// 专门给初始化进程准备的全局信箱（用户态驱动模式的关键中转）
pub static INIT_MAILBOX: Mutex<Option<Message>> = Mutex::new(None);

/// 初始化系统调用硬件配置 (MSR 寄存器设置)
pub fn init() {
    unsafe {
        // 1. LSTAR: 设置 syscall 指令跳转的内核入口
        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));

        // 2. STAR: 设置内核与用户态的选择子基址 (硬件要求顺序：内核在前，用户在后)
        let selectors = gdt::get_selectors();
        Star::write(
            selectors.user_code_selector,
            selectors.user_data_selector,
            selectors.kernel_code_selector,
            selectors.kernel_data_selector,
        ).unwrap();

        // 3. SFMASK: 进入内核时自动屏蔽中断位 (IF位)，保护现场不被嵌套中断破坏
        SFMask::write(x86_64::registers::rflags::RFlags::INTERRUPT_FLAG);

        // 4. EFER: 开启 SCE (System Call Extensions) 扩展位
        Efer::update(|f| f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
}

/// 系统调用底层入口 (Naked Assembly Wrapper)
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    unsafe {
        core::arch::naked_asm!(
            // --- 1. 保存用户态现场 ---
            // syscall 指令将 RIP 存入 RCX，RFLAGS 存入 R11
            "push r11",
            "push rcx",
            "push rbp",
            "push rbx",
            "push rdi",
            "push rsi",
            "push rdx",
            "push r10",
            "push r8",
            "push r9",
            "push r12",
            "push r13",
            "push r14",
            "push r15",

            // --- 2. 映射参数到 Rust C-ABI (RDI, RSI, RDX) ---
            // 用户约定: rax=ID, rdi=arg1, rsi=arg2
            // Rust 约定: rdi=ID, rsi=arg1, rdx=arg2
            "mov rdx, rsi", // 用户的 arg2 (rsi) -> Rust 的第3个参数 rdx
            "mov rsi, rdi", // 用户的 arg1 (rdi) -> Rust 的第2个参数 rsi
            "mov rdi, rax", // 用户的 ID   (rax) -> Rust 的第1个参数 rdi
            
            "call {handler}",

            // --- 3. 恢复现场 ---
            "pop r15", "pop r14", "pop r13", "pop r12",
            "pop r9", "pop r8", "pop r10",
            "pop rdx", "pop rsi", "pop rdi",
            "pop rbx", "pop rbp",
            "pop rcx",
            "pop r11",

            // --- 4. 返回用户态 ---
            "sysretq",
            handler = sym handle_syscall,
        );
    }
}

/// 系统调用业务分发器
#[unsafe(no_mangle)]
extern "C" fn handle_syscall(id: u64, arg1: u64, arg2: u64) -> u64 {
    match id {
        // 1. 基础打印: SYS_PRINT (arg1: char)
        1 => {
            crate::print!("{}", arg1 as u8 as char);
            0
        },

        // 2. 进程自愿退出: SYS_EXIT
        2 => {
            let current_id = TaskId(CURRENT_TASK_ID.load(Ordering::Relaxed));
            let registry = TASK_REGISTRY.read();
            if let Some(task) = registry.get(&current_id) {
                // 标记为终止状态，调度器在下一次滴答时会跳过它
                *task.state.lock() = TaskState::Terminated;
                crate::println!("\n\x1b[33m[KERNEL] Process {} exited gracefully.\x1b[0m", current_id.0);
            }
            0
        },

        // 10. 核心 IPC: SYS_SEND (arg1: TargetPID, arg2: MsgPtr)
        10 => {
            let target_id = TaskId(arg1);
            let msg_ptr = arg2 as *const Message;
            
            // 简单的指针范围校验（安全增强）
            if arg2 < 0x2000_0000 { return 2; } 
            
            let msg = unsafe { *msg_ptr }; 

            let registry = TASK_REGISTRY.read();
            if let Some(target_task) = registry.get(&target_id) {
                // 将消息存入目标的受锁保护的信箱
                let mut mailbox = target_task.mailbox.lock();
                *mailbox = Some(msg);

                // 唤醒目标进程 (如果它正在 Blocked 等消息)
                let mut state = target_task.state.lock();
                if *state == TaskState::Blocked {
                    *state = TaskState::Ready;
                    if let Some(waker) = target_task.waker.lock().take() {
                        waker.wake(); // 触发 Waker 重新加入执行器队列
                    }
                }
                0 // 发送成功
            } else {
                1 // 错误：目标不存在
            }
        },

        // 11. 核心 IPC: SYS_RECV (arg1: MsgBufferPtr)
        11 => {
            let msg_ptr = arg1 as *mut Message;
            // 工业化设计：优先处理内核转发给 init 的硬件驱动消息 (INIT_MAILBOX)
            // 以后可以扩展为每个 Task 拥有独立的私有 mailbox
            if let Some(mut mailbox) = INIT_MAILBOX.try_lock() {
                if let Some(msg) = mailbox.take() {
                    unsafe { core::ptr::write_volatile(msg_ptr, msg); }
                    return 1; 
                }
            }
            0 // 信箱为空
        },

        // 12. 协作调度: SYS_YIELD
        12 => {
            // 让出当前 CPU 时间片，挂起直到下一个时钟/外部中断
            x86_64::instructions::interrupts::enable_and_hlt();
            0
        },

        // 13. 调度策略: SYS_SET_PRIORITY (arg1: NewTicks)
        13 => {
            let current_id = TaskId(CURRENT_TASK_ID.load(Ordering::Relaxed));
            let registry = TASK_REGISTRY.read();
            if let Some(task) = registry.get(&current_id) {
                // 动态修改任务结构体中的优先级字段 (策略与机制分离的体现)
                task.priority.store(arg1, Ordering::Relaxed);
                0
            } else { 1 }
        },

        // 14. 服务发布: SYS_REG_SERVICE (arg1: NameU64)
        14 => {
            let pid = TaskId(CURRENT_TASK_ID.load(Ordering::Relaxed));
            SERVICE_REGISTRY.write().insert(arg1, pid);
            0
        },

        // 15. 服务发现: SYS_GET_SERVICE_PID (arg1: NameU64)
        15 => {
            let reg = SERVICE_REGISTRY.read();
            if let Some(pid) = reg.get(&arg1) {
                pid.0
            } else {
                u64::MAX // 返回未找到标志
            }
        },

        _ => u64::MAX
    }
}