
use core::pin::Pin;
use core::task::{Context, Poll};

pub const YIELD: _Yield = _Yield(false);
pub async fn yield_now() { _Yield(false).await }
pub struct Yield;
impl IntoFuture for Yield {
    type Output = ();
    type IntoFuture = _Yield;
    fn into_future(self) -> Self::IntoFuture { _Yield(false) }
}
pub struct _Yield(bool);
impl Future for _Yield {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.0 { Poll::Ready(()) } else { self.0 = true; Poll::Pending }
    }
}

pub struct Skip(pub usize);
impl Future for Skip {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.0 == 0 { Poll::Ready(()) } else { self.0 -= 1; Poll::Pending }
    }
}

pub struct Pacer {
    count: usize,
    countdown: usize,
}
impl Pacer {
    pub fn new(count: usize) -> Self { Pacer { count, countdown: count } }
    pub fn count_update(&mut self, count: usize) -> &mut Self { self.count = count; self }

    /// 最通用的接口：满足 count 次条件后，执行指定的让出量
    #[inline]
    pub async fn step(&mut self, yield_count: usize, repeat: bool) {
        if self.countdown > 0 { self.countdown -= 1 }
        else {
            if repeat { self.countdown = self.count }
            if yield_count == 0 { Yield.await } else { Skip(yield_count).await }
        }
    }

    /// 倒计时直到为 0，之后每次调用都会触发 Yield (让出控制权)
    #[inline(always)]
    pub async fn throttle(&mut self) { self.step(0, false).await }

    /// 每经过 count 次调用，就触发一次 Yield，并重置计数器
    #[inline(always)]
    pub async fn repeat(&mut self) { self.step(0, true).await }

    /// 每经过 count 次调用，连续让出 i 次
    #[inline(always)]
    pub async fn burst(&mut self, i: usize) { self.step(i, true).await }
}