// use crate::bss::lifo::{Queue, Stealer, Worker};
// use crate::bss::task::{SafeFuture, TaskHeader, TaskPool, TaskSlot, SLOT_EMPTY, SLOT_OCCUPIED};
// use crate::bss::waker::WakePolicy;
// use core::pin::Pin;
// use core::ptr::write;
// use core::sync::atomic::Ordering;
// use core::task::{Context, Poll, Waker};
//
// pub struct Executor<const N: usize> {
//     /// 本地核心的 Worker
//     pub worker: Worker<N>,
//     /// 其他核心的 Stealers
//     pub stealers: &'static [Stealer<N>],
// }
//
// impl<const N: usize> Executor<N> {
//     pub const fn new(worker: Worker<N>, stealers: &'static [Stealer<N>]) -> Self {
//         Self { worker, stealers }
//     }
//
//     pub fn run_step(&self, waker: &Waker) -> bool {
//         // 1. & 2. 获取任务指针 (优先本地 LIFO，其次跨核窃取 FIFO)
//         let task_ptr = self.worker.pop().or_else(|| {
//             self.stealers.iter().find_map(|s| {
//                 // steal_and_pop 直接返回一个可用的任务，同时将其余任务转移到本地
//                 s.steal_and_pop(&self.worker, |n| (n + 1) / 2).ok().map(|(ptr, _)| ptr)
//             })
//         });
//
//         // 3. 执行任务
//         if let Some(ptr) = task_ptr {
//             unsafe {
//                 // 此时 ptr 指向 TaskSlot<F> 的开头，即 TaskHeader 的开头
//                 let header = &*(ptr as *const TaskHeader);
//
//                 // 直接调用 poll_handle
//                 // 内部逻辑（WakePolicy 判定、类型还原、Poll 执行）全部封装在 poll_wrapper 中
//                 let _ = (header.poll_handle)(ptr, waker);
//             }
//             return true;
//         }
//         false
//     }
//
//     pub fn spawn<F: SafeFuture>(&self, f: F) {
//     }
// }
//
// impl<F: SafeFuture> TaskSlot<F> {
//     /// 翻译被抹掉的类型
//     pub fn poll_wrapper(ptr: *mut (), waker: &Waker) -> Poll<()> {
//         unsafe {
//             let slot = &*(ptr as *const Self);
//             let futrue = slot.future.uninit();
//
//             // 1. 原子获取并清除唤醒位
//             let prev_val = slot.header.control.fetch_and(!WakePolicy::WAKE_BIT, Ordering::Acquire);
//
//             // 2. 使用 WakePolicy 内部逻辑解包并判定
//             let (policy, is_waked) = WakePolicy::unpack(prev_val);
//
//             if !policy.should_poll(is_waked) {
//                 // InterruptOnly 且未被唤醒的情况
//                 return Poll::Pending;
//             }
//
//             // 3. 执行真正的 Future 推进
//             let future_mut = &mut *(*slot.future.get()).as_mut_ptr();
//             let res = Pin::new_unchecked(&mut *future_mut).poll(&mut Context::from_waker(waker));
//
//             if res.is_ready() {
//                 core::ptr::drop_in_place(future_mut);
//                 slot.header.occupied.store(SLOT_EMPTY, Ordering::Release);
//             }
//
//             res
//         }
//     }
//
// }
//
// pub const MAX_CORES: usize = 4;
// pub const QUEUE_SIZE: usize = 256;
//
// // 1. 全局原始队列池
// pub static GLOBAL_QUEUES: [Queue<QUEUE_SIZE>; MAX_CORES] = [
//     Queue::new(), Queue::new(), Queue::new(), Queue::new()
// ];
//
// // 2. 全局 Stealer 矩阵 (排除自己)
// static STEALER_POOL: [[Stealer<QUEUE_SIZE>; MAX_CORES - 1]; MAX_CORES] = {
//     let mut pool = [[Stealer { queue: &GLOBAL_QUEUES[0] }; MAX_CORES - 1]; MAX_CORES];
//     let mut i = 0;
//     while i < MAX_CORES {
//         let mut j = 0;
//         let mut target = 0;
//         while target < MAX_CORES {
//             if i != target {
//                 pool[i][j] = Stealer { queue: &GLOBAL_QUEUES[target] };
//                 j += 1;
//             }
//             target += 1;
//         }
//         i += 1;
//     }
//     pool
// };
//
// /// 每个 CPU 核心启动时调用的初始化函数
// pub fn init_executor(core_id: usize) -> Executor<QUEUE_SIZE> {
//     let worker = Worker::new(&GLOBAL_QUEUES[core_id]);
//     let stealers = &STEALER_POOL[core_id];
//     Executor::new(worker, stealers)
// }
//
// /// 将任务指针重新调度到任意可用的全局队列中
// /// 由 Waker 调用
// pub fn schedule_task(ptr: *mut ()) {
//     // 简单策略：遍历所有核心的队列，尝试推入
//     // 实际生产中可能优先推入当前核心或随机选择
//     for q in GLOBAL_QUEUES.iter() {
//         let worker = Worker::new(q);
//         if worker.push(ptr).is_ok() {
//             return;
//         }
//     }
//     // 如果所有队列都满了，这是一个严重问题（系统过载）
//     // 在这个简易实现中，我们只能丢弃这次唤醒（可能会导致任务饿死），或者在这里自旋等待
// }
//
// impl<const N: usize> Worker<N> {
//     /// 从指定的 TaskPool 中分配槽位并推入队列
//     pub fn spawn_task<F, const POOL_N: usize>(
//         &self,
//         pool: &'static TaskPool<F, POOL_N>,
//         fut: F
//     ) -> Result<(), F>
//     where F: Future<Output = ()> + 'static + Send + Sync
//     {
//         // 1. 寻找空闲槽位
//         for slot in pool.0.iter() {
//             // 尝试锁定槽位
//             if slot.header.occupied.compare_exchange(
//                 SLOT_EMPTY, SLOT_OCCUPIED, Ordering::Acquire, Ordering::Relaxed
//             ).is_ok() {
//                 unsafe {
//                     // 2. 初始化 Future 内容
//                     let future_ptr = (*slot.future.get()).as_mut_ptr();
//                     write(future_ptr, fut);
//
//                     // 3. 将槽位地址推入 LIFO 队列
//                     // 如果队列满了，我们需要退还槽位所有权
//                     let ptr = slot as *const TaskSlot<F> as *mut ();
//                     if let Err(_) = self.push(ptr) {
//                         let recovered_fut = core::ptr::read(future_ptr); // 拿回所有权
//                         slot.header.occupied.store(SLOT_EMPTY, Ordering::Release);
//                         return Err(recovered_fut);
//                     }
//                 }
//                 return Ok(());
//             }
//         }
//         Err(fut)
//     }
// }

use core::sync::atomic::Ordering;
use core::task::{Context, Poll};
use crate::no_alloc::future::State;
use crate::no_alloc::lifo::Worker;
use crate::no_alloc::task::TaskHeader;

pub const MAX_CORES: usize = 4;
pub const QUEUE_SIZE: usize = 256;

// 1. 全局原始队列池
pub static GLOBAL_QUEUES: [Queue<QUEUE_SIZE>; MAX_CORES] = [
    Queue::new(), Queue::new(), Queue::new(), Queue::new()
];

pub struct Executor<const N: usize> {
    pub worker: Worker<N>,
}

impl<const N: usize> Executor<N> {
    pub fn execute(&self, task_ptr: *mut ()) {
        let header = unsafe { &*(task_ptr as *const TaskHeader) };

        // 构造 Waker
        let waker = self.create_waker(task_ptr);
        let mut cx = Context::from_waker(&waker);

        // 更新状态为 Running
        header.state.store(State::Running.into(), Ordering::Release);

        unsafe {
            match (header.poll_handle)(task_ptr, &waker) {
                Poll::Ready(_) => {
                    header.state.store(State::Free.into(), Ordering::Release);
                }
                Poll::Pending => {
                    // 如果任务 yield 了，状态设为 Yielded
                    header.state.store(State::Yielded.into(), Ordering::Release);
                }
            }
        }
    }

    fn create_waker(&self, task_ptr: *mut ()) -> Waker {
        // 实现 RawWakerVTable...
        // wake() 的逻辑是：self.worker.push(task_ptr)
    }
}