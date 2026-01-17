//! This code is inspired by the approach in this embedded Rust crate: embassy-executor.
//!
//! Usage:
//! ```rust, no-run
//! #[doc(hidden)]
//! fn __async_fun() -> impl Future<Output = ()> { ( move || async move {})() }
//! fn async_fun() {
//!     const POOL_SIZE: usize = 4;
//!     static POOL: TaskPoolLayout<{ TaskCapture::<_, _>::size::<POOL_SIZE>(__async_fun) }> = unsafe {
//!         transmute(TaskCapture::<_,_>::new::<POOL_SIZE>(__async_fun))
//!     };
//!     const fn get<F, Args, Fut>(_: F) -> &'static TaskPool<Fut, POOL_SIZE>
//!     where F: TaskFn<Args, Fut = Fut>, Fut: SafeFuture {
//!         const {
//!             assert_eq!(size_of::<TaskPool<Fut, POOL_SIZE>>(), size_of_val(&POOL));
//!             assert!(align_of::<TaskPool<Fut, POOL_SIZE>>() <= 128);
//!         }
//!         unsafe { &*POOL.get().cast() }
//!     }
//!     get(__async_fun);
//! }
//! ```
use crate::no_alloc::future::{State, StaticFuture};
use core::any::type_name;
use core::cell::UnsafeCell;
use core::fmt::{Debug, Formatter, Result};
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use core::task::{Poll, Waker};
use static_cell::StaticCell;

pub trait SafeFuture: Future<Output = ()> + 'static + Send + Sync {}
impl<T: Future<Output = ()> + 'static + Send + Sync> SafeFuture for T {}
pub type TaskTypeFn = unsafe fn(*mut (), &Waker) -> Poll<()>;

#[derive(Debug)] #[repr(C)]
pub struct TaskHeader {
    // pub poll_handle: TaskTypeFn,
    pub control: AtomicU8,
    pub state: AtomicU8,
}
#[repr(C, align(128))]
pub struct TaskSlot<F: SafeFuture> {
    pub header: TaskHeader,
    // pub future: StaticCell<spin::Mutex<F>>,
    pub future: StaticFuture<StaticCell<F>>,
}
unsafe impl<F: SafeFuture> Sync for TaskSlot<F> {}
unsafe impl<F: SafeFuture> Send for TaskSlot<F> {}
impl<F: SafeFuture> TaskSlot<F> {
    pub const NEW: Self = Self::new();
    const fn new() -> Self {
        Self {
            header: TaskHeader {
                // poll_handle: Self::poll_wrapper,       // magic: automatically binding to SafeFuture
                control: AtomicU8::new(0),
                state: AtomicU8::new(State::Free as u8),
            },
            future: StaticFuture(AtomicUsize::new(0), StaticCell::new()),
        }
    }
}
impl<F: SafeFuture> Debug for TaskSlot<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let addr = format_args!("StaticCell<{}>@{:p}", type_name::<F>(), &self.future);
        f.debug_struct("TaskSlot")
            .field("header", &self.header)
            .field("future", &addr)
            .finish()
    }
}

#[repr(C, align(128))]
pub struct TaskPool<F: SafeFuture, const N: usize> (pub [TaskSlot<F>; N]);
impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    #[inline(always)] pub const fn new() -> Self { Self([TaskSlot::NEW; N]) }
}
impl<F: SafeFuture, const N: usize> Debug for TaskPool<F, N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("TaskPool")
            .field("size", &N)
            .field("task_type", &type_name::<F>())
            .field("slots", &self.0)
            .finish()
    }
}

#[repr(C, align(128))]
pub struct TaskPoolLayout<const SIZE: usize> (pub UnsafeCell<MaybeUninit<[u8; SIZE]>>);
unsafe impl<const SIZE: usize> Send for TaskPoolLayout<SIZE> {}
unsafe impl<const SIZE: usize> Sync for TaskPoolLayout<SIZE> {}
impl<const SIZE: usize> TaskPoolLayout<SIZE> {
    #[inline(always)] pub const fn get(&self) -> *const u8 { self.0.get().cast() }
}

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