//! Execution pacing and task suspension tools for UEFI asynchronous environments.
//!
//! This module provides three primary ways to control task execution:
//! 1. **Yield**: Cooperatively giving up CPU time to the scheduler.
//! 2. **Skip**: Jumping over a fixed number of scheduling cycles.
//! 3. **Pacer**: A simple pacing mechanism that can be used to throttle task execution.
//!
use core::pin::Pin;
use core::task::{Context, Poll};

/// A unit structure that implements [`IntoFuture`] for a clean `.await` syntax.
///
/// Using `YIELD.await` is the preferred way to cooperatively yield control back
/// to the [`Executor`]. It ensures other tasks (like input handling) can run.
///
/// # Example
/// ```rust
/// async fn heavy_computation() {
///     for i in 0..1000 {
///         do_work(i);
///         // Yield every iteration to prevent system hang
///         YIELD.await;
///     }
/// }
/// ```
pub const YIELD: _Yield = _Yield(false);

/// A unit structure that implements [`IntoFuture`] for a clean `.await` syntax.
///
/// Using `yield_now().await` is the preferred way to cooperatively yield control back
/// to the [`Executor`]. It ensures other tasks (like input handling) can run.
///
/// # Example
/// ```rust
/// async fn heavy_computation() {
///     for i in 0..1000 {
///         do_work(i);
///         // Yield every iteration to prevent system hang
///         yield_now().await;
///     }
/// }
/// ```
#[deprecated(since = "0.2.5", note = "Use `Yield.await` instead")]
pub async fn yield_now() { _Yield(false).await }

/// A unit structure that implements [`IntoFuture`] for a clean `.await` syntax.
///
/// Using `Yield.await` is the preferred way to cooperatively yield control back
/// to the [`Executor`]. It ensures other tasks (like input handling) can run.
///
/// # Example
/// ```rust
/// async fn heavy_computation() {
///     for i in 0..1000 {
///         do_work(i);
///         // Yield every iteration to prevent system hang
///         Yield.await;
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Yield;
impl IntoFuture for Yield {
    type Output = ();
    type IntoFuture = _Yield;
    fn into_future(self) -> Self::IntoFuture { _Yield(false) }
}

/// The actual future returned by [`YIELD`] or [`Yield`].
#[derive(Debug)]
pub struct _Yield(bool);
impl Future for _Yield {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.0 { Poll::Ready(()) } else { self.0 = true; Poll::Pending }
    }
}

/// A future that skips a fixed number of executor polling cycles.
///
/// Useful for low-priority tasks that do not need to check their state
/// on every single tick of the executor.
///
/// # Example
/// ```rust
/// async fn background_task() {
///     loop {
///         // Only run once every 10 scheduler passes
///         Skip(10).await;
///         check_background_logs();
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Skip(pub usize);
impl Future for Skip {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        if self.0 == 0 { Poll::Ready(()) } else { self.0 -= 1; Poll::Pending }
    }
}

#[derive(Debug)]
pub struct Pacer {
    count: usize,
    countdown: usize,
}
impl Pacer {
    /// Creates a new `Pacer` with a specified initial count.
    ///
    /// # Arguments
    /// * `count` - The number of cycles to wait before the first trigger.
    pub fn new(count: usize) -> Self { Pacer { count, countdown: count } }

    /// Updates the internal reload count and returns a mutable reference.
    ///
    /// Useful for dynamically adjusting task priority or execution frequency.
    pub fn count_update(&mut self, count: usize) -> &mut Self { self.count = count; self }

    /// The core generalized pacing interface.
    ///
    /// Decrements the counter on each call. When the counter reaches zero,
    /// it executes an asynchronous wait (Yield or Skip).
    ///
    /// # Arguments
    /// * `yield_count` - If `0`, performs a single `Yield`. If `> 0`, performs a `Skip(yield_count)`.
    /// * `repeat` - If `true`, reloads `countdown` from `count` after triggering.
    #[inline]
    pub async fn step(&mut self, yield_count: usize, repeat: bool) {
        if self.countdown > 0 { self.countdown -= 1 }
        else {
            if repeat { self.countdown = self.count }
            if yield_count == 0 { Yield.await } else { Skip(yield_count).await }
        }
    }

    /// Throttles execution until the countdown reaches zero, then yields on every subsequent call.
    ///
    /// Useful for "one-shot" delays or preventing a task from starting too early.
    ///
    /// # Example
    /// ```rust
    /// let mut pacer = Pacer::new(100);
    /// loop {
    ///     pacer.throttle().await; // Waits 100 ticks once, then yields every loop iteration.
    ///     do_work();
    /// }
    /// ```
    #[inline(always)]
    pub async fn throttle(&mut self) { self.step(0, false).await }

    /// Periodically yields control to the executor every `count` calls.
    ///
    /// This is the standard way to implement cooperative multitasking for background tasks.
    ///
    /// # Example
    /// ```rust
    /// let mut pacer = Pacer::new(60);
    /// loop {
    ///     process_physics();
    ///     pacer.repeat().await; // Yields to other tasks once every 60 iterations.
    /// }
    /// ```
    #[inline(always)]
    pub async fn repeat(&mut self) { self.step(0, true).await }

    /// Periodically yields control for multiple executor steps (`i`) every `count` calls.
    ///
    /// Use this for low-priority tasks that should give significant breathing room to
    /// higher-priority tasks (like UI or Input) after a burst of work.
    ///
    /// # Example
    /// ```rust
    /// let mut pacer = Pacer::new(10);
    /// loop {
    ///     load_resource_chunk();
    ///     pacer.burst(5).await; // Every 10 chunks, skip 5 scheduling passes.
    /// }
    /// ```
    #[inline(always)]
    pub async fn burst(&mut self, i: usize) { self.step(i, true).await }
}