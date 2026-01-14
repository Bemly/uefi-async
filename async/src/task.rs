//! This code is inspired by the approach in this embedded Rust crate:\
//! [https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs](https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs)

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::{Context, Poll, Waker};

#[cfg(feature = "safe")]
fn safe_challenge() {}

pub const SLOT_EMPTY: u8 = 0;
pub const SLOT_OCCUPIED: u8 = 1;
pub type TaskTypeFn = fn(*mut (), &Waker) -> Poll<()>;

#[repr(C, align(128))]
pub struct TaskSlot<F: Future<Output = ()>> {
    // 1. 公共头：偷取者最先看这里
    pub header: TaskTypeFn,

    // 2. 状态控制：建议放在最前面，与 header 共享第一个 Cache Line
    pub occupied: AtomicU8,
    pub task_id: usize,

    // 3. 执行上下文：用于构建 Waker
    pub context_ptr: *const (),

    // 4. 数据区：可能比较大，单独占后面的空间
    pub future: UnsafeCell<MaybeUninit<F>>,
}
impl<F: Future<Output = ()> + 'static + Send + Sync> TaskSlot<F> {
    const NEW: Self = Self::new(); // 黑魔法：绕过rust Copy Clone传染判定
    pub const fn new() -> Self {
        #[cfg(feature = "safe")]
        safe_challenge();

        Self {
            header: Self::poll_wrapper, // 魔法：自动绑定了当前的 F
            occupied: AtomicU8::new(SLOT_EMPTY), // 初始化为空闲状态 0
            task_id: 0,
            context_ptr: core::ptr::null(),
            future: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// 翻译被抹掉的类型
    fn poll_wrapper(ptr: *mut (), waker: &Waker) -> Poll<()>
    where F: Future<Output = ()> {
        unsafe {
            // 1. 强转回具体的 TaskSlot 类型
            let slot = &*(ptr as *const TaskSlot<F>);

            // 2. 获取 Future 的可变指针
            let future_mut = &mut *(*slot.future.get()).as_mut_ptr();

            // 3. 构造 Context 并执行 Poll
            let mut cx = Context::from_waker(waker);

            let res = Pin::new_unchecked(&mut *future_mut).poll(&mut cx);

            // 如果任务完成了，必须原地销毁状态机
            if res.is_ready() {
                core::ptr::drop_in_place(future_mut);
                // 别忘了把标志位设为 0，否则这个槽位永远无法再次 spawn
                slot.occupied.store(SLOT_EMPTY, Ordering::Release);
            }
            res
        }
    }
}
unsafe impl<F: Future<Output = ()> + 'static> Sync for TaskSlot<F> {}
unsafe impl<F: Future<Output = ()> + 'static> Send for TaskSlot<F> {}

#[repr(C, align(128))]
pub struct TaskPool<F: Future<Output = ()> + 'static + Send + Sync, const N: usize> (pub [TaskSlot<F>; N]);
impl<F: Future<Output = ()> + 'static + Send + Sync, const N: usize> TaskPool<F, N> {
    pub const fn new() -> Self { Self([TaskSlot::<F>::NEW; N]) }
}

#[repr(C, align(128))]
pub struct TaskPoolLayout<const SIZE: usize, const ALIGN: usize> (pub [u8; SIZE]);
impl<const SIZE: usize, const ALIGN: usize> TaskPoolLayout<SIZE, ALIGN> {
    pub const fn as_ptr(&self) -> *const u8 { self.0.as_ptr() }
}

pub trait TaskFn<Args>: Copy { type Fut: Future<Output = ()> + 'static + Send + Sync; }
macro_rules! task_fn_impl {
    () => {
        impl<F, Fut> $crate::TaskFn<()> for F
        where
            F: ::core::marker::Copy + ::core::ops::FnOnce() -> Fut,
            Fut: ::core::future::Future<Output = ()> + 'static + ::core::marker::Send + ::core::marker::Sync,
        { type Fut = Fut; }
    };

    ($head:ident $(, $tail:ident)*) => {
        impl<F, Fut, $head, $($tail,)*> $crate::TaskFn<($head, $($tail,)*)> for F
        where
            F: ::core::marker::Copy + ::core::ops::FnOnce($head, $($tail,)*) -> Fut,
            Fut: ::core::future::Future<Output = ()> + 'static + ::core::marker::Send + ::core::marker::Sync,
        { type Fut = Fut; }

        task_fn_impl!($($tail),*);
    };
}
task_fn_impl!(T15, T14, T13, T12, T11, T10, T9, T8, T7, T6, T5, T4, T3, T2, T1, T0);
pub const fn task_pool_size<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where F: TaskFn<Args, Fut = Fut>, Fut: Future<Output = ()> + 'static + Send + Sync,
{ size_of::<TaskPool<Fut, POOL_SIZE>>() }

pub const fn task_pool_align<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where F: TaskFn<Args, Fut = Fut>, Fut: Future<Output = ()> + 'static + Send + Sync,
{ align_of::<TaskPool<Fut, POOL_SIZE>>() }

pub const fn task_pool_new<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE>
where F: TaskFn<Args, Fut = Fut>, Fut: Future<Output = ()> + 'static + Send + Sync,
{ TaskPool::new() }