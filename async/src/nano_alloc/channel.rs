use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use alloc::rc::Rc;
use alloc::sync::Arc;
use core::pin::Pin;
use core::task::{Context, Poll};
use crossbeam::queue::{ArrayQueue, SegQueue};
use spin::Mutex;
use crate::Yield;

pub mod single {
    use super::*;

    pub mod oneshot {
        use super::*;

        /// A synchronization primitive for sending a single value between asynchronous tasks.
        ///
        /// This channel is specifically designed for **single-core, multi-asynchronous** environments
        /// such as UEFI applications. It allows one task to provide a value (the Sender) and
        /// another task to asynchronously wait for that value (the Receiver).
        ///
        /// Since the environment is single-threaded, it utilizes `Rc<RefCell<Option<T>>>`
        /// instead of atomic primitives to maintain low overhead and `no_std` compatibility.
        ///
        /// # Examples
        ///
        /// ```rust
        /// async fn example() {
        ///     // 1. Create a channel pair
        ///     let (tx, rx) = oneshot::new::<u64>();
        ///
        ///     // 2. The Receiver waits for data without blocking the entire system
        ///     // In a real scenario, tx would be moved into a separate TaskNode
        ///     let data = rx.await;
        ///
        ///     // 3. Data is received and execution resumes
        /// }
        /// ```
        #[derive(Debug)]
        pub struct Oneshot<Data> (Rc<RefCell<Option<Data>>>);
        /// A future that resolves to the value sent by the corresponding [`Sender`].
        #[derive(Debug)]
        pub struct OneShotReceiver<Data> (Rc<RefCell<Option<Data>>>);

        impl<Data> Oneshot<Data> {
            /// Creates a new oneshot channel, returning the sender/receiver halves.
            ///
            /// All data sent via the [`Sender`] will become available to the [`Receiver`].
            pub fn new() -> (Self, OneShotReceiver<Data>) {
                let data = Rc::new(RefCell::new(None));
                (Self(data.clone()), OneShotReceiver(data))
            }
            /// Completes the channel by sending a value.
            ///
            /// This consumes the sender, as only one value can ever be sent.
            pub fn send(self, value: Data) { *self.0.borrow_mut() = Some(value) }
        }

        impl<Data> Future for OneShotReceiver<Data> {
            type Output = Data;
            /// Polls the receiver to check if the value has been sent.
            ///
            /// If the value is present, it returns [`Poll::Ready`].
            /// Otherwise, it returns [`Poll::Pending`], allowing the executor to
            /// switch to other tasks (cooperative multitasking).
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Data> {
                match self.0.borrow_mut().take() {
                    Some(val) => Poll::Ready(val),
                    None => Poll::Pending,
                }
            }
        }
    }

    pub mod unbounded_channel {
        use super::*;

        /// A Single-Core, Multi-Producer, Single-Consumer (MPSC) asynchronous channel.
        ///
        /// This channel is designed for single-core, multi-asynchronous environments
        /// typical in UEFI systems. It allows multiple senders to push data into a
        /// shared queue, while a single receiver waits for and consumes that data asynchronously.
        ///
        /// # Features
        /// * **Unbounded Capacity**: The queue grows dynamically (powered by `VecDeque`).
        /// * **Non-blocking Sending**: Sending is an immediate, synchronous operation.
        /// * **Asynchronous Receiving**: The receiver implements [`Future`], returning
        ///   `Poll::Pending` when empty and `Poll::Ready(Data)` when data arrives.
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// // Main logic consuming events
        /// async fn main_logic(mut rx: ChannelReceiver<KeyEvent>) {
        ///     loop {
        ///         // If the queue is empty, .await will yield control back to the executor (Pending).
        ///         // Once data is pushed by a sender, the task is re-polled and returns Ready.
        ///         let key = rx.await;
        ///         match key {
        ///             KeyEvent::ScanCode(0x01) => break, // Exit on ESC key
        ///             _ => render_game(key),
        ///         }
        ///     }
        /// }
        ///
        /// // Input task producing events
        /// async fn input_poller(tx: ChannelSender<KeyEvent>) {
        ///     loop {
        ///         if let Some(key) = poll_uefi_key() {
        ///             // Synchronously push data into the channel
        ///             tx.send(key);
        ///         }
        ///         // Yield execution to allow the main_logic or other tasks to run
        ///         Yield.await;
        ///     }
        /// }
        /// ```
        pub fn channel<Data>() -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Rc::new(RefCell::new(VecDeque::new()));
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }

        #[derive(Clone, Debug)]
        pub struct ChannelSender<Data> (Rc<RefCell<VecDeque<Data>>>);
        #[derive(Debug)]
        pub struct ChannelReceiver<Data> (Rc<RefCell<VecDeque<Data>>>);
        impl<Data> ChannelSender<Data> {
            /// Sends a value into the channel.
            ///
            /// Since this is an unbounded channel, this operation is synchronous and
            /// will not block the current task.
            pub fn send(&self, value: Data) { self.0.borrow_mut().push_back(value); }
        }
        impl<Data> Future for ChannelReceiver<Data> {
            type Output = Data;
            /// Polls the channel for the next available piece of data.
            ///
            /// Returns [`Poll::Ready(value)`] if the queue contains data.
            /// Returns [`Poll::Pending`] if the queue is empty, signaling the executor
            /// to switch to another task until the next polling cycle.
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                match self.0.borrow_mut().pop_front() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }


    }

    pub mod bounded_channel {
        use super::*;

        /// A lightweight, single-core, multi-producer single-consumer (MPSC) asynchronous channel.
        ///
        /// This channel is specifically designed for single-threaded executors (like those in UEFI).
        /// It allows asynchronous tasks to communicate without the need for complex synchronization
        /// primitives like Mutex or Atomic types, leveraging the non-preemptive nature of the executor.
        ///
        /// # Safety
        /// This implementation uses raw pointers. It is safe **only** within a single-threaded
        /// executor where the `Receiver` is guaranteed to outlive all `Sender` instances,
        /// or where the `Sender` is not used after the `Receiver` is dropped.
        ///
        /// # Example
        /// ```rust,no_run
        /// // 1. Create a channel for keyboard events with a capacity of 32
        /// let (tx, mut rx) = bounded_channel::<Key>(32);
        ///
        /// add!(
        ///     executor => {
        ///         // Task A: Producer - Polls hardware at a high frequency (e.g., 100Hz)
        ///         100 -> async move {
        ///             loop {
        ///                 if let Some(key) = poll_keyboard() {
        ///                     tx.send(key); // Non-blocking send
        ///                 }
        ///                 Yield.await;
        ///             }
        ///         },
        ///
        ///         // Task B: Consumer - Processes game logic
        ///         0 -> async move {
        ///             loop {
        ///                 // The await point suspends the task if the queue is empty.
        ///                 // Execution resumes as soon as the producer sends data
        ///                 // and the executor polls this task again.
        ///                 let key = (&mut rx).await;
        ///                 process_game_logic(key);
        ///             }
        ///         }
        ///     }
        /// );
        /// ```
        pub fn channel<Data>(capacity: usize) -> (UnsafeChannelSender<Data>, UnsafeChannelReceiver<Data>) {
            let mut inner = Box::new(ChannelInner(VecDeque::with_capacity(capacity)));
            let sender_ptr = &mut *inner as *mut ChannelInner<Data>;

            (UnsafeChannelSender(sender_ptr), UnsafeChannelReceiver(inner))
        }

        #[derive(Debug)]
        struct ChannelInner<Data> (VecDeque<Data>);
        /// Handle to send data into the channel.
        ///
        /// Can be cloned to allow multiple producers to send data to a single receiver.
        #[derive(Clone, Debug)]
        pub struct UnsafeChannelSender<Data> (*mut ChannelInner<Data>);
        /// Handle to receive data from the channel.
        ///
        /// Implements [`Future`], yielding data when the internal queue is not empty.
        #[derive(Debug)]
        pub struct UnsafeChannelReceiver<Data> (Box<ChannelInner<Data>>);
        impl<Data> UnsafeChannelSender<Data> {
            /// Sends a value into the channel.
            ///
            /// This operation is non-blocking and assumes the internal buffer has
            /// enough capacity or can grow dynamically.
            pub fn send(&self, value: Data) {
                // Safety: In a single-core Executor, the Receiver owns the Box.
                // As long as the Executor holds the tasks, the inner pointer remains valid.
                unsafe { (*self.0).0.push_back(value) }
            }
        }
        impl<Data> Future for UnsafeChannelReceiver<Data> {
            type Output = Data;
            fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                match self.0.0.pop_front() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }


    }
}

pub mod multiple {
    use super::*;

    pub mod oneshot {
        use super::*;

        /// A multi-core, synchronization-safe communication primitive for sending a single value
        /// between asynchronous tasks.
        ///
        /// This implementation uses an `Arc<Mutex<Option<T>>>` internally, making it suitable
        /// for cross-core communication in multi-processor UEFI environments where tasks
        /// in different executors need to synchronize data.
        ///
        /// # Usage
        ///
        /// ```rust
        /// // Example: Multi-core synchronization
        /// // Core 1 performs heavy physics while Core 0 handles rendering.
        /// let (tx, rx) = Oneshot::new();
        ///
        /// // Inside Core 1's executor:
        /// async move {
        ///     let result = heavy_physics_calculation();
        ///     tx.send(result); // Signal completion and send data
        /// };
        ///
        /// // Inside Core 0's executor:
        /// async move {
        ///     let data = rx.await; // Suspends execution until Core 1 sends the data
        ///     update_gpu_buffer(data);
        /// };
        /// ```
        #[derive(Debug)]
        pub struct Oneshot<Data>(Arc<Mutex<Option<Data>>>);

        /// The receiving half of the [`Oneshot`] channel.
        ///
        /// This struct implements [`Future`], allowing it to be `.await`ed until
        /// the sender provides a value.
        #[derive(Debug)]
        pub struct OneShotReceiver<Data>(Arc<Mutex<Option<Data>>>);

        impl<Data> Oneshot<Data> {
            /// Creates a new multi-core safe Oneshot channel pair.
            ///
            /// Returns a sender ([`Oneshot`]) and a receiver ([`OneShotReceiver`]).
            pub fn new() -> (Self, OneShotReceiver<Data>) {
                let data = Arc::new(Mutex::new(None));
                (Self(data.clone()), OneShotReceiver(data))
            }

            /// Consumes the sender and provides a value to the receiver.
            ///
            /// Since this involves an `Arc` and a `Mutex`, it can be safely called
            /// from a different CPU core than the receiver.
            pub fn send(self, value: Data) {
                let mut lock = self.0.lock();
                *lock = Some(value);
            }
        }

        impl<Data> Future for OneShotReceiver<Data> {
            type Output = Data;

            /// Polls the receiver to check if data has been sent.
            ///
            /// Returns [`Poll::Ready`] if the data is available, otherwise returns
            /// [`Poll::Pending`] and yields control back to the executor.
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                // Attempt to acquire the lock and extract the data
                let mut lock = self.0.lock();
                match lock.take() {
                    Some(val) => Poll::Ready(val),
                    None => Poll::Pending,
                }
            }
        }

    }

    pub mod unbounded_channel {
        use super::*;

        /// A lock-free, multi-core safe asynchronous channel for inter-task communication.
        ///
        /// This channel is designed for high-performance scenarios where data needs to be
        /// transferred between different executor instances running on different CPU cores.
        /// It utilizes an atomic `SegQueue` to ensure that `send` operations are non-blocking
        /// and `poll` operations are thread-safe.
        ///
        /// # Architecture
        ///
        /// The channel consists of a [`ChannelSender`] and a [`ChannelReceiver`].
        /// The sender performs atomic push operations, while the receiver acts as a
        /// [`Future`], yielding control back to the executor if the queue is empty.
        ///
        /// # Usage
        ///
        /// ```rust,no_run
        /// // Example: Producer task on Core 1, Consumer task on Core 0
        /// let (tx, rx) = unbounded_channel::channel::<PhysicsResult>();
        ///
        /// // Task running on Core 1's executor
        /// async fn producer(tx: ChannelSender<PhysicsResult>) {
        ///     let result = heavy_physics_calculation();
        ///     tx.send(result); // Non-blocking atomic push
        /// }
        ///
        /// // Task running on Core 0's executor
        /// async fn consumer(rx: ChannelReceiver<PhysicsResult>) {
        ///     // This will return Poll::Pending and yield CPU if the queue is empty,
        ///     // allowing the executor to run other tasks (like UI rendering).
        ///     let data = rx.await;
        ///     update_gpu_buffer(data);
        /// }
        /// ```
        pub fn channel<Data>()
            -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Arc::new(SegQueue::new());
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }

        #[derive(Clone, Debug)]
        pub struct ChannelSender<Data>(Arc<SegQueue<Data>>);
        #[derive(Debug)]
        pub struct ChannelReceiver<Data>(Arc<SegQueue<Data>>);

        impl<Data> ChannelSender<Data> {
            /// Sends a value into the channel.
            ///
            /// This operation is extremely efficient, typically requiring only a
            /// single Atomic CAS (Compare-And-Swap) without acquiring any locks.
            #[inline(always)]
            pub fn send(&self, value: Data) { self.0.push(value) }
        }

        impl<Data> Future for ChannelReceiver<Data> {
            type Output = Data;

            /// Polls the receiver for a value.
            ///
            /// Returns [`Poll::Ready(Data)`] if a value is available in the queue.
            /// Returns [`Poll::Pending`] if the queue is empty, suspending the
            /// current async task until the next executor tick.
            #[inline]
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                // SegQueue's pop is thread-safe and lock-free.
                // In a multi-core scheduler, we return Pending to yield the CPU
                // if no data is currently available.
                match self.0.pop() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }
    }

    pub mod bounded_channel {
        use super::*;

        /// A multicore-safe, asynchronous, bounded MPMC (Multi-Producer, Multi-Consumer) channel.
        ///
        /// This channel is built on top of a lock-free `ArrayQueue`, making it ideal for
        /// communication between different CPU cores in a UEFI environment without the
        /// overhead of traditional mutexes.
        ///
        /// # Characteristics
        /// * **Lock-Free**: Uses Atomic CAS (Compare-And-Swap) operations for high-performance synchronization.
        /// * **Bounded**: Fixed capacity prevents memory exhaustion and provides backpressure.
        /// * **Async-Aware**: Composed of `send_async` and a `Receiver` that implements `Future`.
        ///
        /// # Usage Example
        ///
        /// ```rust
        /// // In UEFI Core 0 (Rendering Task)
        /// async fn render_task(rx: ChannelReceiver<FrameData>) {
        ///     loop {
        ///         // Suspends the task if the queue is empty, allowing other tasks to run.
        ///         let data = rx.await;
        ///         draw_frame(data);
        ///     }
        /// }
        ///
        /// // In UEFI Core 1 (Physics Task)
        /// async fn physics_task(tx: ChannelSender<FrameData>) {
        ///     loop {
        ///         let frame = calculate_physics();
        ///         // If the queue is full, it yields control back to the executor
        ///         // and retries on the next polling cycle.
        ///         tx.send_async(frame).await;
        ///     }
        /// }
        /// ```
        pub fn channel<Data>(cap: usize) -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Arc::new(ArrayQueue::new(cap));
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }

        /// Multicore, multi-asynchronous, bounded
        #[derive(Clone,Debug)]
        pub struct ChannelSender<Data> (Arc<ArrayQueue<Data>>);
        #[derive(Debug)]
        pub struct ChannelReceiver<Data> (Arc<ArrayQueue<Data>>);

        impl<Data> ChannelSender<Data> {
            /// Attempts to send a value into the channel without blocking.
            ///
            /// Returns `Ok(())` if successful, or `Err(Data)` if the queue is full.
            #[inline(always)]
            pub fn try_send(&self, value: Data) -> Result<(), Data> { self.0.push(value) }

            /// Sends a value asynchronously.
            ///
            /// If the channel is full, this method will `Yield` control back to the executor,
            /// effectively suspending the current task until the next executor tick.
            pub async fn send_async(&self, mut data: Data) {
                loop {
                    match self.try_send(data) {
                        Ok(_) => return,
                        Err(d) => {
                            data = d;
                            // Queue is full; yield the current core's execution
                            // and wait for the next polling cycle to retry.
                            Yield.await;
                        }
                    }
                }
            }
        }

        impl<Data> Future for ChannelReceiver<Data> {
            type Output = Data;

            /// Polls the receiver for data.
            ///
            /// Internally uses `ArrayQueue::pop` which relies on atomic operations
            /// to maintain thread-safety across multiple cores.
            #[inline]
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                // If no data is available, return Pending.
                // The executor will switch to other available tasks.
                match self.0.pop() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }
    }
}





