//! uefi-async
//! ================================
//! A lightweight, zero-cost asynchronous executor designed specifically for UEFI environments or bare-metal Rust.
//! It provides a simple task scheduler based on a intrusive linked-list and procedural macro to simplify task registration.
//! 
//! --------------------------------
//! Work in Progress
//! --------------------------------
//! currently only `nano_alloc` feature is supported.
//! 
//! --------------------------------
//! Features
//! --------------------------------
//! * **No-Std Compatible**: Designed for environments without a standard library (requires `alloc`).
//! * **Intrusive Linked-List**: No additional collection overhead for managing tasks.
//! * **Frequency-Based Scheduling**: Define tasks to run at specific frequencies (Hz), automatically converted to ticks.
//! * **Macro-Driven Syntax**: A clean, declarative DSL to assign tasks to executors.
//! * **Tiny Control Primitives**: Comprehensive support for timeouts, joins, and hardware-precise timing.
//! * **Safe Signaling:** Cross-core event notification with atomic state transitions.
//! * **Multicore-Ready**: Thread-safe primitives for cross-core signaling and data synchronization.
//! 
//! --------------------------------
//! Tiny Async Control Flow
//! --------------------------------
//! 
//! ### 1. High-Precision Timing
//! 
//! Support for human-readable time units and hardware-aligned synchronization.
//! 
//! ```rust
//! async fn timer() {
//!     WaitTimer::from_ms(500).await; // Explicit timer
//!     2.year().await;                // Natural language units
//!     1.mins().await;
//!     80.ps().await;                 // Picosecond precision (CPU frequency dependent)
//!     20.fps().await;                // Framerate-locked synchronization
//!     Yield.await;                   // Voluntary cooperative yield
//!     Skip(2).await;                 // Skip N executor cycles
//! }
//! ```
//! ### 2. Task Completion & Concurrency
//! 
//! Powerful macros and traits to combine multiple futures.
//! 
//! * **`join!`**: Runs multiple tasks concurrently; returns `()`.
//! * **`try_join!`**: Short-circuits and returns `Err` if any task fails.
//! * **`join_all!`**: Collects results from all tasks into a flattened tuple.
//! * **Trait-based Joins**: Call `.join().await` or `.try_join().await` directly on Tuples, Arrays, or Vectors.
//! 
//! ```rust
//! async fn calc_1() {}
//! async fn calc_2() {}
//! async fn async_task() {
//!     // Join tasks into a single state machine on the stack
//!     join!(calc_1(), calc_2(), ...).await;
//! 
//!     // Flattened result collection
//!     let (a, b, c, ..) = join_all!(init_fs(), check_mem(), init_net()).await;
//! }
//! ```
//! 
//! ### 3. Timeouts and Guarding
//! 
//! ```rust
//! async fn timeout_example() {
//!     // Built-in timeout support for any Future
//!     match my_task().timeout(500).await {
//!         Ok(val) => handle(val),
//!         Err(_)  => handle_timeout(),
//!     }
//! }
//! 
//! ```
//! 
//! ### 4. Advanced Execution Pacing
//! 
//! `Pacer` allows you to strictly control the "rhythm" of your loops, essential for smooth 3D rendering or UI animations.
//! 
//! ```rust
//! async fn paced_loop() {
//!     let mut pacer = Pacer::new(60); // Target 60 FPS
//!     loop {
//!         pacer.burst(20).await;       // Allow a burst of 20 cycles
//!         pacer.throttle().await;      // Slow down to match target frequency
//!         pacer.step(10, true).await;  // Step-based pacing
//!     }
//! }
//! ```
//! 
//! ### 5. Oneshot, Channel and Signal...
//! 
//! Synchronization Primitives are lightweight, zero-cost and safe for asynchronous communication and synchronization.
//! 
//! ```rust
//! // 1. Create a channel for keyboard events with a capacity of 32
//! extern "efiapi" fn process(arg: *mut c_void) {
//!     let (tx, mut rx) = bounded_channel::<Key>(32);
//! 
//!     add!(
//!         executor => {
//!             // Task A: Producer - Polls hardware at a high frequency (e.g., 100Hz)
//!             100 -> async move {
//!                 loop {
//!                     if let Some(key) = poll_keyboard() {
//!                         tx.send(key); // Non-blocking send
//!                     }
//!                     Yield.await;
//!                 }
//!             },
//!     
//!             // Task B: Consumer - Processes game logic
//!             0 -> async move {
//!                 loop {
//!                     // The await point suspends the task if the queue is empty.
//!                     // Execution resumes as soon as the producer sends data
//!                     // and the executor polls this task again.
//!                     let key = (&mut rx).await;
//!                     process_game_logic(key);
//!                 }
//!             }
//!         }
//!     );
//! }
//! ```
//! 
//! --------------------------------
//! Multicore & Multi-Scheduler Concurrency
//! --------------------------------
//! `uefi-async` enabling seamless and safe parallel execution across multiple cores and schedulers.
//! It provides a robust suite of synchronization and control primitives designed to
//! handle the complexities of asynchronous multicore tasking.
//! 
//! ### Core-Safe Asynchronous Primitives
//! 
//! To ensure data integrity and prevent race conditions during parallel execution,
//! the framework provides three specialized pillars:
//! 
//! * **Event-based Futures (Event Listening):** Designed for non-blocking coordination, these futures allow tasks to
//! react to external signals or hardware interrupts across different cores without polling.
//! * **Synchronization Primitives (Data Integrity):** Reliable data sharing is critical when multiple schedulers
//! access the same memory space. We provide thread-safe containers and locks like **Async Mutexes** and
//! **Atomic Shared States** specifically tuned for UEFI.
//! * **Task Control Futures (Execution Management):** Granular control over the lifecycle of parallel tasks.
//! This includes **Structured Concurrency** to spawn, join, or cancel tasks across different schedulers,
//! and **Priority Steering** to direct critical tasks to specific cores.
//! 
//! ```rust
//! static ASSET_LOADED: Signal<TextureHandle> = Signal::new();
//! 
//! async fn background_loader() {
//!     let texture = load_texture_gop("logo.bmp").await;
//!     // Notify the renderer that the texture is ready
//!     ASSET_LOADED.signal(texture);
//! }
//! 
//! async fn renderer_task() {
//!     // Suspend execution until the signal is triggered
//!     let texture = ASSET_LOADED.wait().await;
//!     draw_to_screen(texture);
//! }
//! ```
//! 
//! --------------------------------
//! Installation
//! --------------------------------
//! Add this to your `Cargo.toml`:
//! 
//! ```toml
//! [dependencies]
//! uefi-async = "*"
//! ```
//! 
//! --------------------------------
//! Usage
//! --------------------------------
//! 
//! ### 1. Define your tasks
//! 
//! Tasks are standard Rust `async` functions or closures.
//! 
//! ### 2. Initialize and Run
//! 
//! Use the `add!` macro to set up your executor.
//! 
//! ```rust
//! extern crate alloc;
//! use alloc::boxed::Box;
//! use uefi_async::nano_alloc::{Executor, TaskNode};
//! 
//! async fn calc_1() {}
//! async fn calc_2() {}
//! 
//! extern "efiapi" fn process(arg: *mut c_void) {
//!     // 1. Create executor
//!     Executor::new()
//!         // 2. Register tasks
//!         .add(&mut TaskNode::new(Box::pin(calc_1()), 0))
//!         .add(&mut TaskNode::new(Box::pin(calc_2()), 60))
//!         // 3. Run the event loop
//!         .run_forever();
//! }
//! ```
//! 
//! or more advanced usage:
//! 
//! ```rust
//! extern crate alloc;
//! use uefi_async::nano_alloc::{Executor, add};
//! use uefi_async::util::tick;
//! 
//! async fn af1() {}
//! async fn af2(_: usize) {}
//! async fn af3(_: usize, _:usize) {}
//! 
//! extern "efiapi" fn process(arg: *mut c_void) {
//!     if arg.is_null() { return }
//!     let ctx = unsafe { &mut *arg.cast::<Context>() };
//!     let core = ctx.mp.who_am_i().expect("Failed to get core ID");
//! 
//!     // 1. Create executor
//!     let mut executor1 = Executor::new();
//!     let mut executor2 = Executor::new();
//!     let mut cx = Executor::init_step();
//! 
//!     let offset = 20;
//!     // 2. Use the macro to register tasks
//!     // Syntax: executor => { frequency -> future }
//!     add! (
//!         executor1 => {
//!             0  -> af1(),        // Runs at every tick
//!             60 -> af2(core),    // Runs at 60 HZ
//!         },
//!         executor2 => {
//!             10u64.saturating_sub(offset) -> af3(core, core),
//!             30 + 10                      -> af1(),
//!         },
//!     );
//! 
//!     loop {
//!         calc_sync(core);
//! 
//!         // 3. Run the event loop manually
//!         executor1.run_step(tick(), &mut cx);
//!         executor2.run_step(tick(), &mut cx);
//!     }
//! }
//! ```
//! 
//! ### 3. Using various control flows, signals, and tunnels...
//! 
//! ```rust
//! // Example: Producer task on Core 1, Consumer task on Core 0
//! extern "efiapi" fn process(arg: *mut c_void) {
//!     if arg.is_null() { return }
//!     let ctx = unsafe { &mut *arg.cast::<Context>() };
//!     let core = ctx.mp.who_am_i().expect("Failed to get core ID");
//! 
//!     let (tx, rx) = unbounded_channel::channel::<PhysicsResult>();
//!     let mut executor = Executor::new();
//!     if core == 1 {
//!         add!(executor => { 20 -> producer(tx)});
//!         executor.run_forever();
//!     }
//!     if core == 0 {
//!         add!(executor => { 0 -> consumer(tx)});
//!         executor.run_forever();
//!     }
//! }
//! 
//! // Task running on Core 1's executor
//! async fn producer(tx: ChannelSender<PhysicsResult>) {
//!     let result = heavy_physics_calculation();
//!     tx.send(result); // Non-blocking atomic push
//! }
//! 
//! // Task running on Core 0's executor
//! async fn consumer(rx: ChannelReceiver<PhysicsResult>) {
//!     // This will return Poll::Pending and yield CPU if the queue is empty,
//!     // allowing the executor to run other tasks (like UI rendering).
//!     let data = rx.await;
//!     update_gpu_buffer(data);
//! }
//! ```
//! 
//! --------------------------------
//! Why use `uefi-async`?
//! --------------------------------
//! In UEFI development, managing multiple periodic tasks (like polling keyboard input while updating a UI
//! or handling network packets) manually can lead to "spaghetti code." `uefi-async` allows you to write clean,
//! linear `async/await` code while the executor ensures that timing constraints are met without a heavy OS-like scheduler.
//! 
//! --------------------------------
//! Acknowledgments
//! --------------------------------
//! 
//! This code is inspired by the approach in these Rust crate:
//! 
//! [uefi](https://crates.io/crates/uefi),
//! [crossbeam](https://crates.io/crates/crossbeam),
//! [futures-concurrency](https://crates.io/crates/futures-concurrency),
//! [static_cell](https://crates.io/crates/static_cell),
//! [embassy-executor](https://crates.io/crates/embassy-executor),
//! [edge-executor](https://crates.io/crates/edge-executor),
//! [num_enum](https://crates.io/crates/num_enum),
//! [nblfq](https://crates.io/crates/nblfq),
//! [cassette](https://crates.io/crates/cassette),
//! [st3](https://crates.io/crates/st3),
//! [async-recursion](https://crates.io/crates/async-recursion),
//! [talc](https://crates.io/crates/talc),
//! [spin](https://crates.io/crates/spin),
//! [pin-project](https://crates.io/crates/pin-project).
//! 
//! --------------------------------
//! Contributing
//! --------------------------------
//! I am still a learner when it comes to low-level async architecture, and many parts of this project might have
//! been implemented in a "roundabout" way. The performance might not be optimal yet,
//! and it has **not** been rigorously tested for production environments.
//! 
//! I would deeply appreciate any guidance, feedback, or suggestions from the community.
//! 
//! **Pull Requests are extremely welcome!** Whether it's a bug fix, performance optimization, or code refactoring,
//! I am open to all contributions and will accept every PR that helps improve this project.
//! Let's make UEFI Great Again!
//! 
//! --------------------------------
//! License
//! --------------------------------
//! MIT or Apache-2.0.

#![warn(unreachable_pub)]
#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(any(feature = "nano-alloc", feature = "alloc"))]
extern crate alloc;

/// Utility functions for hardware timing and platform-specific operations.
///
/// Includes the TSC-based tick counter and frequency calibration.
pub mod common;
pub use common::*;

/// Static task management module.
///
/// This module provides a mechanism for running the executor without
/// a dynamic memory allocator, utilizing static memory or stack-allocated
/// task nodes. Useful for highly constrained environments.
#[cfg(feature = "static")]
pub mod no_alloc;

/// Standard asynchronous executor implementation using `alloc`.
///
/// Provides the Executor and TaskNode types that rely on
/// `Box` and `Pin` for flexible task management.
/// Requires a global allocator to be defined.
#[cfg(feature = "alloc")]
pub mod dynamic;

/// Helper module for setting up a global allocator in UEFI.
///
/// When enabled, this module provides a bridge between the Rust
/// memory allocation API and the UEFI Boot Services memory allocation functions.
#[cfg(feature = "global-allocator")]
pub mod global_allocator;

/// Specialized, lightweight memory allocator for constrained systems.
///
/// A minimal allocator implementation designed to have a very small
/// footprint, specifically optimized for managing asynchronous task nodes.
#[cfg(feature = "nano-alloc")]
pub mod nano_alloc;
