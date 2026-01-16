//! This code is inspired by the approach in this embedded Rust crate:\
//! [https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs](https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs)

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicU8;
use core::task::{Poll, Waker};

pub trait SafeFuture: Future<Output = ()> + 'static + Send + Sync {}
impl<T: Future<Output = ()> + 'static + Send + Sync> SafeFuture for T {}

pub const SLOT_EMPTY: u8 = 0;
pub const SLOT_OCCUPIED: u8 = 1;
pub type TaskTypeFn = unsafe fn(*mut (), &Waker) -> Poll<()>;

#[derive(Debug)]
#[repr(C)]
pub struct TaskHeader {
    pub poll_handle: TaskTypeFn,
    pub control: AtomicU8,
    pub occupied: AtomicU8,
}

#[derive(Debug)]
#[repr(C, align(128))]
pub struct TaskSlot<F: SafeFuture> {
    pub header: TaskHeader,
    pub future: UnsafeCell<MaybeUninit<F>>,
}
unsafe impl<F: SafeFuture> Sync for TaskSlot<F> {}
unsafe impl<F: SafeFuture> Send for TaskSlot<F> {}
impl<F: SafeFuture> TaskSlot<F> {
    const NEW: Self = Self::new();
    pub const fn new() -> Self {
        Self {
            header: TaskHeader {
                poll_handle: Self::poll_wrapper,        // 魔法：自动绑定了当前的 F
                control: AtomicU8::new(0),
                occupied: AtomicU8::new(SLOT_EMPTY),    // 初始化为空闲状态 0
            },
            future: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

#[derive(Debug)]
#[repr(C, align(128))]
pub struct TaskPool<F: SafeFuture, const N: usize> (pub [TaskSlot<F>; N]);
impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    pub const fn new() -> Self { Self([TaskSlot::NEW; N]) }
}

// UB
#[derive(Debug)]
#[repr(C, align(128))]
pub struct TaskPoolLayout<const SIZE: usize> (pub UnsafeCell<MaybeUninit<[u8; SIZE]>>);
impl<const SIZE: usize> TaskPoolLayout<SIZE> {
    pub const fn new() -> Self { Self(UnsafeCell::new(MaybeUninit::uninit())) }
}
unsafe impl<const SIZE: usize> Send for TaskPoolLayout<SIZE> {}
unsafe impl<const SIZE: usize> Sync for TaskPoolLayout<SIZE> {}

pub trait TaskFn<Args>: Copy { type Fut: SafeFuture; }
macro_rules! task_fn_impl {
    () => {
        impl<F, Fut> TaskFn<()> for F where F: Copy + FnOnce() -> Fut, Fut: SafeFuture,
        { type Fut = Fut; }
    };
    ($head:ident $(, $tail:ident)*) => {
        impl<F, Fut, $head, $($tail,)*> TaskFn<($head, $($tail,)*)> for F
        where F: Copy + FnOnce($head, $($tail,)*) -> Fut, Fut: SafeFuture,
        { type Fut = Fut; }
        task_fn_impl!($($tail),*);
    };
}
task_fn_impl!(T15, T14, T13, T12, T11, T10, T9, T8, T7, T6, T5, T4, T3, T2, T1, T0);
pub struct TaskCapture<F, Args>(PhantomData<(F, Args)>);
impl<F, Args, Fut> TaskCapture<F, Args> where F: TaskFn<Args, Fut = Fut>, Fut: SafeFuture {
    #[inline(always)]
    pub const fn size<const POOL_SIZE: usize>(_: F) -> usize { size_of::<TaskPool<Fut, POOL_SIZE>>() }
    #[inline(always)]
    pub const fn new<const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE> { TaskPool::new() }
}