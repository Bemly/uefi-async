use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use alloc::rc::Rc;
use alloc::sync::Arc;
use core::pin::Pin;
use core::task::{Context, Poll};
use crossbeam::queue::{ArrayQueue, SegQueue};
use spin::Mutex;

pub mod single {
    use super::*;

    pub mod oneshot {
        use super::*;

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
    }

    pub mod unbounded_channel {
        use super::*;

        /// Single-core, multi-asynchronous
        ///
        /// MPSC
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
        pub fn channel<Data>() -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Rc::new(RefCell::new(VecDeque::new()));
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }

    }

    pub mod bounded_channel {
        use super::*;
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
        pub fn channel<Data>(capacity: usize) -> (UnsafeChannelSender<Data>, UnsafeChannelReceiver<Data>) {
            let mut inner = Box::new(ChannelInner(VecDeque::with_capacity(capacity)));
            let sender_ptr = &mut *inner as *mut ChannelInner<Data>;

            (UnsafeChannelSender(sender_ptr), UnsafeChannelReceiver(inner))
        }

    }
}

pub mod multiple {
    use super::*;

    pub mod oneshot {
        use super::*;

        /// Multi-core, multi-asynchronous
        ///
        /// Usage:
        /// ```rust,no_run
        /// // 假设 Core 0 负责渲染，Core 1 负责物理
        /// let (tx, rx) = MultiCoreOneshot::new();
        ///
        /// // Core 1 的调度器中运行的任务
        /// async core_1() {
        ///     let result = heavy_physics_calculation();
        ///     tx.send(result);
        /// }
        ///
        /// // Core 0 的调度器中运行的任务
        /// async core_2() {
        ///     let data = rx.await; // 这里会 Pending 直到 Core 1 完成
        ///     update_gpu_buffer(data);
        /// }
        /// ```
        #[derive(Debug)]
        pub struct Oneshot<Data>(Arc<Mutex<Option<Data>>>);

        #[derive(Debug)]
        pub struct OneShotReceiver<Data>(Arc<Mutex<Option<Data>>>);

        impl<Data> Oneshot<Data> {
            /// 创建一对多核安全的 Oneshot 通道
            pub fn new() -> (Self, OneShotReceiver<Data>) {
                let data = Arc::new(Mutex::new(None));
                (Self(data.clone()), OneShotReceiver(data))
            }

            /// 发送数据。由于使用了 Arc 和 Mutex，这可以跨核调用。
            pub fn send(self, value: Data) {
                let mut lock = self.0.lock();
                *lock = Some(value);
            }
        }

        impl<Data> Future for OneShotReceiver<Data> {
            type Output = Data;

            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                // 尝试获取锁并提取数据
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

        /// Multi-core, multi-asynchronous
        ///
        /// Usage:
        /// ```rust,no_run
        /// // 示例：CPU 0 负责处理复杂的 GOP 渲染逻辑，计算完后把 UI 顶点数据发给 CPU 1 绘图
        /// let (tx, rx) = multi_core_unbounded_channel::<VertexData>();
        ///
        /// // 在 CPU 0 的 Executor 任务中
        /// executor0.add(async move {
        ///     let data = compute_ui();
        ///     tx.send(data); // 瞬间完成，不阻塞渲染
        /// });
        ///
        /// // 在 CPU 1 的 Executor 任务中
        /// executor1.add(async move {
        ///     loop {
        ///         let vertex = rx.await; // 如果没数据，CPU 1 会去跑其他任务，不会卡死
        ///         render_to_screen(vertex);
        ///     }
        /// });
        /// ```
        #[derive(Clone, Debug)]
        pub struct ChannelSender<Data>(Arc<SegQueue<Data>>);
        #[derive(Debug)]
        pub struct ChannelReceiver<Data>(Arc<SegQueue<Data>>);

        /// 创建一个多核安全的无锁异步通道
        pub fn channel<Data>()
            -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Arc::new(SegQueue::new());
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }

        impl<Data> ChannelSender<Data> {
            /// 极其高效的入队操作，仅需一次 Atomic CAS
            #[inline(always)]
            pub fn send(&self, value: Data) { self.0.push(value) }
        }

        impl<Data> Future for ChannelReceiver<Data> {
            type Output = Data;
            #[inline]
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                // SegQueue 的 pop 是线程安全的且无锁
                // 在多核调度器中，如果队列为空，我们返回 Pending 让出当前核的 CPU
                match self.0.pop() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }
    }

    pub mod bounded_channel {
        use super::*;

        /// Multicore, multi-asynchronous, bounded
        #[derive(Clone,Debug)]
        pub struct ChannelSender<Data> (Arc<ArrayQueue<Data>>);
        #[derive(Debug)]
        pub struct ChannelReceiver<Data> (Arc<ArrayQueue<Data>>);
        pub fn channel<Data>(cap: usize) -> (ChannelSender<Data>, ChannelReceiver<Data>) {
            let queue = Arc::new(ArrayQueue::new(cap));
            (ChannelSender(queue.clone()), ChannelReceiver(queue))
        }
        impl<Data> ChannelSender<Data> {
            /// Lock-free
            #[inline(always)]
            pub fn try_send(&self, value: Data) -> Result<(), Data> { self.0.push(value) }
        }

        impl<Data> Future for ChannelReceiver<Data> {
            type Output = Data;
            #[inline]
            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                // ArrayQueue 的 pop 使用原子操作 CAS 维护 head 指针
                // 如果一个核的任务在等待数据， 直接返回 Pending，Executor 去执行其他 Task
                match self.0.pop() {
                    Some(value) => Poll::Ready(value),
                    None => Poll::Pending,
                }
            }
        }
    }
}





