use core::pin::Pin;
use core::task::{Context, Poll};
use crate::common::tick;
use alloc::boxed::Box;

pub struct Timeout<'a, T> {
    future: Pin<Box<dyn Future<Output = T> + 'a>>,
    deadline: u64,
}

impl<'a, T> Future for Timeout<'a, T> {
    type Output = Result<T, ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if tick() >= self.deadline {
            return Poll::Ready(Err(()));
        }

        // 这里的 self.future 本身就是 Pin 过的，直接调用 poll 即可
        match self.future.as_mut().poll(cx) {
            Poll::Ready(val) => Poll::Ready(Ok(val)),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct WaitTimer {
    deadline: u64,
}

impl WaitTimer {
    /// 创建一个等待指定 ticks 数量的计时器
    pub fn new(ticks_to_wait: u64) -> Self { Self { deadline: tick() + ticks_to_wait } }

    /// 基于毫秒的便捷构造函数
    pub fn from_ms(ms: u64, freq: u64) -> Self {
        let ticks = (ms * freq) / 1000;
        Self::new(ticks)
    }
}

impl Future for WaitTimer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if tick() >= self.deadline { Poll::Ready(()) } else { Poll::Pending }
    }
}