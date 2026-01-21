use core::cell::Cell;
use core::task::{Poll, Context};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// Single-core, multi-asynchronous
///
/// Usage:
/// ```rust,no_run
/// static TEXTURE_LOADED: Signal = Signal::new();
/// async fn loader_task() {
///     load_gpu_textures();
///     TEXTURE_LOADED.signal();
/// }
/// async fn render_task() {
///     TEXTURE_LOADED.wait().await;
///     loop {
///         draw_frame();
///         Yield.await;
///     }
/// }
/// ```
pub struct Signal (Cell<bool>); // triggered

impl Signal {
    pub const fn new() -> Self { Self(Cell::new(false)) }
    pub fn signal(&self) { self.0.set(true) }
    pub fn wait(&self) -> _Signal<'_> { _Signal(self) }
}

pub struct _Signal<'bemly_> (&'bemly_ Signal); // signal

impl Future for _Signal<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0.0.get() { Poll::Ready(()) } else { Poll::Pending }
    }
}

/// Multi-core, multi-asynchronous
///
/// Usage:
/// ```rust,no_run
/// static RENDER_SIGNAL: MultiCoreSignal<u64> = MultiCoreSignal::new();
/// async fn physics_task() {
///     let mut frame_count = 0;
///     loop {
///         do_physics_calc();
///         frame_count += 1;
///         RENDER_SIGNAL.signal(frame_count);
///         Yield.await;
///     }
/// }
/// async fn render_task() {
///     loop {
///         let frame = RENDER_SIGNAL.wait().await;
///         draw_frame(frame);
///         Yield.await;
///     }
/// }
/// ```
pub struct MultiCoreSignal<Data> {
    data: Mutex<Option<Data>>,
    has_data: AtomicBool,
}

impl<Data> MultiCoreSignal<Data> {
    pub const fn new() -> Self { Self { data: Mutex::new(None), has_data: AtomicBool::new(false) } }
    pub fn signal(&self, data: Data) {
        *self.data.lock() = Some(data);
        self.has_data.store(true, Ordering::Release)
    }
    pub fn wait(&self) -> _MultiCoreSignal<'_, Data> { _MultiCoreSignal(self) }
    pub fn reset(&self) {
        self.has_data.store(false, Ordering::Relaxed);
        *self.data.lock() = None
    }
}

pub struct _MultiCoreSignal<'bemly_, Data> (&'bemly_ MultiCoreSignal<Data>);
impl<'bemly_, Data> Future for _MultiCoreSignal<'bemly_, Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.0.has_data.load(Ordering::Acquire) { return Poll::Pending }
        match self.0.data.lock().take() {
            Some(data) => {
                self.0.has_data.store(false, Ordering::Relaxed);
                Poll::Ready(data)
            },
            None => Poll::Pending,
        }
    }
}

