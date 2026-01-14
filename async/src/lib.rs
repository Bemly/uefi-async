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
pub trait TaskFn<Args>: Copy {
    type Fut: Future<Output = ()> + 'static;
}

pub struct TaskPool<F: Future + 'static, const N: usize> {
    pool: [TaskSlot<F>; N],
}

macro_rules! task_fn_impl {
    // 递归基准：处理 0 个参数的情况，并停止递归
    () => {
        impl<F, Fut> TaskFn<()> for F
        where
            F: Copy + FnOnce() -> Fut,
            Fut: Future<Output = ()> + 'static,
        {
            type Fut = Fut;
        }
    };

    // 递归分支：处理当前参数列表，并递归处理“少一个参数”的情况
    ($head:ident $(, $tail:ident)*) => {
        // 1. 为当前的参数列表生成一个 impl
        // 这里的 $($tail),* 表示除了第一个以外的剩余部分
        impl<F, Fut, $head, $($tail,)*> TaskFn<($head, $($tail,)*)> for F
        where
            F: Copy + FnOnce($head, $($tail,)*) -> Fut,
            Fut: Future<Output = ()> + 'static,
        {
            type Fut = Fut;
        }

        // 2. 递归调用自己，去掉最前面的 $head
        task_fn_impl!($($tail),*);
    };
}

// 现在只需要这一行，就会自动生成 17 个 impl (0 到 16)
task_fn_impl!(T15, T14, T13, T12, T11, T10, T9, T8, T7, T6, T5, T4, T3, T2, T1, T0);

pub const fn task_pool_size<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where F: TaskFn<Args, Fut = Fut>, Fut: Future + 'static,
{ size_of::<TaskPool<Fut, POOL_SIZE>>() }

pub const fn task_pool_align<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where F: TaskFn<Args, Fut = Fut>, Fut: Future + 'static,
{ align_of::<TaskPool<Fut, POOL_SIZE>>() }

// pub const fn task_pool_new<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE>
// where
//     F: TaskFn<Args, Fut = Fut>,
//     Fut: Future + 'static,
// {
//     TaskPool::new()
// }

#[repr(transparent)]
pub struct Align<const N: usize>([<Self as Alignment>::Archetype; 0])
where Self: Alignment;

pub trait Alignment {
    /// A zero-sized type of particular alignment.
    type Archetype: Copy + Eq + PartialEq + Send + Sync + Unpin;
}

#[repr(C)]
pub struct TaskPoolHolder<const SIZE: usize, const ALIGN: usize>
where
    Align<ALIGN>: Alignment,
{
    data: UnsafeCell<[MaybeUninit<u8>; SIZE]>,
    align: Align<ALIGN>,
}

unsafe impl<const SIZE: usize, const ALIGN: usize> Send for TaskPoolHolder<SIZE, ALIGN>
where Align<ALIGN>: Alignment {}
unsafe impl<const SIZE: usize, const ALIGN: usize> Sync for TaskPoolHolder<SIZE, ALIGN>
where Align<ALIGN>: Alignment {}

impl<const SIZE: usize, const ALIGN: usize> TaskPoolHolder<SIZE, ALIGN>
where
    Align<ALIGN>: Alignment,
{
    pub const fn get(&self) -> *const u8 {
        self.data.get().cast()
    }
}




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
    pub const fn new() -> Self {
        Self {
            future: UnsafeCell::new(MaybeUninit::uninit()),
            occupied: AtomicBool::new(false),
            task_id: 0,
        }
    }
}
