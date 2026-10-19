//! Dynamic task executor based on lock-free work-stealing queues.
//!
//! Unlike `nano_alloc` (borrowed nodes in one intrusive list),
//! tasks here are **owned** (`Box`) and live in [`st3`] LIFO workers, so they
//! can be spawned at runtime, cancelled by id, retuned live, and stolen
//! across cores via [`Stealer`] handles.
//!
//! Each core normally owns one [`Executor`]; executors exchange [`Stealer`]
//! handles once at startup (see `add_stealer`), which is the "priority
//! steering / core pinning" story: spawn latency-sensitive tasks on the fast
//! core's executor, background work anywhere, and let stealing balance the rest.
//!
//! Scheduling is frequency-based like everywhere else in this crate: every
//! task carries a Hz rate converted with the global [`FREQ`](crate::FREQ)
//! calibration, and cooperative primitives (`Yield`, `Skip`, `Pacer`,
//! [`WaitTimer`](crate::nano_alloc::time::WaitTimer)) work unchanged because
//! they are plain futures.

pub mod executor;
pub mod task;

pub use executor::Executor;
pub use task::{TaskId, TaskNode};
