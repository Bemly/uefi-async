use core::cell::{RefCell, UnsafeCell};
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};
use spin::Mutex;

/// Single-core, multi-asynchronous
///
/// Usage:
/// ```rust,no_run
/// // 定义一个传输 3D 模型元数据的信号
/// static MODEL_READY: Signal<ModelInfo> = Signal::new();
///
/// async fn loader_task() {
///     let info = load_from_disk().await;
///     MODEL_READY.signal(info); // 发送数据
/// }
///
/// async fn render_task() {
///     // 异步等待数据到达
///     let info = MODEL_READY.wait().await;
///     println!("Loading model: {}", info.name);
///     draw_mesh(info).await;
/// }
/// ```
#[derive(Debug)]
pub struct Signal<Data> (RefCell<Option<Data>>);
#[derive(Debug)]
pub struct _Signal<'bemly_, Data> (&'bemly_ Signal<Data>);

impl<Data> Signal<Data> {
    pub const fn new() -> Self { Self(RefCell::new(None)) }
    /// 发送信号并存入数据
    pub fn signal(&self, value: Data) { *self.0.borrow_mut() = Some(value) }

    /// 获取等待信号的 Future
    pub fn wait(&self) -> _Signal<'_, Data> { _Signal(self) }

    /// 检查信号是否已被触发（不消耗数据）
    pub fn is_triggered(&self) -> bool { self.0.borrow().is_some() }
}
impl<Data> Future for _Signal<'_, Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.0.borrow_mut().take() {
            Some(data) => Poll::Ready(data),
            None => Poll::Pending,
        }
    }
}

/// Single-core, multi-asynchronous, Unsafe, Fast
#[derive(Debug)]
pub struct UnsafeSignal<Data> (UnsafeCell<Option<Data>>);
#[derive(Debug)]
pub struct _UnsafeSignal<'bemly_, Data> (&'bemly_ UnsafeSignal<Data>);
unsafe impl<Data> Sync for UnsafeSignal<Data> {}
impl<Data> UnsafeSignal<Data> {
    pub const fn new() -> Self { Self(UnsafeCell::new(None)) }
    pub fn signal(&self, value: Data) { unsafe { *self.0.get() = Some(value) } }

    pub fn is_triggered(&self) -> bool { unsafe { (*self.0.get()).is_some() } }

    pub fn wait(&self) -> _UnsafeSignal<'_, Data> { _UnsafeSignal(self) }
}
impl<Data> Future for _UnsafeSignal<'_, Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let data_ptr = self.0.0.get();
        unsafe {
            if (*data_ptr).is_some() {
                Poll::Ready((*data_ptr).take().expect("Signal data is None"))
            } else { Poll::Pending }
        }
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
#[derive(Debug)]
pub struct MultiCoreSignal<Data> {
    data: Mutex<Option<Data>>,
    has_data: AtomicBool,
}
#[derive(Debug)]
pub struct _MultiCoreSignal<'bemly_, Data> (&'bemly_ MultiCoreSignal<Data>);

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

