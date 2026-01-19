use core::sync::atomic::Ordering;
use core::task::{RawWaker, RawWakerVTable, Waker};
use crate::no_alloc::task::TaskHeader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WakePolicy {
    PollOnly = 0b00,        // 纯轮询：执行器每一轮都尝试 Poll 它
    InterruptOnly = 0b01,   // 纯中断：只有被唤醒后才 Poll
    Hybrid = 0b10,          // 混合：执行器轮询，但中断也可以加速它
}
impl WakePolicy {
    pub const WAKE_BIT: u8 = 0b1000_0000; // Bit 7
    const POLICY_MASK: u8 = 0b0000_0011; // Bit 0-1

    /// 从 control 字节中解包出策略和唤醒状态
    #[inline(always)]
    pub fn unpack(control: u8) -> (Self, bool) {
        let policy = match control & Self::POLICY_MASK {
            0b00 => Self::PollOnly,
            0b01 => Self::InterruptOnly,
            0b10 => Self::Hybrid,
            _ => unreachable!("Invalid WakePolicy"),
        };
        let is_waked = (control & Self::WAKE_BIT) != 0;
        (policy, is_waked)
    }

    /// 判定在该策略和唤醒状态下，是否应该执行真正的 Poll
    #[inline(always)]
    pub fn should_poll(self, is_waked: bool) -> bool {
        match self {
            Self::PollOnly => true,
            Self::Hybrid => true,
            Self::InterruptOnly => is_waked,
        }
    }
}



// Waker 的虚函数表
static VTABLE: RawWakerVTable = RawWakerVTable::new(
    waker_clone,
    waker_wake,
    waker_wake_by_ref,
    waker_drop,
);

unsafe fn waker_clone(ptr: *const ()) -> RawWaker {
    RawWaker::new(ptr, &VTABLE)
}

// 核心逻辑：唤醒任务
unsafe fn waker_wake(ptr: *const ()) {
    let task_ptr = ptr as *mut TaskHeader;
    let header = &*task_ptr;

    // 1. 更新状态：从 Waiting/Running 变为 Ready
    // 只有状态切换成功，才允许重新入队，防止重复入队
    header.state.store(State::Ready.into(), Ordering::Release);

    // 2. 重新入队
    // 在 UEFI 多核中，最简单的做法是根据 core_id 找到对应的全局 Worker
    // 假设你在 TaskHeader 里存了初始化的 core_id
    let core_id = header.control.load(Ordering::Relaxed) as usize;
    let worker = Worker::new(&GLOBAL_QUEUES[core_id]);

    let _ = worker.push(task_ptr);
}

unsafe fn waker_wake_by_ref(ptr: *const ()) {
    waker_wake(ptr);
}

unsafe fn waker_drop(_ptr: *const ()) {
    // 静态内存任务不需要手动释放内存
}