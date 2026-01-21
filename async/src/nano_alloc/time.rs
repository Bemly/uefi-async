use crate::{tick, FREQ};
use alloc::boxed::Box;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Usage:
/// ```rust,no_run
/// use uefi_async::nano_alloc::time::Timeout;
///
///  async fn calc_1() {}
///  async fn calc_2() {
///     match Box::pin(calc_2()).timeout(Duration::from_secs(2).as_secs()).await { _ => () }
///     match Timeout::new_pin(Box::pin(calc_2()), 500).await { _ => () }
///     match Timeout::new(calc_1(), 300).await { _ => () }
///     match calc_2().timeout(500).await { Ok(_) => {}, Err(_) => {} }
///  }
/// ```
pub struct Timeout<'bemly_, Content> {
    future: Pin<Box<dyn Future<Output = Content> + 'bemly_>>,
    deadline: u64,
}

impl<'bemly_, Content> Timeout<'bemly_, Content> {
    #[inline]
    pub fn new(future: impl Future<Output = Content> + 'bemly_, ticks: u64) -> Self {
        Self { future: Box::pin(future), deadline: tick().saturating_add(ticks) }
    }

    #[inline(always)]
    pub fn new_pin(future: Pin<Box<dyn Future<Output = Content> + 'bemly_>>, ticks: u64) -> Self {
        Self { future, deadline: tick().saturating_add(ticks) }
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
pub trait _Timeout<'bemly_>: Future + Sized {
    fn timeout(self, duration_ticks: u64) -> Timeout<'bemly_, Self::Output> where Self: 'bemly_ {
        Timeout::new(self, duration_ticks)
    }
}
impl<F: Future + Sized> _Timeout<'_> for F {}

///
/// Usage:
/// ```rust,no_run
/// use uefi_async::nano_alloc::time::{WaitTimer, _WaitTimer};
/// async fn blink_led_task(cpu_freq: u64) {
///     loop {
///         set_led(true);
///         2.year().await;
///         5.day().await;
///         1.mins().await;
///         80.ps().await;
///         1.us().await;
///         500.ms().await;
///         20.fps().await;
///         set_led(false);
///         WaitTimer::from_ms(500).await;
///     }
/// }
/// ```
///
#[derive(Debug)]
pub struct WaitTimer(u64);

impl WaitTimer {
    /// 核心构造函数：传入目标截止时间（tick）
    #[inline(always)]
    pub fn until(deadline: u64) -> Self { Self(deadline) }

    /// 基于当前 tick 增加偏移量
    #[inline(always)]
    pub fn after(ticks_to_wait: u64) -> Self { Self(tick().wrapping_add(ticks_to_wait)) }

    /// 使用 wrapping_sub 处理计数器回绕问题
    /// 如果当前时间减去截止时间的结果 > 一半的 u64 范围，说明截止时间还没到
    #[inline(always)]
    pub fn is_expired(&self) -> bool { tick() >= self.0 }

    #[inline(always)]
    pub fn from_year(y: u64) -> Self { Self::after(y.saturating_mul(FREQ.hz().saturating_mul(31536000))) }

    #[inline(always)]
    pub fn from_month(m: u64) -> Self { Self::after(m.saturating_mul(FREQ.hz() * 2592000)) }

    #[inline(always)]
    pub fn from_week(w: u64) -> Self { Self::after(w.saturating_mul(FREQ.hz() * 604800)) }

    #[inline(always)]
    pub fn from_day(d: u64) -> Self { Self::after(d.saturating_mul(FREQ.hz() * 86400)) }

    #[inline(always)]
    pub fn from_hour(h: u64) -> Self { Self::after(h.saturating_mul(FREQ.hz() * 3600)) }

    #[inline(always)]
    pub fn from_min(min: u64) -> Self { Self::after(min.saturating_mul(FREQ.hz() * 60)) }

    #[inline(always)]
    pub fn from_s(hz: u64) -> Self { Self::after(hz.saturating_mul(FREQ.hz())) }

    #[inline(always)]
    pub fn from_ms(ms: u64) -> Self { Self::after(ms.saturating_mul(FREQ.ms())) }

    #[inline(always)]
    pub fn from_us(us: u64) -> Self { Self::after(us.saturating_mul(FREQ.us())) }

    #[inline(always)]
    pub fn from_ns(ns: u64) -> Self { Self::after(ns.saturating_mul(FREQ.ns())) }

    #[inline(always)]
    pub fn from_ps(ps: u64) -> Self { Self::after(ps.saturating_mul(FREQ.ps())) }

    #[inline(always)]
    pub fn from_fps(fps: u64) -> Self { Self::after(FREQ.hz() / fps.max(1)) }
}

impl Future for WaitTimer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.is_expired() { Poll::Ready(()) } else { Poll::Pending }
    }
}

pub trait _WaitTimer {
    fn year(self) -> WaitTimer;
    fn month(self) -> WaitTimer;
    fn week(self) -> WaitTimer;
    fn day(self) -> WaitTimer;
    fn hour(self) -> WaitTimer;
    fn mins(self) -> WaitTimer;
    fn s(self) -> WaitTimer;
    fn hz(self) -> WaitTimer;
    fn ms(self) -> WaitTimer;
    fn us(self) -> WaitTimer;
    fn ns(self) -> WaitTimer;
    fn ps(self) -> WaitTimer;
    fn fps(self) -> WaitTimer;
}

impl _WaitTimer for u64 {
    #[inline(always)]
    fn year(self) -> WaitTimer { WaitTimer::from_year(self) }

    #[inline(always)]
    fn month(self) -> WaitTimer { WaitTimer::from_month(self) }

    #[inline(always)]
    fn week(self) -> WaitTimer { WaitTimer::from_week(self) }

    #[inline(always)]
    fn day(self) -> WaitTimer { WaitTimer::from_day(self) }

    #[inline(always)]
    fn hour(self) -> WaitTimer { WaitTimer::from_hour(self) }

    #[inline(always)]
    fn mins(self) -> WaitTimer { WaitTimer::from_min(self) }

    #[inline(always)]
    fn s(self) -> WaitTimer { WaitTimer::from_s(self) }

    #[inline(always)]
    fn hz(self) -> WaitTimer { WaitTimer::from_s(self) }

    #[inline(always)]
    fn ms(self) -> WaitTimer { WaitTimer::from_ms(self) }

    #[inline(always)]
    fn us(self) -> WaitTimer { WaitTimer::from_us(self) }

    #[inline(always)]
    fn ns(self) -> WaitTimer { WaitTimer::from_ns(self) }

    #[inline(always)]
    fn ps(self) -> WaitTimer { WaitTimer::from_ps(self) }

    #[inline(always)]
    fn fps(self) -> WaitTimer { WaitTimer::from_fps(self) }
}