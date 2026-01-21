use crate::{init_clock_freq, tick};
use crate::nano_alloc::TaskNode;
use core::task::{Context, Poll, Waker};

/// A simple round-robin executor that manages a linked-list of tasks.
///
/// The executor polls tasks based on their scheduled `next_run_time`
/// and supports dynamic task addition.
pub struct Executor<'curr, 'next> {
    /// The head of the task linked-list.
    head: Option<&'next mut TaskNode<'curr, 'next>>,
    /// The hardware clock frequency, used to normalize task intervals.
    pub freq: u64,
}

impl<'curr, 'next> Executor<'curr, 'next> {
    /// Initializes a new executor and calculates the hardware frequency.
    #[inline]
    pub fn new() -> Self { Self{ head: None, freq: init_clock_freq() } }

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
    #[inline(always)]
    pub fn run_forever(&mut self) {
        let mut cx = Self::init_step();
        loop { self.run_step(tick(), &mut cx) }
    }

    /// Initializes a no-op [`Context`] required for polling futures.
    #[inline(always)]
    pub fn init_step() -> Context<'curr> { Context::from_waker(&Waker::noop()) }

    /// Performs a single pass over all registered tasks.
    ///
    /// For each node:
    /// 1. Checks if the current `time` has reached `next_run_time`.
    /// 2. Polls the future.
    /// 3. If `Ready`, the node is removed from the list.
    /// 4. If `Pending`, the `next_run_time` is updated by the node's interval.
    #[inline(always)]
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