use core::time::Duration;
use uefi::boot::stall;

#[inline(always)]
pub fn tick() -> u64 {
    #[cfg(target_arch = "x86")]
    unsafe { core::arch::x86::_rdtsc() }

    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::x86_64::_rdtsc() }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut ticks: u64;
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) ticks);
        ticks
    }
}

/// 校准系统每秒的时钟滴答数（Ticks per Second）。
///
/// # 阻塞提醒 (BLOCKING WARNING)
/// 该函数是一个**同步阻塞**操作，会通过 `stall`（通常是忙等待）停顿约 100 毫秒。
/// 在调用此函数期间，当前 CPU 核心将无法处理其他任务或中断（取决于 stall 的实现）。
/// 建议仅在系统初始化阶段（如内核启动时）调用一次。
pub fn calc_freq_blocking() -> u64 {
    let start = tick();
    stall(Duration::from_millis(100));
    let end = tick();
    let ticks_per_100ms = end - start;
    ticks_per_100ms * 10
}