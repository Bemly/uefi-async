extern crate alloc;
use crate::util::{calc_freq_blocking, tick};
use core::hint::spin_loop;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use alloc::boxed::Box;

pub struct TaskNode<'bemly_> {
    future: Pin<Box<dyn Future<Output = ()> + 'bemly_>>,
    interval: u64,
    next_run_time: u64,
    // 指向链表中的下一个任务
    next: Option<&'bemly_ mut TaskNode<'bemly_>>,
}
impl<'bemly_> TaskNode<'bemly_> {
    pub fn new(future: Pin<Box<dyn Future<Output = ()> + 'bemly_>>, freq: u64) -> Self {
        Self { future, interval: freq, next_run_time: 0, next: None }
    }
}

pub struct Executor<'bemly_> { head: Option<&'bemly_ mut TaskNode<'bemly_>>, freq: u64 }

impl<'bemly_> Executor<'bemly_> {
    pub fn new() -> Self { Self{ head: None, freq: calc_freq_blocking() } }
    pub fn add(&mut self, node: &'bemly_ mut TaskNode<'bemly_>) -> &mut Self {
        node.interval = self.freq / node.interval;
        node.next = self.head.take();
        self.head = Some(node);
        self
    }

    pub fn run(&mut self) {
        let mut cx = Context::from_waker(&Waker::noop());

        loop {
            let time = tick();

            // 获取 head 的可变引用，用于开始遍历
            let mut cursor = &mut self.head;

            // 遍历链表
            while let Some(node) = cursor {
                if time >= node.next_run_time {

                    // 2. Poll 异步任务
                    match node.future.as_mut().poll(&mut cx) {
                        Poll::Ready(_) => {
                            *cursor = node.next.take();
                            continue;
                        }
                        Poll::Pending => {
                            // 任务未完成：更新下一次运行时间点
                            node.next_run_time = time + node.interval;
                        }
                    }
                }

                // 3. 移动到下一个节点的引用槽位
                // Re-borrow
                cursor = match { cursor } {
                    Some(node) => &mut node.next,
                    None => break,
                };
            }

            // 这里的 spin_loop 放在整个链表轮询完一次之后
            spin_loop();
        }
    }
}