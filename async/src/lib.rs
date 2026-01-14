#![no_main]
#![no_std]

extern crate alloc;

mod st3;
mod executor;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicBool;
use core::time::Duration;
use uefi::boot::stall;
use uefi::prelude::*;
use uefi::proto::pi::mp::MpServices;
// use uefi_async_macros::ヽ;
// use uefi_async_macros::ヽ as Caillo;
// Ciallo～(∠・ω< )⌒☆

// #[ヽ('ε')]
// mod example_app {
//     async fn master_setup() {}
//     async fn agent_setup() {}
//     async fn agent_main() {}
//     fn agent_idle() {}
//     fn on_panic() {}
//     fn on_error() {}
//     fn on_exit() {}
// }

// This code is inspired by the approach in this embedded Rust crate:
// https://github.com/embassy-rs/embassy/blob/main/embassy-executor-macros/src/macros/task.rs
#[repr(C, align(128))]
pub struct TaskSlot<F: Future> {
    // 存放 Future 状态机
    future: UnsafeCell<MaybeUninit<F>>,
    // 标记该槽位是否已被占用（用于分配）
    occupied: AtomicBool,
    // 该任务的 Waker 需要的元数据（如全局索引）
    task_id: usize,
}
impl<F: Future + 'static + Send + Sync> TaskSlot<F> {
    // 黑魔法
    const NEW: Self = Self::new();

    const fn new() -> Self {
        Self {
            future: UnsafeCell::new(MaybeUninit::uninit()),
            occupied: AtomicBool::new(false),
            task_id: 0,
        }
    }
}
unsafe impl<F: Future + 'static> Sync for TaskSlot<F> {}
unsafe impl<F: Future + 'static> Send for TaskSlot<F> {}

#[repr(C, align(128))]
pub struct TaskPool<F: Future + 'static + Send + Sync, const N: usize> ([TaskSlot<F>; N]);
impl<F: Future + 'static + Send + Sync, const N: usize> TaskPool<F, N> {
    pub const fn new() -> Self { Self([TaskSlot::NEW; N]) }
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
where F: TaskFn<Args, Fut = Fut>, Fut: Future + 'static + Send + Sync,
{ size_of::<TaskPool<Fut, POOL_SIZE>>() }

pub const fn task_pool_align<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where F: TaskFn<Args, Fut = Fut>, Fut: Future + 'static + Send + Sync,
{ align_of::<TaskPool<Fut, POOL_SIZE>>() }

pub const fn task_pool_new<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE>
where F: TaskFn<Args, Fut = Fut>, Fut: Future + 'static + Send + Sync,
{ TaskPool::new() }