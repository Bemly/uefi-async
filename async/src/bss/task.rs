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
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use core::task::{Poll, Waker};
use static_cell::StaticCell;

pub trait SafeFuture: Future<Output = ()> + 'static + Send + Sync {}
impl<T: Future<Output = ()> + 'static + Send + Sync> SafeFuture for T {}

pub const SLOT_EMPTY: u8 = 0;
pub const SLOT_OCCUPIED: u8 = 1;
pub type TaskTypeFn = unsafe fn(*mut (), &Waker) -> Poll<()>;

#[derive(Debug)] #[repr(C)]
pub struct TaskHeader {
    // pub poll_handle: TaskTypeFn,
    pub control: AtomicU8,
    pub occupied: AtomicU8,
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
                control: AtomicU8::new(0),          // TODO
                occupied: AtomicU8::new(SLOT_EMPTY),   // init
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

// TODO: 用 MaybeUninit
#[derive(Debug)] #[repr(C)]
struct StaticFuture<F>(pub AtomicUsize, pub F);
impl<F: SafeFuture> StaticFuture<StaticCell<F>> {
    #[inline(always)] pub fn spawn(&'static self, future: fn() -> F) -> &'static mut F {
        match self.0.load(Ordering::Acquire) {
            0 => {
                let cell = self.1.uninit();
                self.0.store(cell as *const _ as usize, Ordering::Release);
                cell.write(future())
            }
            _ => {
                // let addr = self.0.load(Ordering::Acquire) as *const F;

            }
        }
    } // Lazy Initialization
}
impl<F: SafeFuture> Deref for StaticFuture<StaticCell<F>> {
    type Target = F;
    #[inline(always)] fn deref(&self) -> &Self::Target {
        let addr = self.0.load(Ordering::Acquire) as *const F;
        if addr.is_null() { panic!("Attempted to deref an uninitialized StaticFuture!") }
        unsafe { &*addr }
    }
}
impl<F: SafeFuture> DerefMut for StaticFuture<StaticCell<F>> {
    #[inline(always)] fn deref_mut(&mut self) -> &mut Self::Target {
        let addr = self.0.load(Ordering::Acquire) as *mut F;
        if addr.is_null() { panic!("Attempted to deref mut an uninitialized StaticFuture!") }
        unsafe { &mut *addr }
    }
}

#[repr(C, align(128))]
pub struct TaskPool<F: SafeFuture, const N: usize> (pub [TaskSlot<F>; N]);
impl<F: SafeFuture, const N: usize> TaskPool<F, N> {
    #[inline(always)] pub const fn new() -> Self { Self([TaskSlot::NEW; N]) }
    #[inline(always)] pub fn spawn(&'static self, f: fn() -> F) {
        for slot in self.0.iter() {
            if slot.header.occupied.compare_exchange(
                SLOT_EMPTY, SLOT_OCCUPIED, Ordering::Acquire, Ordering::Relaxed
            ).is_ok() {
                slot.future.spawn(f);
                // match slot.future.try_uninit() {
                //     None => unimplemented!("task pool is not empty"), // TODO: 支持async重入
                //     Some(cell) => cell.write(f()),
                // };
                // slot.future.try_init_with(f);
            }
        }
    }
    #[inline(always)] pub const fn is_running() {

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