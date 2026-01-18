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
use core::any::type_name;
use core::cell::UnsafeCell;
use core::fmt::{Debug, Formatter, Result};
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::ptr::{drop_in_place, from_mut, from_ref, with_exposed_provenance, with_exposed_provenance_mut, write};
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::{Poll, Waker};
use num_enum::{FromPrimitive, IntoPrimitive};
use static_cell::StaticCell;

pub trait SafeFuture: Future<Output = ()> + 'static + Send + Sync {}
impl<T: Future<Output = ()> + 'static + Send + Sync> SafeFuture for T {}
pub trait TaskFn<Args>: Copy { type Fut: SafeFuture; }
pub type TaskTypeFn = unsafe fn(*mut (), &Waker) -> Poll<()>;
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, FromPrimitive)] #[repr(u8)]
pub enum State {
    Free,           // 空槽
    Initialized,    // 槽使用
    Ready,          // 进入任务队列
    Running,        // 运行
    Yielded,        // 让出
    Waiting,        // 等待

    #[default]
    Unreachable,    // 不可达
}
pub struct TaskCapture<F, Args>(PhantomData<(F, Args)>);
#[derive(Debug)] #[repr(C)]
pub struct TaskHeader {
    pub poll: TaskTypeFn,
    pub control: AtomicU8,
    pub state: AtomicU8,
}
#[derive(Debug)] #[repr(C)]
pub struct StaticFuture<F>(pub UnsafeCell<usize>, pub F);
#[repr(C, align(128))]
pub struct TaskSlot<F: SafeFuture> {
    pub header: TaskHeader,
    pub future: StaticFuture<StaticCell<F>>,
}
#[repr(C, align(128))]
pub struct TaskPool<F: SafeFuture, const N: usize> (pub [TaskSlot<F>; N]);
#[repr(C, align(128))]
pub struct TaskPoolLayout<const SIZE: usize> (pub UnsafeCell<MaybeUninit<[u8; SIZE]>>);

impl<F: SafeFuture> TaskSlot<F> {
    pub const NEW: Self = Self::new();
    const fn new() -> Self {
        Self {
            header: TaskHeader {
                poll: Self::poll,        // magic: automatically binding to SafeFuture
                control: AtomicU8::new(0),
                state: AtomicU8::new(State::Free as u8),
            },
            future: StaticFuture::new(),                // 占位
        }
    }
    fn poll(&self) {

    }
}
impl<F: SafeFuture> StaticFuture<StaticCell<F>> {
    #[inline(always)]
    pub const fn new() -> Self { Self(UnsafeCell::new(0), StaticCell::new()) }
    /// Lazy Initialization
    /// SAFETY: data race!
    #[inline(always)]
    pub unsafe fn init(&'static self, future: impl FnOnce() -> F) {
        let future_ptr = self.0.get();
        let future_addr = unsafe { future_ptr.read() };
        if future_addr == 0 {
            // init_with return value maybe cause stack overflow
            let uninit_ptr = self.1.uninit();
            let new_ptr = uninit_ptr.as_mut_ptr();
            unsafe {
                write(new_ptr, future());
                future_ptr.write(from_mut(uninit_ptr).addr());
            }
        } else {
            let cell = with_exposed_provenance_mut(future_addr);
            unsafe {
                drop_in_place(cell);
                write(cell, future());
            }
        }
    }
}
impl<F: SafeFuture> Deref for StaticFuture<StaticCell<F>> {
    type Target = F;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe {
            let addr = self.0.get().read();
            debug_assert_ne!(addr, 0, "Future is not initialized");
            with_exposed_provenance::<F>(addr).as_ref().unwrap_unchecked()
        }
    }
}
impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    #[inline(always)]
    pub const fn new() -> Self { Self([TaskSlot::NEW; N]) }
    #[inline(always)]
    pub fn init(&'static self, future: impl FnOnce() -> F) -> *mut TaskHeader {
        for slot in self.0.iter() {
            if slot.header.state.compare_exchange(
                State::Free.into(), State::Initialized.into(), Ordering::Acquire, Ordering::Relaxed
            ).is_err() { continue }

            // Only init Future => TaskSlot, not run F
            // impl NRVO (Named Return Value Optimization), avoid stack overflow
            unsafe { slot.future.init(future) }

            return from_ref(&slot.header).cast_mut()
        }

        panic!("TaskPool capacity exceeded! No empty slots available.");
    }
}
impl<const SIZE: usize> TaskPoolLayout<SIZE> {
    #[inline(always)]
    pub const fn get(&self) -> *const u8 { self.0.get().cast() }
}
impl<F, Args, Fut> TaskCapture<F, Args> where F: TaskFn<Args, Fut = Fut>, Fut: SafeFuture {
    #[inline(always)]
    pub const fn size<const POOL_SIZE: usize>(_: F) -> usize { size_of::<TaskPool<Fut, POOL_SIZE>>() }
    #[inline(always)]
    pub const fn new<const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE> { TaskPool::new() }
}

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
impl<F: SafeFuture> Debug for TaskSlot<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let addr = format_args!("StaticCell<{}>@{:p}", type_name::<F>(), &self.future);
        f.debug_struct("TaskSlot")
            .field("header", &self.header)
            .field("future", &addr)
            .finish()
    }
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
unsafe impl<F: SafeFuture> Sync for TaskSlot<F> {}
unsafe impl<F: SafeFuture> Send for TaskSlot<F> {}
unsafe impl<const SIZE: usize> Send for TaskPoolLayout<SIZE> {}
unsafe impl<const SIZE: usize> Sync for TaskPoolLayout<SIZE> {}
