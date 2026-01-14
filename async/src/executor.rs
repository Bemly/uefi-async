use crate::st3::lifo::{Queue, Stealer, Worker};
use crate::task::TaskTypeFn;
use core::mem::transmute;
use core::task::Waker;

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
    worker: Worker<N>,
    stealers: &'static [Stealer<N>],
}

impl<const N: usize> Executor<N> {
    pub fn run_step(&self, waker: &Waker) -> bool {
        // 1. 尝试从本地队列获取任务
        let task_ptr = self.worker.pop().or_else(|| {
            // 2. 本地没有，尝试从其他核心偷取
            for stealer in self.stealers {
                if let Ok((ptr, _)) = stealer.steal_and_pop(
                    &self.worker,
                    |n| (n + 1) / 2 // 偷取一半
                ) { return Some(ptr); }
            }
            None
        });

        if let Some(ptr) = task_ptr {
            #[cfg(feature = "safe")]
            safe_challenge();
            unsafe {
                let poll_fn: TaskTypeFn = transmute(*(ptr as *const usize));
                let _ = poll_fn(ptr, waker);
            }
            return true;
        }
        false
    }
}







// 假设我们支持 4 个核心，每个核心队列大小为 256
pub const MAX_CORES: usize = 4;
pub const QUEUE_SIZE: usize = 256;
static GLOBAL_QUEUES: [Queue<QUEUE_SIZE>; MAX_CORES] = [
    Queue::new(), Queue::new(), Queue::new(), Queue::new()
];

pub fn init_executor(core_id: usize) -> Executor<QUEUE_SIZE> {
    assert!(core_id < MAX_CORES, "Core ID out of range");

    Executor {
        worker: Worker::new(&GLOBAL_QUEUES[core_id]),
        stealers: collect_static_stealers(core_id),
    }
}

// 1. 定义 Stealer 存储池：每个核心拥有一个能够存放 (MAX_CORES - 1) 个 Stealer 的数组
// 这里我们直接初始化，编译器会在编译时计算好偏移量
static STEALER_POOL: [[Stealer<QUEUE_SIZE>; MAX_CORES - 1]; MAX_CORES] = {
    let mut pool = [[Stealer { queue: &GLOBAL_QUEUES[0] }; MAX_CORES - 1]; MAX_CORES];

    let mut core_i = 0;
    while core_i < MAX_CORES {
        let mut stealer_j = 0;
        let mut target_core = 0;
        while target_core < MAX_CORES {
            if core_i != target_core {
                // 这里的 Stealer 只是持有静态 Queue 的引用，Copy 成本极低
                pool[core_i][stealer_j] = Stealer { queue: &GLOBAL_QUEUES[target_core] };
                stealer_j += 1;
            }
            target_core += 1;
        }
        core_i += 1;
    }
    pool
};

/// 核心函数：根据 core_id 映射到对应的静态切片
pub fn collect_static_stealers(core_id: usize) -> &'static [Stealer<QUEUE_SIZE>] {
    &STEALER_POOL[core_id]
}


