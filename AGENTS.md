# Agent Instruction: Zero-Alloc Multi-Core Task-Stealing Executor (UEFI)

## 1. Project Vision
Build a `no_std`, 0-allocation, high-performance asynchronous executor for UEFI environments. The goal is to provide an "implicit" async experience where users can write standard `async/await` code that "infects" the system, automatically distributing tasks across multiple CPU cores via work-stealing.

## 2. Core Architectural Principles
- **Zero Allocation:** No `alloc` crate. All task memory is statically allocated at compile-time or via fixed-size pre-allocated buffers.
- **Intrusive Task Nodes:** Task states (Future + Metadata) are stored in `TaskNode` structures that contain their own linked-list pointers.
- **Work-Stealing:** Every core (BSP and APs) runs a local executor. If a local queue is empty, the core attempts to "steal" a `TaskNode` from another core's queue using atomic operations.
- **Preemptive-Like Cooperation:** Implement a "budget" or "timer" based yield mechanism using macros to prevent long-running async tasks (like 3D rendering) from starving I/O (like keyboard/mouse).

## 3. Data Structure Specifications

### 3.1 TaskNode<F>
Each task must be wrapped in a `TaskNode` with the following memory layout:
- `state`: `AtomicUsize` (States: Idle, Queued, Running, Stealing, Completed).
- `future`: `UnsafeCell<F>` (The generated state machine).
- `pointers`: `AtomicPtr` for `prev` and `next` to form intrusive lists.
- `waker_context`: Metadata to track which core currently "owns" the task.

### 3.2 Global & Local Queues
- **Local Queue:** Lock-free SPSC (Single-Producer Single-Consumer) or similar for high-speed local access.
- **Global Injector:** A fallback queue for newly spawned tasks.
- **Steal Mechanism:** Use `compare_exchange` (CAS) on the `TaskNode.state` to ensure only one core can transition a task from `Queued` to `Running`.

## 4. Implementation Requirements for AI Assistant

### 4.1 Memory Safety & Pinning
- All tasks must be `Pinned` because they are stored in static memory and cannot be moved once execution starts.
- Use `UnsafeCell` and `Atomic` operations to bypass the borrow checker safely in a multi-core context.

### 4.2 Multi-Core Synchronization
- Implement the `Waker` trait by wrapping a pointer to the `TaskNode`.
- `wake()` must be multi-core safe: it should re-insert the task into a ready-queue (local or global) and potentially signal a CPU core via IPI (Inter-Processor Interrupt) if the core is halted.

### 4.3 UEFI Integration
- Bridge UEFI Events to Rust `Wakers`.
- Wrap `EFI_SIMPLE_TEXT_INPUT_PROTOCOL` and `EFI_GRAPHICS_OUTPUT_PROTOCOL` into `Stream` and `Future` traits.

## 5. Development Roadmap for Agent
1. **Module 1: Atomic State Machine.** Define the `TaskState` and atomic transition logic.
2. **Module 2: Intrusive List.** Implement the 0-alloc linked list for task management.
3. **Module 3: The Executor Loop.** Implement the `poll` loop with work-stealing logic.
4. **Module 4: Macro System.** Create `spawn_static!` and `#[entry]` macros to hide executor complexity.
5. **Module 5: Yielding Macro.** Create a macro to inject `yield_now().await` into heavy loops based on a counter or `RDTSC`.

## 6. Constraints
- Strict `no_std`.
- No `Box`, `Vec`, or `Arc`. Use `StaticCell` or raw pointers where necessary.
- Target Architecture: `x86_64-unknown-uefi`.