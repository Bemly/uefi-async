/// A node in a linked-list representing an asynchronous task.
pub mod task;
pub use task::TaskNode;

/// A simple round-robin executor that manages a linked-list of tasks.
pub mod executor;
pub use executor::Executor;
pub mod time;
pub mod uefi;
pub mod channel;
pub mod control;

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