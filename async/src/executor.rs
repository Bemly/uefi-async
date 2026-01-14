use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicBool;
use static_cell::StaticCell;
use uefi::println;

pub struct Executor<const N: usize> {
    core_id: usize,
    // 你的 st3 Worker
    worker: Worker<*const TaskHeader, N>,
    // 其他核心的 Stealer 集合，用于偷取
    stealers: &'static [Stealer<*const TaskHeader, N>],
}

impl<const N: usize> Executor<N> {
    pub fn run_loop(&self) -> ! {
        loop {
            // 1. 尝试从本地弹出一个任务
            if let Some(task_ptr) = self.worker.pop() {
                self.run_task(task_ptr);
            }
            // 2. 本地没任务，去偷
            else if let Some(task_ptr) = self.try_steal() {
                self.run_task(task_ptr);
            }
            // 3. 彻底没活干，进入低功耗
            else {
                core::hint::spin_loop(); // UEFI 下可以用 CpuPause
            }
        }
    }
}


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
where
    F: TaskFn<Args, Fut = Fut>,
    Fut: Future + 'static,
{
    size_of::<TaskPool<Fut, POOL_SIZE>>()
}

pub const fn task_pool_align<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> usize
where
    F: TaskFn<Args, Fut = Fut>,
    Fut: Future + 'static,
{
    align_of::<TaskPool<Fut, POOL_SIZE>>()
}

pub const fn task_pool_new<F, Args, Fut, const POOL_SIZE: usize>(_: F) -> TaskPool<Fut, POOL_SIZE>
where
    F: TaskFn<Args, Fut = Fut>,
    Fut: Future + 'static,
{
    TaskPool::new()
}

#[repr(transparent)]
pub struct Align<const N: usize>([<Self as Alignment>::Archetype; 0])
where
    Self: Alignment;

trait Alignment {
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

unsafe impl<const SIZE: usize, const ALIGN: usize> Send for TaskPoolHolder<SIZE, ALIGN> where Align<ALIGN>: Alignment {}
unsafe impl<const SIZE: usize, const ALIGN: usize> Sync for TaskPoolHolder<SIZE, ALIGN> where Align<ALIGN>: Alignment {}

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
