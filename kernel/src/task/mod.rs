pub mod keyboard;

use core::{future::Future, pin::Pin};
use core::task::{Context, Poll};
use core::sync::atomic::{AtomicU64, Ordering}; // 引入原子操作，确保多核安全
use alloc::boxed::Box;

// 导出执行器模块，使其在外部可用
pub mod executor;

/// 任务 ID 包装类
/// 使用原子计数器生成，确保即使在多核环境下每个任务都有唯一 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(u64);

impl TaskId {
    /// 生成一个新的唯一 TaskId
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        // 使用 fetch_add 原子递增。Relaxed 排序在生成唯一 ID 场景下已足够
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// 任务容器
/// 封装了一个异步 Future，并为其分配了唯一的 ID
pub struct Task {
    /// 任务的唯一标识符（现在被存起来了，消除了 unused 警告）
    pub(crate) id: TaskId, 
    /// 异步逻辑的状态机。使用 Pin<Box<...>> 是因为 Future 
    /// 可能包含指向自身的引用，在内存中必须保持位置固定。
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    /// 创建并初始化一个新任务
    /// future: 传入一个异步块或异步函数，例如 `async { ... }`
    pub fn new(future: impl Future<Output = ()> + 'static) -> Task {
        Task {
            id: TaskId::new(), // 自动分配唯一 ID
            future: Box::pin(future),
        }
    }

    /// 轮询任务状态 (Poll)
    /// 这是执行器 (Executor) 调用任务的核心接口
    pub(crate) fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}