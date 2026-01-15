use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll};
use core::time::Duration;
use uefi::boot::stall;

static TSC_PER_MS: AtomicU64 = AtomicU64::new(2000000); // 默认假设 2GHz

pub fn calibrate_tsc() {
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    stall(Duration::from_millis(10)); // UEFI 同步等待 10ms
    let end = unsafe { core::arch::x86_64::_rdtsc() };
    TSC_PER_MS.store((end - start) / 10, Ordering::Release);
}

#[inline(always)]
pub fn get_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

pub struct Sleep {
    target_tsc: u64,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if get_tsc() >= self.target_tsc {
            Poll::Ready(())
        } else {
            // 重点：在 UEFI 简单异步模型中，如果没有硬件定时器中断唤醒，
            // 我们需要让 Waker 把自己重新放回队列，否则任务会“睡死”过去。
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub fn sleep_ms(ms: u64) -> Sleep {
    let ticks = ms * TSC_PER_MS.load(Ordering::Relaxed);
    Sleep { target_tsc: get_tsc() + ticks }
}