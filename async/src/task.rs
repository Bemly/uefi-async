//! This code is inspired by the approach in this embedded Rust crate:\
//! [https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs](https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs)

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicU8;
use core::task::{Poll, Waker};
// use crate::waker::WakePolicy;

#[cfg(feature = "safe")]
fn safe_challenge() {}

pub const SLOT_EMPTY: u8 = 0;
pub const SLOT_OCCUPIED: u8 = 1;
pub type TaskTypeFn = unsafe fn(*mut (), &Waker) -> Poll<()>;

#[repr(C)]
pub struct TaskHeader {
    // 1. 公共头：偷取者最先看这里
    pub poll_handle: TaskTypeFn,
    // 2. 状态控制：建议放在最前面，与 header 共享第一个 Cache Line
    pub control: AtomicU8,
    pub occupied: AtomicU8,
}

#[repr(C, align(128))]
pub struct TaskSlot<F: Future<Output = ()> + 'static + Send + Sync> {
    pub header: TaskHeader,
    pub future: UnsafeCell<MaybeUninit<F>>,
}
impl<F: Future<Output = ()> + 'static + Send + Sync> TaskSlot<F> {
    const NEW: Self = Self::new();
    pub const fn new() -> Self {
        #[cfg(feature = "safe")]
        safe_challenge();

        Self {
            header: TaskHeader {
                poll_handle: Self::poll_wrapper,        // 魔法：自动绑定了当前的 F
                control: AtomicU8::new(0),
                occupied: AtomicU8::new(SLOT_EMPTY),    // 初始化为空闲状态 0
            },
            future: UnsafeCell::new(MaybeUninit::zeroed()),
        }
    }
}
unsafe impl<F: Future<Output = ()> + 'static + Send + Sync> Sync for TaskSlot<F> {}
unsafe impl<F: Future<Output = ()> + 'static + Send + Sync> Send for TaskSlot<F> {}

#[repr(C, align(128))]
pub struct TaskPool<F: Future<Output = ()> + 'static + Send + Sync, const N: usize> (pub [TaskSlot<F>; N]);
impl<F: Future<Output = ()> + 'static + Send + Sync, const N: usize> TaskPool<F, N> {
    pub const fn new() -> Self { Self([TaskSlot::<F>::NEW; N]) }
}

#[repr(C, align(128))]
pub struct TaskPoolLayout<const SIZE: usize, const ALIGN: usize> (pub UnsafeCell<[MaybeUninit<u8>; SIZE]>);
unsafe impl<const SIZE: usize, const ALIGN: usize> Send for TaskPoolLayout<SIZE, ALIGN> {}
unsafe impl<const SIZE: usize, const ALIGN: usize> Sync for TaskPoolLayout<SIZE, ALIGN> {}

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