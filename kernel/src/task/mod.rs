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

/// 记录当前 CPU 正在执行的任务 ID
pub static CURRENT_TASK_ID: AtomicU64 = AtomicU64::new(0);

// =========================================================
// 2. 任务状态机
// =========================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,       
    Blocked,     
    Terminated,  
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

// =========================================================
// 3. 核心 Task 结构 (TCB)
// =========================================================
pub struct Task {
    pub id: TaskId,
    /// 内核异步 Future (用于内核态后台监控任务)
    pub future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    /// 任务状态
    pub state: Mutex<TaskState>,
    /// 【抢占核心】保存挂起时的内核栈指针 (Atomic 确保中断安全)
    /// 它始终指向栈上 15 个通用寄存器的起始位置
    pub kernel_rsp: AtomicU64, 
    /// 【抢占核心】内核栈的顶端 (RSP0) - 切换后必须写入 TSS.RSP0
    pub kernel_stack_top: u64,
    /// IPC 信箱
    pub mailbox: Mutex<Option<Message>>,
    /// 唤醒器 (用于唤醒 Blocked 状态的进程)
    pub waker: Mutex<Option<Waker>>,
}

impl Task {
    /// 创建内核后台异步任务
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Arc<Self> {
        Arc::new(Task {
            id: TaskId::new(),
            future: Mutex::new(Box::pin(future)),
            state: Mutex::new(TaskState::Ready),
            kernel_rsp: AtomicU64::new(0),
            kernel_stack_top: 0,
            mailbox: Mutex::new(None),
            waker: Mutex::new(None),
        })
    }

    /// 【Phase 7 核心：进程初始化】
    /// 构造一个看起来像“刚刚被中断保存过”的栈现场
    pub fn create_user_process(entry: VirtAddr, user_stack_top: VirtAddr) -> Arc<Self> {
        // 1. 分配独立的内核栈 (32KB)
        const KSTACK_SIZE: usize = 4096 * 8;
        let kstack = Box::leak(Box::new([0u8; KSTACK_SIZE]));
        let kstack_top = VirtAddr::from_ptr(kstack.as_ptr()) + KSTACK_SIZE;

        let mut rsp = kstack_top.as_u64();
        
        unsafe {
            let mut push = |val: u64| {
                rsp -= 8;
                (rsp as *mut u64).write_volatile(val);
            };

            // --- A. 构造 iretq 框架 (硬件要求顺序) ---
            // 当执行 iretq 时，CPU 会按此顺序弹出并切换特权级
            let selectors = crate::gdt::get_selectors();
            push(selectors.user_data_selector.0 as u64 | 3); // SS (用户数据段)
            push(user_stack_top.as_u64());                  // User RSP
            push(0x202);                                    // RFLAGS (开启中断 IF=1)
            push(selectors.user_code_selector.0 as u64 | 3); // CS (用户代码段)
            push(entry.as_u64());                           // RIP (程序入口)

            // --- B. 构造 15 个通用寄存器现场 ---
            // 对应 interrupts.rs 宏里的 pop r15...pop rax
            // 初始值全设为 0
            for _ in 0..15 { push(0); }
            
            // 此时 rsp 正好指向 rax 所在的地址
        }

        Arc::new(Task {
            id: TaskId::new(),
            future: Mutex::new(Box::pin(async {})), 
            state: Mutex::new(TaskState::Ready),
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
}

// =========================================================
// 4. 全局任务注册表
// =========================================================
lazy_static! {
    /// TASK_REGISTRY 存储了所有进程的 Arc 引用
    /// 这是微内核寻址、消息投递和调度的核心索引表
    pub static ref TASK_REGISTRY: RwLock<BTreeMap<TaskId, Arc<Task>>> = 
        RwLock::new(BTreeMap::new());
}

pub fn register_task(task: Arc<Task>) {
    TASK_REGISTRY.write().insert(task.id, task);
}

// =========================================================
// 5. IPC 接收 Future (工业级协程 IPC)
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
            *self.task.waker.lock() = Some(cx.waker().clone());
            *self.task.state.lock() = TaskState::Blocked;
            Poll::Pending
        }
    }
}