use conquer_once::spin::OnceCell; // 确保 Cargo.toml 有这个依赖
use crossbeam_queue::ArrayQueue;
use core::{pin::Pin, task::{Poll, Context}};
use futures_util::stream::Stream;
use futures_util::task::AtomicWaker;

/// 1. 使用 OnceCell 包裹队列。OnceCell 允许我们定义一个空的静态变量，
/// 然后在运行时（ScancodeStream::new 时）初始化它一次。
static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();

/// 2. 唤醒器保持不变
static WAKER: AtomicWaker = AtomicWaker::new();

/// 由中断处理程序调用。通过 try_get 确保队列已初始化
pub fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if let Err(_) = queue.push(scancode) {
            // 队列满时丢弃，不打印以保持中断处理速度
        } else {
            WAKER.wake();
        }
    }
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        // 3. 在第一次创建 Stream 时初始化全局队列
        SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(100))
            .expect("ScancodeStream::new 只能被调用一次");
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE.try_get().expect("键盘队列未初始化");

        if let Some(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
        }

        WAKER.register(cx.waker());

        if let Some(scancode) = queue.pop() {
            WAKER.take();
            Poll::Ready(Some(scancode))
        } else {
            Poll::Pending
        }
    }
}