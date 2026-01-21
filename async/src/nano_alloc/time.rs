use crate::{tick, FREQ};
use alloc::boxed::Box;
use core::pin::Pin;
use core::task::{Context, Poll};

/// A wrapper that adds a time limit to a `Future`.
///
/// If the internal future does not complete before the deadline (in hardware ticks),
/// it returns `Err(())`. Otherwise, it returns `Ok(Output)`.
///
/// # Examples
///
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
    /// Creates a new `Timeout` by pinning the provided future to the heap.
    ///
    /// `ticks` is the relative duration from the current time.
    #[inline]
    pub fn new(future: impl Future<Output = Content> + 'bemly_, ticks: u64) -> Self {
        Self { future: Box::pin(future), deadline: tick().saturating_add(ticks) }
    }

    /// Creates a new `Timeout` from an already pinned future.
    ///
    /// Useful when the future is already boxed or has a complex lifetime.
    #[inline(always)]
    pub fn new_pin(future: Pin<Box<dyn Future<Output = Content> + 'bemly_>>, ticks: u64) -> Self {
        Self { future, deadline: tick().saturating_add(ticks) }
    }
}

impl<'bemly_, Content> Future for Timeout<'bemly_, Content> {
    /// Returns `Ok(T)` if finished in time, or `Err(())` if the deadline is exceeded.
    type Output = Result<Content, ()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // Check if the current hardware tick has passed the deadline
        if tick() >= self.deadline { return Poll::Ready(Err(())) }

        // Poll the inner pinned future
        match self.future.as_mut().poll(cx) {
            Poll::Ready(val) => Poll::Ready(Ok(val)),
            Poll::Pending => Poll::Pending,
        }
    }
}
/// An extension trait to provide the `.timeout()` method for all `Future` types.
pub trait _Timeout<'bemly_>: Future + Sized {
    /// Wraps the future in a `Timeout` with the specified duration in hardware ticks.
    fn timeout(self, duration_ticks: u64) -> Timeout<'bemly_, Self::Output> where Self: 'bemly_ {
        Timeout::new(self, duration_ticks)
    }
}
impl<F: Future + Sized> _Timeout<'_> for F {}

/// A high-precision asynchronous timer based on hardware ticks.
///
/// `WaitTimer` leverages the CPU's Time Stamp Counter (TSC) to provide non-blocking
/// delays ranging from picoseconds to years. It implements the [`Future`] trait,
/// allowing the executor to suspend tasks until the specified hardware deadline is reached.
///
/// # Precision and Accuracy
/// The timer relies on the calibrated frequency stored in `FREQ`. Accuracy is
/// determined by the sampling quality during `init_clock_freq()`.
///
/// # Examples
///
/// Using the builder-style methods:
/// ```rust,no_run
/// async fn delay_example() {
///     // Create a timer for 500 milliseconds
///     WaitTimer::from_ms(500).await;
/// }
/// ```
///
/// Using the [`_WaitTimer`] extension trait for a more expressive DSL:
/// ```rust,no_run
/// use uefi_async::nano_alloc::time::{WaitTimer, _WaitTimer};
///
/// async fn blink_led_task() {
///     loop {
///         // Highly readable duration syntax
///         1.s().await;        // Wait 1 second
///         500.ms().await;     // Wait 500 milliseconds
///         20.fps().await;     // Wait for 1 frame duration at 20 FPS (50ms)
///
///         // Precise sub-microsecond timing
///         10.us().await;      // 10 microseconds
///         80.ps().await;      // 80 picoseconds (Note: Dependent on CPU Ghz)
///
///         // Long-term scheduling
///         2.hour().await;
///         1.day().await;
///     }
/// }
/// ```
#[derive(Debug)]
pub struct WaitTimer(u64);

impl WaitTimer {
    /// Constructs a timer that expires at an absolute hardware tick count.
    #[inline(always)]
    pub fn until(deadline: u64) -> Self { Self(deadline) }

    /// Constructs a timer that expires after a relative number of ticks from now.
    #[inline(always)]
    pub fn after(ticks_to_wait: u64) -> Self { Self(tick().wrapping_add(ticks_to_wait)) }

    /// Checks if the current hardware tick has reached or exceeded the deadline.
    ///
    /// This uses a direct comparison, assuming the timer is polled frequently
    /// enough to handle 64-bit tick overflows (which take centuries on modern CPUs).
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

/// Extension trait providing a fluent API for creating [`WaitTimer`] instances
/// directly from unsigned 64-bit integers.
///
/// This trait is exclusively implemented for `u64` to ensure stable type inference
/// when using numeric literals (e.g., `500.ms().await`).
pub trait _WaitTimer: Into<u64> + Sized {
    /// Creates a timer for the specified number of years.
    #[inline(always)]
    fn year(self) -> WaitTimer { WaitTimer::from_year(self.into()) }

    /// Creates a timer for the specified number of months.
    #[inline(always)]
    fn month(self) -> WaitTimer { WaitTimer::from_month(self.into()) }

    /// Creates a timer for the specified number of weeks.
    #[inline(always)]
    fn week(self) -> WaitTimer { WaitTimer::from_week(self.into()) }

    /// Creates a timer for the specified number of days.
    #[inline(always)]
    fn day(self) -> WaitTimer { WaitTimer::from_day(self.into()) }

    /// Creates a timer for the specified number of hours.
    #[inline(always)]
    fn hour(self) -> WaitTimer { WaitTimer::from_hour(self.into()) }

    /// Creates a timer for the specified number of minutes.
    #[inline(always)]
    fn mins(self) -> WaitTimer { WaitTimer::from_min(self.into()) }

    /// Creates a timer for the specified number of seconds.
    #[inline(always)]
    fn s(self) -> WaitTimer { WaitTimer::from_s(self.into()) }

    /// Creates a timer based on Frequency (Hz).
    /// Represented as the period: 1 second / frequency.
    #[inline(always)]
    fn hz(self) -> WaitTimer { WaitTimer::from_s(self.into()) }

    /// Creates a timer for the specified number of milliseconds (10^-3 s).
    #[inline(always)]
    fn ms(self) -> WaitTimer { WaitTimer::from_ms(self.into()) }

    /// Creates a timer for the specified number of microseconds (10^-6 s).
    #[inline(always)]
    fn us(self) -> WaitTimer { WaitTimer::from_us(self.into()) }

    /// Creates a timer for the specified number of nanoseconds (10^-9 s).
    #[inline(always)]
    fn ns(self) -> WaitTimer { WaitTimer::from_ns(self.into()) }

    /// Creates a timer for the specified number of picoseconds (10^-12 s).
    #[inline(always)]
    fn ps(self) -> WaitTimer { WaitTimer::from_ps(self.into()) }

    /// Creates a timer based on Frames Per Second (FPS).
    /// Used for synchronizing game loops or rendering intervals.
    #[inline(always)]
    fn fps(self) -> WaitTimer { WaitTimer::from_fps(self.into()) }
}
/// Implement only for u64 to provide a concrete base for numeric literals.
/// Only one primitive type is allowed; otherwise, the rustc_infer cannot deduce it.
impl _WaitTimer for u64 {}