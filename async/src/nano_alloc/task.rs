use alloc::boxed::Box;
use core::pin::Pin;

/// A node in a linked-list representing an asynchronous task.
///
/// # Lifetimes
/// * `'curr`: The lifetime of the future's internal data.
/// * `'next`: The lifetime of the reference to the next node in the list.
///
/// tip: `'curr` is longer than `'next`.
pub struct TaskNode<'curr, 'next> {
    /// The pinned asynchronous computation to be executed.
    pub future: Pin<Box<dyn Future<Output = ()> + 'curr>>,
    /// The execution interval, converted into hardware ticks.
    pub interval: u64,
    /// The timestamp (in ticks) when this task should run again.
    pub next_run_time: u64,
    /// Link to the next task in the executor's chain.
    pub next: Option<&'next mut TaskNode<'curr, 'next>>,
}
impl<'curr, 'next> TaskNode<'curr, 'next> {
    /// Creates a new `TaskNode` with a given future and frequency.
    ///
    /// Note: `freq` is initially the user-requested frequency and is
    /// recalculated into an interval when added to an [`Executor`].
    #[inline]
    pub fn new(future: Pin<Box<dyn Future<Output = ()> + 'curr>>, freq: u64) -> Self {
        Self { future, interval: freq, next_run_time: 0, next: None }
    }
}