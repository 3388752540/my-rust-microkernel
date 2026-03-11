use super::{Task, TaskId, TaskState, TASK_REGISTRY};
use alloc::{collections::BTreeMap, sync::Arc, task::Wake};
use core::task::{Context, Poll, Waker};
use crossbeam_queue::ArrayQueue;
use x86_64::instructions::interrupts;
use spin::Mutex;
use lazy_static::lazy_static;

// =========================================================
// 【核心修改】全局执行器单例
// =========================================================
lazy_static! {
    /// 全局内核执行器，允许在中断处理函数中被安全访问
    pub static ref KERNEL_EXECUTOR: Mutex<Executor> = Mutex::new(Executor::new());
}

pub struct Executor {
    tasks: BTreeMap<TaskId, Arc<Task>>,
    task_queue: Arc<ArrayQueue<TaskId>>,
    waker_cache: BTreeMap<TaskId, Waker>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            tasks: BTreeMap::new(),
            task_queue: Arc::new(ArrayQueue::new(100)),
            waker_cache: BTreeMap::new(),
        }
    }

    /// 暴露给外部的静态 spawn 方法
    pub fn spawn_static(task: Arc<Task>) {
        KERNEL_EXECUTOR.lock().spawn(task);
    }

    pub fn spawn(&mut self, task: Arc<Task>) {
        let task_id = task.id;
        if self.tasks.insert(task_id, task.clone()).is_some() {
            panic!("Task with same ID already in executor");
        }
        TASK_REGISTRY.write().insert(task_id, task);
        self.task_queue.push(task_id).expect("Task queue full");
    }

    /// 【关键修改】改为 pub，以便中断处理程序调用
    /// 该函数现在负责“抽空”运行当前的就绪任务
    pub fn run_ready_tasks(&mut self) {
        let Self { tasks, task_queue, waker_cache } = self;

        while let Some(task_id) = task_queue.pop() {
            let task = match tasks.get(&task_id) {
                Some(t) => t,
                None => continue,
            };

            if *task.state.lock() != TaskState::Ready {
                continue;
            }

            // 记录当前执行的任务 ID，方便 Syscall 识别
            super::CURRENT_TASK_ID.store(task_id.0, core::sync::atomic::Ordering::Relaxed);

            let waker = waker_cache
                .entry(task_id)
                .or_insert_with(|| TaskWaker::new(task_id, task_queue.clone()));
            
            let mut context = Context::from_waker(waker);

            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    tasks.remove(&task_id);
                    TASK_REGISTRY.write().remove(&task_id);
                    waker_cache.remove(&task_id);
                }
                Poll::Pending => {
                    // 任务未完成，保持现状
                }
            }
        }
        
        // 运行完后清空当前任务 ID 标识（回到 IDLE 或 User Mode 状态）
        super::CURRENT_TASK_ID.store(0, core::sync::atomic::Ordering::Relaxed);
    }

    /// 此时的 run 方法不再是死循环，而是作为 main 的备用路径
    /// 但在并存模式下，主逻辑通常在 jump_to_user_mode 之后
    pub fn run(&mut self) -> ! {
        loop {
            self.run_ready_tasks();
            self.sleep_if_idle();
        }
    }

    fn sleep_if_idle(&self) {
        interrupts::disable();
        if self.task_queue.is_empty() {
            interrupts::enable_and_hlt();
        } else {
            interrupts::enable();
        }
    }
}

// --- TaskWaker 实现保持不变 ---
struct TaskWaker {
    task_id: TaskId,
    task_queue: Arc<ArrayQueue<TaskId>>,
}

impl TaskWaker {
    fn new(task_id: TaskId, task_queue: Arc<ArrayQueue<TaskId>>) -> Waker {
        Waker::from(Arc::new(TaskWaker { task_id, task_queue }))
    }
    fn wake_task(&self) {
        let _ = self.task_queue.push(self.task_id);
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) { self.wake_task(); }
    fn wake_by_ref(self: &Arc<Self>) { self.wake_task(); }
}