//! Work-stealing dynamic executor.
//!
//! Each core owns one [`Executor`]; tasks are [`Box`]-owned nodes in an
//! [`st3`] LIFO worker queue, so they can be spawned at runtime and stolen
//! across cores via [`Stealer`] handles registered with [`add_stealer`](Executor::add_stealer).
//!
//! Control plane (spawn / cancel / retune) goes through one global registry
//! keyed by [`TaskId`]; the registry only holds plain data (no raw pointers),
//! so cancelling a task that already completed -- or was stolen elsewhere --
//! is always safe (returns `false`, never touches freed memory).
//!
//! Scheduling mirrors the rest of the crate: frequency-based with the global
//! tick calibration, cooperative yield/skip/pacer/wait-timer futures work
//! unchanged because they are plain futures.

use crate::dynamic::task::{TaskId, TaskNode};
use crate::{init_clock_freq, tick};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};
use spin::Mutex;
use st3::lifo::{Stealer, Worker};

/// Queue item: a heap-owned task node.
pub type Task<'a> = Box<TaskNode<'a>>;

/// Default worker capacity (power of two; see [`st3`]).
const DEFAULT_CAPACITY: usize = 64;

/// How many tasks to steal from one sibling per step.
const STEAL_BATCH: usize = 4;

/// Registry slot: plain data only, so any core may inspect it safely.
struct Slot {
    id: TaskId,
    alive: bool,
    cancelled: bool,
    freq_hz: u64,
}

/// Global id counter and registry shared by all dynamic executors.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REGISTRY: Mutex<Vec<Slot>> = Mutex::new(Vec::new());

/// Looks a slot up by id; callers hold no lock across polls.
fn with_slot<R>(id: TaskId, f: impl FnOnce(Option<&mut Slot>) -> R) -> R {
    let mut reg = REGISTRY.lock();
    let slot = reg.iter_mut().find(|s| s.alive && s.id == id);
    f(slot)
}

/// A per-core dynamic executor.
///
/// Owns one [`st3`] worker queue; sibling [`Stealer`]s registered via
/// [`add_stealer`](Executor::add_stealer) supply cross-core balancing.
/// The executor itself is `!Sync` (one owner core), the stealers are shared.
pub struct Executor<'a> {
    worker: Worker<Task<'a>>,
    stealers: Vec<Stealer<Task<'a>>>,
    /// Hardware ticks per second (snapshot of [`FREQ`]).
    pub freq: u64,
}

impl<'a> Executor<'a> {
    /// Creates an executor and calibrates the tick frequency (~50ms stall).
    pub fn new() -> Self {
        let freq = init_clock_freq();
        Self {
            worker: Worker::new(DEFAULT_CAPACITY),
            stealers: Vec::new(),
            freq,
        }
    }

    /// Registers a sibling's [`Stealer`] for cross-core balancing.
    pub fn add_stealer(&mut self, stealer: Stealer<Task<'a>>) {
        self.stealers.push(stealer);
    }

    /// Exposes our own [`Stealer`] so siblings can steal from us
    /// (core pinning + balancing story: spawn latency-sensitive work on the
    /// fast core's executor, share stealers everywhere else).
    pub fn stealer(&self) -> Stealer<Task<'a>> {
        self.worker.stealer()
    }

    /// Spawns a task, returning its [`TaskId`] (or `None` if the queue is full).
    ///
    /// The task runs at `freq` Hz (`0` = every step). Ownership moves into
    /// the local worker queue; any core holding our [`Stealer`] may steal it.
    pub fn spawn(
        &self,
        future: Pin<Box<dyn Future<Output = ()> + Send + 'a>>,
        freq: u64,
    ) -> Option<TaskId> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        // Retire a dead slot when possible so the registry stays bounded.
        let mut reg = REGISTRY.lock();
        if let Some(slot) = reg.iter_mut().find(|s| !s.alive) {
            *slot = Slot {
                id,
                alive: true,
                cancelled: false,
                freq_hz: freq,
            };
        } else {
            reg.push(Slot {
                id,
                alive: true,
                cancelled: false,
                freq_hz: freq,
            });
        }
        drop(reg);
        let mut node = Box::new(TaskNode::new(future, freq));
        node.id = id;
        node.interval = TaskNode::ticks(freq, self.freq);
        if self.worker.push(node).is_err() {
            // Queue full: retire the slot, drop the task, report failure.
            let mut reg = REGISTRY.lock();
            if let Some(slot) = reg.iter_mut().find(|s| s.id == id) {
                slot.alive = false;
            }
            return None;
        }
        Some(id)
    }

    /// Cooperatively cancels a task (takes effect at its next poll boundary).
    ///
    /// Returns `false` when the id is unknown or already finished -- never
    /// touches freed memory (the registry only holds plain data).
    pub fn cancel(&self, id: TaskId) -> bool {
        with_slot(id, |slot| match slot {
            Some(s) => {
                s.cancelled = true;
                true
            }
            None => false,
        })
    }

    /// Retunes a live task's rate in Hz (`0` = every step).
    pub fn set_freq(&self, id: TaskId, freq_hz: u64) -> bool {
        with_slot(id, |slot| match slot {
            Some(s) => {
                s.freq_hz = freq_hz;
                true
            }
            None => false,
        })
    }

    /// Returns `true` while the id names a live (spawned, unfinished) task.
    pub fn is_alive(&self, id: TaskId) -> bool {
        with_slot(id, |slot| slot.is_some())
    }

    /// Initializes a no-op [`Context`] required for polling futures.
    pub fn init_step() -> Context<'a> {
        Context::from_waker(Waker::noop())
    }

    /// Runs the event loop forever on this core.
    pub fn run_forever(&self) {
        let mut cx = Self::init_step();
        loop {
            self.run_step(tick(), &mut cx);
        }
    }

    /// Performs a single scheduling pass: steal one batch per sibling, then
    /// drain the local queue once (bounded by capacity so a hot queue can't
    /// starve the step).
    pub fn run_step(&self, time: u64, cx: &mut Context) {
        // 1. Cross-core balancing: steal a small batch from each sibling.
        for stealer in &self.stealers {
            let _ = stealer.steal(&self.worker, |_| STEAL_BATCH);
        }
        // 2. Drain the local queue (bounded: leftovers ride to the next step).
        let mut budget = self.worker.capacity();
        while budget > 0 {
            let Some(mut task) = self.worker.pop() else {
                break;
            };
            budget -= 1;
            if !Self::poll_one(&mut task, time, cx, self.freq) {
                // Not finished: requeue (a concurrent steal may have filled
                // the queue meanwhile -- then it rides the next step).
                let _ = self.worker.push(task);
            }
        }
    }

    /// Polls one task; returns `false` when it must be requeued.
    fn poll_one(task: &mut Task<'a>, time: u64, cx: &mut Context, freq_base: u64) -> bool {
        // Sync with the registry exactly once per poll (short critical section,
        // never held across `poll`).
        let (cancelled, freq_hz) = {
            let mut reg = REGISTRY.lock();
            match reg.iter_mut().find(|s| s.alive && s.id == task.id) {
                None => return true, // Slot gone: treat as finished, drop the box.
                Some(slot) => {
                    let cancelled = slot.cancelled;
                    let freq_hz = slot.freq_hz;
                    if cancelled {
                        slot.alive = false;
                    }
                    (cancelled, freq_hz)
                }
            }
        };
        if cancelled {
            return true; // Drop the box: cooperative cancel complete.
        }
        if time < task.next_run_time {
            return false; // Not due yet: requeue untouched.
        }
        task.interval = TaskNode::ticks(freq_hz, freq_base);
        match task.future.as_mut().poll(cx) {
            Poll::Ready(()) => {
                // Retire the slot, then drop the box: memory reclaimed.
                let mut reg = REGISTRY.lock();
                if let Some(slot) = reg.iter_mut().find(|s| s.id == task.id) {
                    slot.alive = false;
                }
                true
            }
            Poll::Pending => {
                task.next_run_time = time.wrapping_add(task.interval);
                false
            }
        }
    }
}
