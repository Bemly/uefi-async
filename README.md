uefi-async
================================
A lightweight, zero-cost asynchronous executor designed specifically for UEFI environments or bare-metal Rust. It provides a simple task scheduler based on a intrusive linked-list and a procedural macro to simplify task registration.

WIP
--------------------------------
currently only `nano_alloc` feature is supported.

Features
--------------------------------
* **No-Std Compatible**: Designed for environments without a standard library (only requires `alloc`).
* **Intrusive Linked-List**: No additional collection overhead for managing tasks.
* **Frequency-Based Scheduling**: Define tasks to run at specific frequencies (Hz), automatically converted to hardware ticks.
* **Macro-Driven Syntax**: A clean, declarative DSL to assign tasks to executors.

Architecture
--------------------------------
The project consists of two main components:

1. **The Executor**: A round-robin scheduler that polls `TaskNode`s based on timing requirements.
2. **The Task Macro**: A procedural macro that handles the boilerplate of pinning futures and registering them to executors.

Installation
--------------------------------
Add this to your `Cargo.toml`:

```toml
[dependencies]
uefi-async = "*"

```

Usage
--------------------------------

### 1. Define your tasks

Tasks are standard Rust `async` functions or closures.

### 2. Initialize and Run

Use the `add!` macro to set up your executor.

```rust
extern crate alloc;
use alloc::boxed::Box;
use uefi_async::nano_alloc::{Executor, TaskNode};

async fn calc_1() {}
async fn calc_2() {}

extern "efiapi" fn process(arg: *mut c_void) {
    // 1. Create executor
    Executor::new()
        // 2. Register tasks
        .add(&mut TaskNode::new(Box::pin(calc_1()), 0))
        .add(&mut TaskNode::new(Box::pin(calc_2()), 60))
        // 3. Run the event loop
        .run_forever();
}
```

or more advanced usage:

```rust
extern crate alloc;
use uefi_async::nano_alloc::{Executor, add};
use uefi_async::util::tick;

async fn af1() {}
async fn af2(_: usize) {}
async fn af3(_: usize, _:usize) {}

extern "efiapi" fn process(arg: *mut c_void) {
    if arg.is_null() { return }
    let ctx = unsafe { &mut *arg.cast::<Context>() };
    let core = ctx.mp.who_am_i().expect("Failed to get core ID");

    // 1. Create executor
    let mut executor1 = Executor::new();
    let mut executor2 = Executor::new();
    let mut cx = Executor::init_step();
    
    let offset = 20;
    // 2. Use the macro to register tasks
    // Syntax: executor => { frequency -> future }
    add! (
        executor1 => {
            0  -> af1(),        // Runs at every tick
            60 -> af2(core),    // Runs at 60 HZ
        },
        executor2 => {
            10u64.saturating_sub(offset) -> af3(core, core),
            30 + 10                      -> af1(),
        },
    );

    loop {
        calc_sync(core);
        
        // 3. Run the event loop manually
        executor1.run_step(tick(), &mut cx);
        executor2.run_step(tick(), &mut cx);
    }
}
```

Why use `uefi-async`?
--------------------------------
In UEFI development, managing multiple periodic tasks (like polling keyboard input while updating a UI or handling network packets) manually can lead to "spaghetti code." `uefi-async` allows you to write clean, linear `async/await` code while the executor ensures that timing constraints are met without a heavy OS-like scheduler.

License
--------------------------------
This project is licensed under the MIT License or Apache-2.0. (temporary)