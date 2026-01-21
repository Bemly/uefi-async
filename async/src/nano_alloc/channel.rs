use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use alloc::rc::Rc;
use core::pin::Pin;
use core::task::{Context, Poll};

/// A oneshot channel
/// Single-core, multi-asynchronous
///
/// Usage:
/// ```rust,no_run
/// async fn sender() {
///     let result = perform_heavy_calc();
///     tx.send(result); // 发送数据
/// }
/// async fn example() {
///     // 1. 创建一对通信端点
///     let (tx, rx) = Oneshot::<u64>::new();
///
///     // 2. 将发送端交给某个异步任务（比如后台加载）
///     join!(Box::pin(sender())); // TODO: join macro
///
///     // 3. 接收端在另一个任务中等待
///     // 在数据到达前，这行代码会返回 Poll::Pending，让出 CPU
///     let data = rx.await;
///
///     // 4. 数据到达后，代码继续执行
///     println!("Received: {}", data);
/// }
/// ```
#[derive(Debug)]
pub struct Oneshot<Data> (Rc<RefCell<Option<Data>>>);
#[derive(Debug)]
pub struct OneShotReceiver<Data> (Rc<RefCell<Option<Data>>>);

impl<Data> Oneshot<Data> {
    pub fn new() -> (Self, OneShotReceiver<Data>) {
        let data = Rc::new(RefCell::new(None));
        (Self(data.clone()), OneShotReceiver(data))
    }

    pub fn send(self, value: Data) { *self.0.borrow_mut() = Some(value) }
}

impl<Data> Future for OneShotReceiver<Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Data> {
        match self.0.borrow_mut().take() {
            Some(val) => Poll::Ready(val),
            None => Poll::Pending,
        }
    }
}

/// Single-core, multi-asynchronous MPMC
///
/// Usage:
/// ```rust,no_run
/// async fn main_logic(mut rx: ChannelReceiver<KeyEvent>) {
///     loop {
///         // 当队列为空时，这里会自动 Pending，让出 CPU
///         // 当有数据时，这里会被 Ready 激活
///         let key = rx.await;
///         match key {
///             KeyEvent::ScanCode(0x01) => break, // ESC 键退出
///             _ => render_game(key),
///         }
///     }
/// }
/// // 在另一个异步任务中
/// async fn input_poller(tx: ChannelSender<KeyEvent>) {
///     loop {
///         if let Some(key) = poll_uefi_key() {
///             tx.send(key);
///         }
///         Yield.await; // 给主逻辑运行的机会
///     }
/// }
/// ```
#[derive(Clone, Debug)]
pub struct ChannelSender<Data> (Rc<RefCell<VecDeque<Data>>>);
#[derive(Debug)]
pub struct ChannelReceiver<Data> (Rc<RefCell<VecDeque<Data>>>);
impl<Data> ChannelSender<Data> {
    /// 发送数据到通道
    pub fn send(&self, value: Data) { self.0.borrow_mut().push_back(value); }
}
impl<Data> Future for ChannelReceiver<Data> {
    type Output = Data;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.borrow_mut().pop_front() {
            Some(value) => Poll::Ready(value),
            None => Poll::Pending,
        }
    }
}
/// 创建一个不限容量的异步通道 (Unbounded Channel)
pub fn unbounded_channel<Data>() -> (ChannelSender<Data>, ChannelReceiver<Data>) {
    let queue = Rc::new(RefCell::new(VecDeque::new()));
    (ChannelSender(queue.clone()), ChannelReceiver(queue))
}


/// Single-core, multi-asynchronous
///
/// Usage:
/// ```rust,no_run
/// // 1. 创建一个容量为 32 的按键通道
/// let (tx, mut rx) = bounded_channel::<Key>(32);
///
/// add!(
///     // 任务 A: 生产者 (采样频率高，比如 100Hz)
///     executor => {
///         100 -> async move {
///             loop {
///                 if let Some(key) = poll_keyboard() {
///                     tx.send(key); // 发送按键到通道
///                 }
///                 Yield.await;
///             }
///         },
///
///         // 任务 B: 消费者 (根据游戏逻辑处理按键)
///         executor => {
///             0 -> async move {
///                 loop {
///                     // await 会在队列为空时自动挂起当前任务
///                     // 当上面的生产者 send 后，下一轮 poll 就能拿到数据
///                     let key = (&mut rx).await;
///                     process_key(key);
///                 }
///             }
///         }
///     }
/// );
/// ```
#[derive(Debug)]
struct ChannelInner<Data> (VecDeque<Data>);
#[derive(Clone, Debug)]
pub struct UnsafeChannelSender<Data> (*mut ChannelInner<Data>);
#[derive(Debug)]
pub struct UnsafeChannelReceiver<Data> (Box<ChannelInner<Data>>);
impl<Data> UnsafeChannelSender<Data> {
    pub fn send(&self, value: Data) {
        // 安全说明：在单线程 Executor 下，Receiver 保证 inner 指针有效
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
pub fn bounded_channel<Data>(capacity: usize) -> (UnsafeChannelSender<Data>, UnsafeChannelReceiver<Data>) {
    let mut inner = Box::new(ChannelInner(VecDeque::with_capacity(capacity)));
    let sender_ptr = &mut *inner as *mut ChannelInner<Data>;

    (UnsafeChannelSender(sender_ptr), UnsafeChannelReceiver(inner))
}
