# AGENTS.md: Symmetric Multi-Core Zero-Alloc Async Executor

## 1. System Overview
A symmetric, no-std, zero-allocation asynchronous executor for UEFI. It uses a "Shadow Header" pattern with bit-packed control flags to support Poll-only, Interrupt-only, and Hybrid scheduling strategies with O(1) complexity.

## 2. Memory Layout & Data Structures

### A. TaskHeader (The Common Interface)
- **Constraint**: `#[repr(C)]` for stable memory offsets.
- **Fields**:
    1. `poll_handle: TaskTypeFn` (Offset 0): Address of the concrete `poll_wrapper<F>`.
    2. `control: AtomicU8` (Offset 8): Bit-packed field.
        - **Bits 0-1**: `WakePolicy` (00: PollOnly, 01: InterruptOnly, 10: Hybrid).
        - **Bit 7**: `WAKE_BIT` (0: Sleeping, 1: Pending/Waked).
    3. `occupied: AtomicU8` (Offset 9): Lifecycle state (0: EMPTY, 1: OCCUPIED).

### B. TaskSlot<F> (The Concrete Task)
- **Alignment**: `align(128)` to prevent False Sharing between cores.
- **Composition**: Contains a `TaskHeader` and an `UnsafeCell<MaybeUninit<F>>`.
- **Symmetry**: Any core can safely spawn any `Future` into any `TaskSlot`.

## 3. Wake-up Logic (Bit-Level Optimization)

### A. Atomic Wake Operation (ISR)
1. ISR receives a raw `*mut ()` (pointing to the `TaskHeader`).
2. Performs `header.control.fetch_or(0x80, Ordering::Release)`.
3. If the returned old value has Bit 7 == 0, the ISR triggers `schedule_task(ptr)` to re-queue the task. This ensures **Hardware-level Deduplication**.

### B. Scheduling Policies
- **PollOnly (00)**: Executor always polls the task upon acquisition.
- **InterruptOnly (01)**: `poll_wrapper` only executes `poll()` if Bit 7 was set.
- **Hybrid (10)**: Task is polled every cycle, but interrupts can force immediate re-queuing for lower latency.

## 4. Execution Workflow

### A. Executor::run_step() (The Blind Mover)
- **Fetch**: Pops from local `Worker` or steals from peer `Stealers`.
- **Dispatch**: Directly invokes `(ptr.read_as_fn())(ptr, waker)`.
- **Agnoticism**: The executor logic is policy-agnostic; it simply jumps to the handler.

### B. poll_wrapper<F>() (The Logic Controller)
1. **Unpack**: Casts `*mut ()` to `&TaskSlot<F>`, then `fetch_and(!0x80)` on `control` to read policy and clear the wake bit.
2. **Policy Enforcement**: Decides whether to poll the `future` based on policy and wake bit status.
3. **Lifecycle**: If `Poll::Ready`, calls `drop_in_place` and sets `occupied` to `EMPTY` (Release).

## 5. Multi-Core Synergy
- **Symmetric SMP**: BSP and APs are identical. All cores participate in spawning, stealing, and execution.
- **Load Balancing**: Uses the ST3 lock-free algorithm (`steal_and_pop`) for efficient task distribution.
- **Zero-Alloc**: Entirely static-memory based, ensuring safety for UEFI Boot and Runtime services.