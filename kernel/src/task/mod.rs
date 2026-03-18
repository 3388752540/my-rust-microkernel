pub mod keyboard;
pub mod executor;

use core::{future::Future, pin::Pin};
use core::task::{Context, Poll, Waker};
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::{boxed::Box, sync::Arc, collections::BTreeMap};
use spin::{Mutex, RwLock};
use lazy_static::lazy_static;
use x86_64::VirtAddr;

// =========================================================
// 1. 工业级 IPC 消息定义
// =========================================================
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub label: u64,
    pub payload: [u64; 2],
}

/// 记录当前 CPU 正在执行的任务 ID (用于系统调用识别身份)
pub static CURRENT_TASK_ID: AtomicU64 = AtomicU64::new(0);

// =========================================================
// 2. 任务状态机
// =========================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,       // 就绪：可以分配 CPU 时间
    Blocked,     // 阻塞：正在等待 IPC 消息
    Terminated,  // 终止：正在等待回收
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1); // 0 留给内核初始上下文
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// =========================================================
// 3. 核心 Task 结构 (TCB - Task Control Block)
// =========================================================
pub struct Task {
    pub id: TaskId,
    /// 任务状态
    pub state: Mutex<TaskState>,
    /// 【策略分离核心】优先级：代表该进程获得一次运行机会所需的时钟滴答数
    /// 数值越小，优先级越高（运行越频繁）
    pub priority: AtomicU64,
    /// 内核异步 Future (用于处理内核后台监控、键盘驱动等)
    pub future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    /// 【抢占核心】保存挂起时的内核栈指针 (Atomic 确保中断安全)
    pub kernel_rsp: AtomicU64, 
    /// 【抢占核心】内核栈的顶端 (RSP0) - 切换后必须写入 TSS.RSP0
    pub kernel_stack_top: u64,
    /// IPC 信箱 (每个进程唯一的接收缓冲区)
    pub mailbox: Mutex<Option<Message>>,
    /// 唤醒器 (用于 SYS_SEND 精准唤醒目标)
    pub waker: Mutex<Option<Waker>>,
}

impl Task {
    /// 创建内核后台异步任务
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Arc<Self> {
        // 为内核任务也分配一个独立的内核栈，使其可以被抢占
        const KSTACK_SIZE: usize = 4096 * 4;
        let kstack = Box::leak(Box::new([0u8; KSTACK_SIZE]));
        let kstack_top = VirtAddr::from_ptr(kstack.as_ptr()) + KSTACK_SIZE;

        Arc::new(Task {
            id: TaskId::new(),
            state: Mutex::new(TaskState::Ready),
            priority: AtomicU64::new(5), // 默认优先级 5
            future: Mutex::new(Box::pin(future)),
            kernel_rsp: AtomicU64::new(kstack_top.as_u64()),
            kernel_stack_top: kstack_top.as_u64(),
            mailbox: Mutex::new(None),
            waker: Mutex::new(None),
        })
    }

    /// 【Phase 7 核心：用户进程初始化】
    pub fn create_user_process(entry: VirtAddr, user_stack_top: VirtAddr) -> Arc<Self> {
        const KSTACK_SIZE: usize = 4096 * 8; // 用户进程内核栈给 32KB
        let kstack = Box::leak(Box::new([0u8; KSTACK_SIZE]));
        let kstack_top = VirtAddr::from_ptr(kstack.as_ptr()) + KSTACK_SIZE;

        let mut rsp = kstack_top.as_u64();
        
        unsafe {
            let mut push = |val: u64| {
                rsp -= 8;
                (rsp as *mut u64).write_volatile(val);
            };

            // --- 按照 interrupts.rs 中 handler_wrapper 宏的顺序逆向压栈 ---
            
            // 1. iretq 框架
            let selectors = crate::gdt::get_selectors();
            push(selectors.user_data_selector.0 as u64 | 3); // SS
            push(user_stack_top.as_u64());                  // RSP
            push(0x202);                                    // RFLAGS (IF=1)
            push(selectors.user_code_selector.0 as u64 | 3); // CS
            push(entry.as_u64());                           // RIP

            // 2. 15 个通用寄存器 (对应 handler_wrapper 里的 push rax...push r15)
            // 初始全部设为 0
            for _ in 0..15 { push(0); }
        }

        Arc::new(Task {
            id: TaskId::new(),
            state: Mutex::new(TaskState::Ready),
            priority: AtomicU64::new(5),
            future: Mutex::new(Box::pin(async {})), // 用户进程不使用内核 future
            kernel_rsp: AtomicU64::new(rsp), 
            kernel_stack_top: kstack_top.as_u64(),
            mailbox: Mutex::new(None),
            waker: Mutex::new(None),
        })
    }

    pub fn poll(&self, context: &mut Context) -> Poll<()> {
        let mut future = self.future.lock();
        future.as_mut().poll(context)
    }

    /// 检查任务是否可以被调度
    pub fn is_runnable(&self) -> bool {
        *self.state.lock() == TaskState::Ready
    }
}

// =========================================================
// 4. 全局任务注册表
// =========================================================
lazy_static! {
    /// 全局任务索引表：支持系统调用路径下的跨进程操作
    pub static ref TASK_REGISTRY: RwLock<BTreeMap<TaskId, Arc<Task>>> = 
        RwLock::new(BTreeMap::new());
}

/// 将任务加入全局管理系统
pub fn register_task(task: Arc<Task>) {
    TASK_REGISTRY.write().insert(task.id, task);
}

// =========================================================
// 5. 辅助功能：IPC 接收 Future
// =========================================================

pub struct IpcReceiveFuture {
    pub task: Arc<Task>,
}

impl IpcReceiveFuture {
    pub fn new(task: Arc<Task>) -> Self { Self { task } }
}

impl Future for IpcReceiveFuture {
    type Output = Message;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut mailbox = self.task.mailbox.lock();
        if let Some(msg) = mailbox.take() {
            *self.task.state.lock() = TaskState::Ready;
            Poll::Ready(msg)
        } else {
            // 注册 Waker 并进入 Blocked 状态
            *self.task.waker.lock() = Some(cx.waker().clone());
            *self.task.state.lock() = TaskState::Blocked;
            Poll::Pending
        }
    }
}