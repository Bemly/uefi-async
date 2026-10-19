/// the TSC-based tick counter and frequency calibration.
pub mod tick;

/// Execution pacing and task suspension tools for UEFI asynchronous environments.
pub mod step;
pub mod signal;

/// Executor-agnostic control combinators (join/select) usable from every line.
pub mod control;

/// Executor-agnostic timing (timeouts, human-readable timers), no heap.
pub mod time;

/// Cooperative async mutex (park instead of spin) for cross-core exclusion.
pub mod mutex;

pub use tick::*;
pub use step::*;

pub use tick::*;
pub use step::*;