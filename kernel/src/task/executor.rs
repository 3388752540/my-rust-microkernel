use super::{Task, TaskId};
use alloc::{collections::BTreeMap, sync::Arc, task::Wake};
use core::task::{Context, Poll, Waker};
use crossbeam_queue::ArrayQueue;
use x86_64::instructions::interrupts;

pub struct Executor {
    // 存储所有活跃任务
    tasks: BTreeMap<TaskId, Task>,
    // 任务 ID 队列，存放被唤醒（准备好运行）的任务
    task_queue: Arc<ArrayQueue<TaskId>>,
    // 缓存 Waker，避免重复创建
    waker_cache: BTreeMap<TaskId, Waker>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            tasks: BTreeMap::new(),
            task_queue: Arc::new(ArrayQueue::new(100)), // 最多缓冲 100 个唤醒信号
            waker_cache: BTreeMap::new(),
        }
    }

    /// 将任务加入执行器
    pub fn spawn(&mut self, task: Task) {
        let task_id = task.id;
        if self.tasks.insert(task_id, task).is_some() {
            panic!("Task with same ID already in tasks");
        }
        self.task_queue.push(task_id).expect("Queue full");
    }

    /// 运行就绪队列中的任务
    fn run_ready_tasks(&mut self) {
        // 解构 self 以避开借用检查器的限制
        let Self { tasks, task_queue, waker_cache } = self;

        while let Some(task_id) = task_queue.pop() {
            let task = match tasks.get_mut(&task_id) {
                Some(task) => task,
                None => continue, // 任务可能已完成并被移除
            };

            // 获取或创建 Waker
            let waker = waker_cache
                .entry(task_id)
                .or_insert_with(|| TaskWaker::new(task_id, task_queue.clone()));
            
            let mut context = Context::from_waker(waker);

            // 核心动作：Poll 任务
            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    // 任务执行完毕，清理资源
                    tasks.remove(&task_id);
                    waker_cache.remove(&task_id);
                }
                Poll::Pending => {} // 任务还没完，等下次唤醒
            }
        }
    }

    /// 执行器主循环
    pub fn run(&mut self) -> ! {
        loop {
            self.run_ready_tasks();
            // 如果所有就绪任务都跑完了，尝试让 CPU 休息
            self.sleep_if_idle();
        }
    }

    fn sleep_if_idle(&self) {
        // 这里必须禁用中断再检查队列，防止“检查完-还没睡-中断来了”导致的死锁
        interrupts::disable();
        if self.task_queue.is_empty() {
            // hlt 指令会原子性地开启中断并让 CPU 进入睡眠
            interrupts::enable_and_hlt();
        } else {
            interrupts::enable();
        }
    }
}

/// 任务唤醒器：负责将任务 ID 重新丢进就绪队列
struct TaskWaker {
    task_id: TaskId,
    task_queue: Arc<ArrayQueue<TaskId>>,
}

impl TaskWaker {
    fn new(task_id: TaskId, task_queue: Arc<ArrayQueue<TaskId>>) -> Waker {
        Waker::from(Arc::new(TaskWaker { task_id, task_queue }))
    }

    fn wake_task(&self) {
        self.task_queue.push(self.task_id).expect("task_queue full");
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) { self.wake_task(); }
    fn wake_by_ref(self: &Arc<Self>) { self.wake_task(); }
}