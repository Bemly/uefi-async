use core::cell::{RefCell, UnsafeCell};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};
use spin::Mutex;

pub mod single {
    use super::*;

    /// A Single-Core, single-producer, single-consumer (SPSC) signaling primitive for asynchronous synchronization.
    ///
    /// `Signal` allows one asynchronous task to notify another task and optionally pass data.
    /// In a single-core, cooperative multitasking environment like UEFI, it acts as a
    /// lightweight "oneshot" channel without the need for complex locking or heap-allocated wakers.
    ///
    /// # Design
    /// The signal uses a `RefCell` to store data. The `wait()` method produces a `Future`
    /// that polls the internal state until the data is provided via `signal()`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// // Defined as a static or shared resource
    /// static ASSET_LOADED: Signal<TextureHandle> = Signal::new();
    ///
    /// async fn background_loader() {
    ///     let texture = load_texture_gop("logo.bmp").await;
    ///     // Notify the renderer that the texture is ready
    ///     ASSET_LOADED.signal(texture);
    /// }
    ///
    /// async fn renderer_task() {
    ///     // Suspend execution until the signal is triggered
    ///     let texture = ASSET_LOADED.wait().await;
    ///     draw_to_screen(texture);
    /// }
    /// ```
    #[derive(Debug)]
    pub struct Signal<Data> (RefCell<Option<Data>>);

    /// A `Future` that resolves when the associated [`Signal`] receives data.
    ///
    /// Created by the [`Signal::wait`] method.
    #[derive(Debug)]
    pub struct _Signal<'bemly_, Data> (&'bemly_ Signal<Data>);

    impl<Data> Signal<Data> {
        /// Creates a new, empty signal.
        pub const fn new() -> Self { Self(RefCell::new(None)) }
        /// Triggers the signal and stores the provided data.
        ///
        /// This will make the associated [`_Signal`] future resolve on its next poll.
        pub fn signal(&self, value: Data) { *self.0.borrow_mut() = Some(value) }

        /// Returns a future that resolves to the data when the signal is triggered.
        ///
        /// Note: The data is "consumed" (taken out) once the future resolves.
        pub fn wait(&self) -> _Signal<'_, Data> { _Signal(self) }

        /// Checks if the signal has been triggered without consuming the data.
        pub fn is_triggered(&self) -> bool { self.0.borrow().is_some() }
    }
    impl<Data> Future for _Signal<'_, Data> {
        type Output = Data;
        /// Polls the internal state of the signal.
        ///
        /// Returns [`Poll::Ready`] if data is present, consuming it from the signal.
        /// Otherwise, returns [`Poll::Pending`].
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            match self.0.0.borrow_mut().take() {
                Some(data) => Poll::Ready(data),
                None => Poll::Pending,
            }
        }
    }

    /// A low-level, zero-overhead signaling primitive for cross-task communication.
    /// Single-core, multi-asynchronous, Unsafe, Fast
    ///
    /// `UnsafeSignal` allows one asynchronous task to wait for a specific piece of data
    /// or a "trigger" event sent by another task or an interrupt handler.
    ///
    /// # Safety
    /// This type is marked as `Sync` to allow static definition. However, it uses
    /// `UnsafeCell` internally without locking. It is designed for single-threaded
    /// executors (like the UEFI executor) where re-entrancy is managed by the scheduler.
    #[derive(Debug)]
    pub struct UnsafeSignal<Data> (UnsafeCell<Option<Data>>);
    /// A handle returned by [`UnsafeSignal::wait`], implementing [`Future`].
    #[derive(Debug)]
    pub struct _UnsafeSignal<'bemly_, Data> (&'bemly_ UnsafeSignal<Data>);
    unsafe impl<Data> Sync for UnsafeSignal<Data> {}
    impl<Data> UnsafeSignal<Data> {
        /// Creates a new, untriggered signal.
        pub const fn new() -> Self { Self(UnsafeCell::new(None)) }
        /// Triggers the signal by providing a value.
        ///
        /// Any task currently `.await`-ing the associated [`_UnsafeSignal`] future
        /// will be resolved on its next poll.
        pub fn signal(&self, value: Data) { unsafe { *self.0.get() = Some(value) } }
        /// Checks if the signal has been triggered without consuming the data.
        pub fn is_triggered(&self) -> bool { unsafe { (*self.0.get()).is_some() } }
        /// Returns a future that resolves when the signal is triggered.
        ///
        /// # Example
        /// ```rust
        /// static ASYNC_EVENT: UnsafeSignal<u32> = UnsafeSignal::new();
        ///
        /// // In Task A:
        /// async fn wait_for_event() {
        ///     let data = ASYNC_EVENT.wait().await;
        ///     println!("Received: {}", data);
        /// }
        ///
        /// // In Task B (or Sync context):
        /// fn trigger_event() {
        ///     ASYNC_EVENT.signal(42);
        /// }
        /// ```
        pub fn wait(&self) -> _UnsafeSignal<'_, Data> { _UnsafeSignal(self) }
    }
    impl<Data> Future for _UnsafeSignal<'_, Data> {
        type Output = Data;
        /// Polls the signal to check if data is available.
        ///
        /// If the signal has been triggered, the data is taken from the signal
        /// (resetting it to an empty state) and returned as [`Poll::Ready`].
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            let data_ptr = self.0.0.get();
            unsafe {
                if (*data_ptr).is_some() {
                    // Takes the data out, leaving None in its place.
                    Poll::Ready((*data_ptr).take().expect("Signal data is None"))
                } else { Poll::Pending }
            }
        }
    }
}


pub mod multiple {
    use super::*;

    /// A cross-task/cross-core synchronization primitive for passing data between producers and consumers.
    ///
    /// `MultiCoreSignal` acts as a high-performance, single-slot mailbox. It allows one task
    /// (or core) to notify another task that data is ready. It uses an `AtomicBool` for
    /// fast "dirty" checks before acquiring a heavier `Mutex` lock, minimizing contention.
    ///
    /// # Design
    /// This signal is **reset-on-read**. When the consumer `.await`s the signal and successfully
    /// receives the data, the internal state is cleared, and subsequent polls will return
    /// `Poll::Pending` until the producer calls `signal()` again.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// static RENDER_SIGNAL: MultiCoreSignal<u64> = MultiCoreSignal::new();
    ///
    /// /// Producer: Runs physics calculations and notifies the renderer
    /// async fn physics_task() {
    ///     let mut frame_count = 0;
    ///     loop {
    ///         do_physics_calc();
    ///         frame_count += 1;
    ///
    ///         // Update the signal with the new frame index
    ///         RENDER_SIGNAL.signal(frame_count);
    ///
    ///         Yield.await;
    ///     }
    /// }
    ///
    /// /// Consumer: Waits for the signal and renders the frame
    /// async fn render_task() {
    ///     loop {
    ///         // Execution suspends here until RENDER_SIGNAL.signal() is called
    ///         let frame = RENDER_SIGNAL.wait().await;
    ///
    ///         draw_frame(frame);
    ///
    ///         // Yield once to allow other tasks to process before next wait
    ///         Yield.await;
    ///     }
    /// }
    /// ```
    #[derive(Debug)]
    pub struct MultiCoreSignal<Data> {
        data: Mutex<Option<Data>>,
        has_data: AtomicBool,
    }

    /// A handle representing the "wait" state of a [`MultiCoreSignal`].
    ///
    /// This struct implements [`Future`], allowing it to be integrated into
    /// an asynchronous executor.
    #[derive(Debug)]
    pub struct _MultiCoreSignal<'bemly_, Data> (&'bemly_ MultiCoreSignal<Data>);

    impl<Data> MultiCoreSignal<Data> {
        /// Creates a new, empty `MultiCoreSignal`.
        pub const fn new() -> Self { Self { data: Mutex::new(None), has_data: AtomicBool::new(false) } }
        /// Deposits data into the signal and notifies any waiting consumers.
        ///
        /// If data already exists in the slot and hasn't been read yet,
        /// it will be overwritten (Last-Write-Wins).
        pub fn signal(&self, data: Data) {
            *self.data.lock() = Some(data);
            self.has_data.store(true, Ordering::Release)
        }
        /// Returns a future that resolves when data is signaled.
        pub fn wait(&self) -> _MultiCoreSignal<'_, Data> { _MultiCoreSignal(self) }
        /// Manually clears the signal data and resets the notification flag.
        pub fn reset(&self) {
            self.has_data.store(false, Ordering::Relaxed);
            *self.data.lock() = None
        }
    }
    impl<'bemly_, Data> Future for _MultiCoreSignal<'bemly_, Data> {
        type Output = Data;
        /// Attempts to consume the data from the signal.
        ///
        /// If the `has_data` flag is set, it attempts to lock the internal data.
        /// If data is found, it is taken out (leaving `None`), the flag is reset,
        /// and `Poll::Ready(data)` is returned.
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            // Fast atomic check (Acquire) to avoid unnecessary Mutex locking
            if !self.0.has_data.load(Ordering::Acquire) { return Poll::Pending }
            match self.0.data.lock().take() {
                Some(data) => {
                    self.0.has_data.store(false, Ordering::Relaxed);
                    Poll::Ready(data)
                },
                // Fallback in case another consumer/core stole the data between the check and the lock
                None => Poll::Pending,
            }
        }
    }
}



