pub mod keyboard;
pub mod executor;

use core::{future::Future, pin::Pin};
use core::task::{Context, Poll, Waker};
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::{boxed::Box, sync::Arc, collections::BTreeMap};
use spin::{Mutex, RwLock};
use lazy_static::lazy_static;

// =========================================================
// 1. 工业级 IPC 消息定义
// =========================================================
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub from: u64,         // 发送者 TaskId
    pub to: u64,           // 接收者 TaskId
    pub label: u64,        // 协议标签 (1: 打印, 2: 退出...)
    pub payload: [u64; 2], // 16 字节固定载荷
}

pub static CURRENT_TASK_ID: AtomicU64 = AtomicU64::new(0);
// =========================================================
// 2. 任务状态机
// =========================================================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,       // 就绪
    Blocked,     // 阻塞：正在等待 IPC 消息
    Terminated,  // 已终止
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
// 3. 工业级 Task 结构 (修复了 Send 约束报错)
// =========================================================
pub struct Task {
    pub id: TaskId,
    /// 【核心修复】：增加 + Send 约束
    /// 只有满足 Send 的 Future 才能在全局 RwLock 中安全传输
    pub future: Mutex<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    pub state: Mutex<TaskState>,
    pub mailbox: Mutex<Option<Message>>,
    pub waker: Mutex<Option<Waker>>,
}

impl Task {
    /// 创建并注册任务：增加了 Send 约束
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Arc<Self> {
        Arc::new(Task {
            id: TaskId::new(),
            future: Mutex::new(Box::pin(future)),
            state: Mutex::new(TaskState::Ready),
            mailbox: Mutex::new(None),
            waker: Mutex::new(None),
        })
    }

    /// 执行轮询
    pub fn poll(&self, context: &mut Context) -> Poll<()> {
        let mut future = self.future.lock();
        future.as_mut().poll(context)
    }
}

// =========================================================
// 4. 全局任务管理器 (Registry)
// =========================================================
lazy_static! {
    /// 全局任务表：允许 Syscall 模块通过 ID 寻找进程
    /// 由于 Task 现在是 Send + Sync 的，这里不会再报线程安全错误
    pub static ref TASK_REGISTRY: RwLock<BTreeMap<TaskId, Arc<Task>>> = 
        RwLock::new(BTreeMap::new());
}

/// 辅助函数：快速注册任务
pub fn register_task(task: Arc<Task>) {
    TASK_REGISTRY.write().insert(task.id, task);
}

// =========================================================
// 5. IPC 异步原语 (支持在内核任务中 await 消息)
// =========================================================
pub struct IpcReceiveFuture {
    pub task: Arc<Task>,
}

impl IpcReceiveFuture {
    pub fn new(task: Arc<Task>) -> Self {
        Self { task }
    }
}

impl Future for IpcReceiveFuture {
    type Output = Message;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut mailbox = self.task.mailbox.lock();
        
        if let Some(msg) = mailbox.take() {
            // 收到信件：切换状态并返回 Ready
            *self.task.state.lock() = TaskState::Ready;
            Poll::Ready(msg)
        } else {
            // 没收到：存下 Waker，标记 Blocked 并返回 Pending
            // 内核执行器看到 Blocked 状态会跳过此任务，直到被 SYS_SEND 唤醒
            *self.task.waker.lock() = Some(cx.waker().clone());
            *self.task.state.lock() = TaskState::Blocked;
            Poll::Pending
        }
    }
}