//! Owned task nodes for the [`dynamic`](crate::dynamic) executor.
//!
//! A node owns its future (`Box`), so the executor can move it between
//! per-core queues (stealing) and drop it when it completes or is cancelled.
//! Cross-core control (cancel / retune) goes through the id-keyed registry in
//! [`executor`](crate::dynamic::executor); the node itself only carries data.

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Opaque task identifier returned by [`Executor::spawn`](crate::dynamic::Executor::spawn).
pub type TaskId = u64;

/// A heap-owned asynchronous task.
///
/// The future must be `Send` because stealing may move the node to another
/// core's queue. `interval`/`next_run_time` are in hardware ticks (see
/// [`tick`](crate::tick) and [`FREQ`](crate::FREQ)).
pub struct TaskNode<'a> {
    /// The pinned asynchronous computation.
    pub future: Pin<Box<dyn Future<Output = ()> + Send + 'a>>,
    /// Ticks between runs (`0` = every step).
    pub interval: u64,
    /// Tick timestamp when this task should run next.
    pub next_run_time: u64,
    /// Registry id (see [`Executor::spawn`](crate::dynamic::Executor::spawn)).
    pub id: TaskId,
    /// Cooperative cancel flag (set via registry, observed by the poller).
    pub cancelled: AtomicBool,
    /// Live-tunable rate in Hz (see [`Executor::set_freq`]).
    pub freq: AtomicU64,
}

impl<'a> TaskNode<'a> {
    /// Creates a node; the executor assigns `interval`/`id` on spawn.
    pub fn new(future: Pin<Box<dyn Future<Output = ()> + Send + 'a>>, freq: u64) -> Self {
        Self {
            future,
            interval: 0,
            next_run_time: 0,
            id: 0,
            cancelled: AtomicBool::new(false),
            freq: AtomicU64::new(freq),
        }
    }

    /// Converts a rate (Hz) to a tick interval with the global calibration.
    pub fn ticks(freq_hz: u64, freq_base: u64) -> u64 {
        if freq_hz == 0 {
            0
        } else {
            freq_base.checked_div(freq_hz).unwrap_or(0)
        }
    }

    /// Marks this node cancelled (observed at the next poll boundary).
    ///
    /// Prefer [`Executor::cancel`] so the registry slot is retired too.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Retunes the rate in Hz (takes effect at the next poll boundary).
    ///
    /// Prefer [`Executor::set_freq`] so registry and node stay consistent.
    pub fn set_freq(&self, freq_hz: u64) {
        self.freq.store(freq_hz, Ordering::Release);
    }
}
