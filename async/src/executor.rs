use crate::st3::lifo::{Queue, Stealer, Worker};
use crate::task::{TaskHeader, TaskTypeFn, SLOT_EMPTY, SLOT_OCCUPIED};
use core::mem::transmute;
use core::pin::Pin;
use core::ptr;
use core::sync::atomic::Ordering;
use core::task::{Context, Poll, Waker};
use crate::{TaskPool, TaskSlot};
use crate::waker::WakePolicy;

// #[cfg(feature = "safe")]
// use core::sync::atomic::AtomicUsize;
// #[cfg(feature = "safe")]
// static POINTER_COOKIE: AtomicUsize = AtomicUsize::new(0x5A5A_B6B6_C3C3_8D8D);
// #[cfg(feature = "safe")]
// pub fn init_executor_security(seed: usize) {
//     POINTER_COOKIE.store(seed ^ 0xDEADBEEF, core::sync::atomic::Ordering::Relaxed);
// }
#[cfg(feature = "safe")]
fn safe_challenge() {
    // 严重降低效率
    // 检查指针是否 128 字节对齐（如果不满足，说明指针肯定坏了）
    if (ptr as usize) & 127 != 0 { panic!("Corrupted pointer!"); }

    // 2. 检查 Magic Number
    let magic = unsafe { *(ptr as *const usize) };
    if magic != 0x5441534B { panic!("Not a valid TaskSlot!"); }
    let mangled_fn_ptr = *(ptr as *const usize);
    let original_fn_ptr = mangled_fn_ptr ^ POINTER_COOKIE.load(core::sync::atomic::Ordering::Relaxed);
    let original_fn_ptr = mangled_fn_ptr;
    let poll_fn: fn(*mut (), &Waker) -> Poll<()> = core::mem::transmute(original_fn_ptr);
    (poll_fn)(ptr, waker);
}
pub struct Executor<const N: usize> {
    /// 本地核心的 Worker
    worker: Worker<N>,
    /// 其他核心的 Stealers
    stealers: &'static [Stealer<N>],
}

impl<const N: usize> Executor<N> {
    pub const fn new(worker: Worker<N>, stealers: &'static [Stealer<N>]) -> Self {
        Self { worker, stealers }
    }

    pub fn run_step(&self, waker: &core::task::Waker) -> bool {
        // 1. & 2. 获取任务指针 (优先本地 LIFO，其次跨核窃取 FIFO)
        let task_ptr = self.worker.pop().or_else(|| {
            self.stealers.iter().find_map(|s| {
                // steal_and_pop 直接返回一个可用的任务，同时将其余任务转移到本地
                s.steal_and_pop(&self.worker, |n| (n + 1) / 2).ok().map(|(ptr, _)| ptr)
            })
        });

        // 3. 执行任务
        if let Some(ptr) = task_ptr {
            unsafe {
                // 此时 ptr 指向 TaskSlot<F> 的开头，即 TaskHeader 的开头
                let header = &*(ptr as *const TaskHeader);

                // 直接调用 poll_handle
                // 内部逻辑（WakePolicy 判定、类型还原、Poll 执行）全部封装在 poll_wrapper 中
                let _ = (header.poll_handle)(ptr, waker);
            }
            return true;
        }
        false
    }
}

impl<F: Future<Output = ()> + 'static + Send + Sync> TaskSlot<F> {
    /// 翻译被抹掉的类型
    pub fn poll_wrapper(ptr: *mut (), waker: &Waker) -> Poll<()> {
        unsafe {
            let slot = &*(ptr as *const TaskSlot<F>);

            // 1. 原子获取并清除唤醒位
            let prev_val = slot.header.control.fetch_and(!WakePolicy::WAKE_BIT, Ordering::Acquire);

            // 2. 使用 WakePolicy 内部逻辑解包并判定
            let (policy, is_waked) = WakePolicy::unpack(prev_val);

            if !policy.should_poll(is_waked) {
                // InterruptOnly 且未被唤醒的情况
                return Poll::Pending;
            }

            // 3. 执行真正的 Future 推进
            let future_mut = &mut *(*slot.future.get()).as_mut_ptr();
            let res = Pin::new_unchecked(&mut *future_mut).poll(&mut Context::from_waker(waker));

            if res.is_ready() {
                ptr::drop_in_place(future_mut);
                slot.header.occupied.store(SLOT_EMPTY, Ordering::Release);
            }

            res
        }
    }

}

pub const MAX_CORES: usize = 4;
pub const QUEUE_SIZE: usize = 256;

// 1. 全局原始队列池
pub static GLOBAL_QUEUES: [Queue<QUEUE_SIZE>; MAX_CORES] = [
    Queue::new(), Queue::new(), Queue::new(), Queue::new()
];

// 2. 全局 Stealer 矩阵 (排除自己)
static STEALER_POOL: [[Stealer<QUEUE_SIZE>; MAX_CORES - 1]; MAX_CORES] = {
    let mut pool = [[Stealer { queue: &GLOBAL_QUEUES[0] }; MAX_CORES - 1]; MAX_CORES];
    let mut i = 0;
    while i < MAX_CORES {
        let mut j = 0;
        let mut target = 0;
        while target < MAX_CORES {
            if i != target {
                pool[i][j] = Stealer { queue: &GLOBAL_QUEUES[target] };
                j += 1;
            }
            target += 1;
        }
        i += 1;
    }
    pool
};

/// 每个 CPU 核心启动时调用的初始化函数
pub fn init_executor(core_id: usize) -> Executor<QUEUE_SIZE> {
    let worker = Worker::new(&GLOBAL_QUEUES[core_id]);
    let stealers = &STEALER_POOL[core_id];
    Executor::new(worker, stealers)
}

/// 将任务指针重新调度到任意可用的全局队列中
/// 由 Waker 调用
pub fn schedule_task(ptr: *mut ()) {
    // 简单策略：遍历所有核心的队列，尝试推入
    // 实际生产中可能优先推入当前核心或随机选择
    for q in GLOBAL_QUEUES.iter() {
        let worker = Worker::new(q);
        if worker.push(ptr).is_ok() {
            return;
        }
    }
    // 如果所有队列都满了，这是一个严重问题（系统过载）
    // 在这个简易实现中，我们只能丢弃这次唤醒（可能会导致任务饿死），或者在这里自旋等待
}

impl<const N: usize> Worker<N> {
    /// 从指定的 TaskPool 中分配槽位并推入队列
    pub fn spawn_task<F, const POOL_N: usize>(
        &self,
        pool: &'static TaskPool<F, POOL_N>,
        fut: F
    ) -> Result<(), F>
    where F: Future<Output = ()> + 'static + Send + Sync
    {
        // 1. 寻找空闲槽位
        for slot in pool.0.iter() {
            // 尝试锁定槽位
            if slot.header.occupied.compare_exchange(
                SLOT_EMPTY, SLOT_OCCUPIED, Ordering::Acquire, Ordering::Relaxed
            ).is_ok() {
                unsafe {
                    // 2. 初始化 Future 内容
                    let future_ptr = (*slot.future.get()).as_mut_ptr();
                    core::ptr::write(future_ptr, fut);

                    // 3. 将槽位地址推入 LIFO 队列
                    // 如果队列满了，我们需要退还槽位所有权
                    let ptr = slot as *const TaskSlot<F> as *mut ();
                    if let Err(_) = self.push(ptr) {
                        ptr::drop_in_place(future_ptr);
                        slot.header.occupied.store(SLOT_EMPTY, Ordering::Release);
                        // 这里比较遗憾，Future 已经被丢弃了，无法还给调用者原来的 F
                        // 但在嵌入式环境下，通常意味着设计容量不足
                        return Err(unsafe { core::mem::zeroed() });
                    }
                }
                return Ok(());
            }
        }
        Err(fut)
    }
}