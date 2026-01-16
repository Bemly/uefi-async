use core::sync::atomic::Ordering;
use core::task::{RawWaker, RawWakerVTable, Waker};
use crate::bss::executor::schedule_task;
use crate::bss::task::{TaskHeader, TaskSlot};

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



// VTable 实现
unsafe fn clone_waker(ptr: *const ()) -> RawWaker {
    RawWaker::new(ptr, &VTABLE)
}

unsafe fn wake(ptr: *const ()) {
    let header = unsafe { &*(ptr as *const TaskHeader) };

    // 使用 fetch_or 设置第 7 位
    // Ordering::Release 确保中断前的所有数据写入对执行器可见
    let prev = header.control.fetch_or(WakePolicy::WAKE_BIT, Ordering::Release);

    // 核心逻辑：设置唤醒标志位
    // 使用 swap 确保原子性，并起到去重作用（Deduplication）
    // 如果之前已经是 1，说明任务已经在队列中或者正在运行且已被标记，无需重复推入队列
    if (prev & WakePolicy::WAKE_BIT) == 0 {
        // 只有当状态从 0 变为 1 时，才真正将任务推入调度队列
        schedule_task(ptr as *mut ());
    }
}

unsafe fn drop_waker(_ptr: *const ()) {
    // 这里的 ptr 指向静态分配的 TaskSlot，不需要我们回收内存
}
pub const VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_waker,
    wake,
    wake,
    drop_waker,
);

/// 从 TaskSlot 创建 Waker
pub fn from_task<F: Future<Output = ()> + 'static + Send + Sync> (slot: *const TaskSlot<F>) -> Waker {
    let raw = RawWaker::new(slot as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}