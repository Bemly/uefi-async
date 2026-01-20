extern crate alloc;
use crate::util::{calc_freq_blocking, tick};
use alloc::boxed::Box;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

/// Procedural macro logic to initialize and register task nodes to executors.
///
/// # Expansion
/// For every task defined, this macro generates:
/// 1. A unique variable name (e.g., `__node_0_1`).
/// 2. A `TaskNode` initialization with a pinned function and frequency.
/// 3. A registration call adding the node to the specified executor.
///
/// # Example Input
/// ```rust, no_run
/// executor => {
///     100 -> task_one(),
///     my_config.interval -> || { do_work() }
/// }
/// ```
pub use uefi_async_macros::nano as add;

/// A node in a linked-list representing an asynchronous task.
///
/// # Lifetimes
/// * `'curr`: The lifetime of the future's internal data.
/// * `'next`: The lifetime of the reference to the next node in the list.
///
/// tip: `'curr` is longer than `'next`.
pub struct TaskNode<'curr, 'next> {
    /// The pinned asynchronous computation to be executed.
    future: Pin<Box<dyn Future<Output = ()> + 'curr>>,
    /// The execution interval, converted into hardware ticks.
    interval: u64,
    /// The timestamp (in ticks) when this task should run again.
    next_run_time: u64,
    /// Link to the next task in the executor's chain.
    next: Option<&'next mut TaskNode<'curr, 'next>>,
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

/// A simple round-robin executor that manages a linked-list of tasks.
///
/// The executor polls tasks based on their scheduled `next_run_time`
/// and supports dynamic task addition.
pub struct Executor<'curr, 'next> {
    /// The head of the task linked-list.
    head: Option<&'next mut TaskNode<'curr, 'next>>,
    /// The hardware clock frequency, used to normalize task intervals.
    freq: u64,
}

impl<'curr, 'next> Executor<'curr, 'next> {
    /// Initializes a new executor and calculates the hardware frequency.
    #[inline]
    pub fn new() -> Self {
        Self{ head: None, freq: calc_freq_blocking() }
    }

    /// Adds a task node to the executor.
    ///
    /// This method converts the node's frequency into a tick interval
    /// and pushes the node to the front of the linked-list.
    #[inline]
    pub fn add(&mut self, node: &'next mut TaskNode<'curr, 'next>) -> &mut Self {
        // convert frequency (Hz) to interval (ticks)
        node.interval = self.freq.checked_div(node.interval).unwrap_or(0);
        // push to the front of the list
        node.next = self.head.take();
        self.head = Some(node);
        self
    }

    /// Enters an infinite loop, continuously ticking the executor.
    #[inline]
    pub fn run_forever(&mut self) {
        let mut cx = Self::init_step();
        loop { self.run_step(tick(), &mut cx) }
    }

    /// Initializes a no-op [`Context`] required for polling futures.
    #[inline(always)]
    pub fn init_step() -> Context<'curr> {
        Context::from_waker(&Waker::noop())
    }

    /// Performs a single pass over all registered tasks.
    ///
    /// For each node:
    /// 1. Checks if the current `time` has reached `next_run_time`.
    /// 2. Polls the future.
    /// 3. If `Ready`, the node is removed from the list.
    /// 4. If `Pending`, the `next_run_time` is updated by the node's interval.
    #[inline]
    pub fn run_step(&mut self, time: u64, cx: &mut Context) {
        let mut cursor = &mut self.head;

        // traverse the task list and poll each async task
        while let Some(node) = cursor {
            if time >= node.next_run_time {
                match node.future.as_mut().poll(cx) {
                    // remove it from the list by bridging the pointer when async finished.
                    // it might miss the next one, but it's not a problem for polling.
                    Poll::Ready(_) => *cursor = node.next.take(),
                    // schedule next execution time
                    Poll::Pending => node.next_run_time = time + node.interval,
                }
            }

            // Re-borrow: not time to run yet, skip to next node
            cursor = match { cursor } {
                Some(node) => &mut node.next,
                None => break,
            };
        }
    }
}