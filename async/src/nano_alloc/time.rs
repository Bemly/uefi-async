use core::pin::Pin;
use core::task::{Context, Poll};
use crate::common::tick;
use alloc::boxed::Box;
use crate::calc_freq_blocking;

pub struct Timeout<'bemly_, Content> {
    future: Pin<Box<dyn Future<Output = Content> + 'bemly_>>,
    deadline: u64,
}

impl<'bemly_, Content> Timeout<'bemly_, Content> {
    pub fn new(future: impl Future<Output = Content> + 'bemly_, duration: u64) -> Self {
        Self { future: Box::pin(future), deadline: tick() + duration }
    }
}

impl<'bemly_, Content> Future for Timeout<'bemly_, Content> {
    type Output = Result<Content, ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if tick() >= self.deadline { return Poll::Ready(Err(())) }

        // 这里的 self.future 本身就是 Pin 过的，直接调用 poll 即可
        match self.future.as_mut().poll(cx) {
            Poll::Ready(val) => Poll::Ready(Ok(val)),
            Poll::Pending => Poll::Pending,
        }
    }
}
pub trait TimeoutExt<'bemly_>: Future + Sized {
    fn timeout(self, duration_ticks: u64) -> Timeout<'bemly_, Self::Output> where Self: 'bemly_ {
        Timeout {
            future: Box::pin(self),
            deadline: tick() + duration_ticks,
        }
    }
}
impl<F: Future + Sized> TimeoutExt<'_> for F {}

///
/// Usage:
/// ```rust,no_run
/// async fn blink_led_task(cpu_freq: u64) {
///     loop {
///         set_led(true);
///         WaitTimer::from_ms(500, cpu_freq).await;
///
///         set_led(false);
///         WaitTimer::from_ms(500, cpu_freq).await;
///     }
/// }
/// ```
///
pub struct WaitTimer (u64);

impl WaitTimer {
    /// 创建一个等待指定 ticks 数量的计时器
    pub fn new(ticks_to_wait: u64) -> Self { Self(tick() + ticks_to_wait) }

    /// 基于毫秒的便捷构造函数
    pub fn from_ms(ms: u64, freq: u64) -> Self {
        let ticks = (ms * freq) / 1000;
        Self::new(ticks)
    }
}

impl Future for WaitTimer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if tick() >= self.0 { Poll::Ready(()) } else { Poll::Pending }
    }
}

pub trait WaitTimerParamExt {
    fn ms(self, freq: u64) -> WaitTimer;
    fn us(self, freq: u64) -> WaitTimer;
}

impl WaitTimerParamExt for u64 {
    fn ms(self, freq: u64) -> WaitTimer { WaitTimer::from_ms(self, freq) }
    fn us(self, freq: u64) -> WaitTimer { WaitTimer::new((self * freq) / 1_000_000) }
}

pub trait WaitTimerBlockingExt {
    fn ms(self) -> WaitTimer;
    fn us(self) -> WaitTimer;
}

impl WaitTimerBlockingExt for u64 {
    fn ms(self) -> WaitTimer { WaitTimer::from_ms(self, calc_freq_blocking()) }
    fn us(self) -> WaitTimer { WaitTimer::new((self * calc_freq_blocking()) / 1_000_000) }
}