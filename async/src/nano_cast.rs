use core::hint::spin_loop;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use crate::util::tick;

const NOOP_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_| RawWaker::new(core::ptr::null(), &NOOP_WAKER_VTABLE), // clone
    |_| {}, // wake
    |_| {}, // wake_by_ref
    |_| {}, // drop
);

fn noop_waker() -> Waker {
    let raw = RawWaker::new(core::ptr::null(), &NOOP_WAKER_VTABLE);
    unsafe { Waker::from_raw(raw) }
}

// --- 3. 异步任务容器 ---
struct AsyncTask<'bemly_> {
    future: Pin<&'bemly_ mut (dyn Future<Output = ()> + 'bemly_)>,
    interval_cycles: u64, // 执行间隔 (TSC单位)
    next_run_tsc: u64,    // 下次执行的目标时间点
}

pub struct Executor<'bemly_, const N: usize> {
    tasks: [Option<AsyncTask<'bemly_>>; N],
}

impl<'bemly_, const N: usize> Executor<'bemly_, N> {
    pub fn new() -> Self {
        Self { tasks: [const { None }; N] }
    }

    pub fn add_task(&mut self, future: Pin<&'bemly_ mut (dyn Future<Output = ()> + 'bemly_)>, hz: u64, cpu_speed_hz: u64) {
        let interval = cpu_speed_hz / hz;
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                *slot = Some(AsyncTask {
                    future,
                    interval_cycles: interval,
                    next_run_tsc: tick(),
                });
                break;
            }
        }
    }

    pub fn run_forever(&mut self) {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            let current_tsc = tick();

            for task_opt in self.tasks.iter_mut() {
                if let Some(task) = task_opt {
                    // 检查 TSC 时间戳，实现频率控制
                    if current_tsc >= task.next_run_tsc {
                        match task.future.as_mut().poll(&mut cx) {
                            Poll::Ready(_) => {
                                // 任务完成后移除或重置
                                *task_opt = None;
                            }
                            Poll::Pending => {
                                // 更新下次运行时间点
                                task.next_run_tsc = current_tsc + task.interval_cycles;
                            }
                        }
                    }
                }
            }
            // 这里可以添加核心绘图逻辑，或者简单的 cpu_relax
            spin_loop();
        }
    }
}