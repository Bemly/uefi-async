use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use uefi::boot::stall;

/// Reads the current value of the hardware cycle counter (timestamp).
///
/// This function provides a high-resolution time source by accessing
/// architecture-specific registers:
/// * **x86 / x86_64**: Uses the `RDTSC` (Read Time Stamp Counter) instruction.
/// * **AArch64**: Uses the `CNTVCT_EL0` (Virtual Count Register) system register.
///
/// # Safety
/// While technically wrapping `unsafe` architecture instructions, this is
/// generally safe on modern processors. However, note that the frequency
/// of these counters may vary on older systems with power-saving features
/// (non-invariant TSC).
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

/// Estimates the hardware clock frequency (ticks per second) using a blocking delay.
///
/// This function measures the number of ticks elapsed over a 100ms period
/// using the UEFI `stall` service and extrapolates the result to 1 second.
///
/// # Behavior
/// * This is a **blocking** operation that takes at least 100 milliseconds to complete.
/// * It is typically called once during the initialization of the Executor
///   to normalize task intervals.
///
/// # Returns
/// The estimated number of hardware ticks per second (Hz).
#[deprecated(since = "0.2.4", note = "Use `FREQ.hz()` instead")]
pub fn calc_freq_blocking() -> u64 {
    let start = tick();
    // Use the UEFI stall service (assumed provided by the environment)
    stall(Duration::from_millis(100));
    let end = tick();
    let ticks_per_100ms = end - start;

    // Scale 100ms up to 1000ms (1 second)
    ticks_per_100ms * 10
}

pub struct ClockFreq { hz: AtomicU64, ms: AtomicU64, us: AtomicU64, ns: AtomicU64, ps: AtomicU64 }
impl ClockFreq {
    #[inline(always)]
    pub fn hz(&self) -> u64 { self.hz.load(Ordering::Relaxed) }
    #[inline(always)]
    pub fn ms(&self) -> u64 { self.ms.load(Ordering::Relaxed) }
    #[inline(always)]
    pub fn us(&self) -> u64 { self.us.load(Ordering::Relaxed) }
    #[inline(always)]
    pub fn ns(&self) -> u64 { self.ns.load(Ordering::Relaxed) }
    #[inline(always)]
    pub fn ps(&self) -> u64 { self.ps.load(Ordering::Relaxed) }
}
pub static FREQ: ClockFreq = ClockFreq {
    hz: AtomicU64::new(0), ms: AtomicU64::new(0), us: AtomicU64::new(0),
    ns: AtomicU64::new(0), ps: AtomicU64::new(0),
};
/// MUST-USE when Executor Initiation
pub(crate) fn init_clock_freq() -> u64 {
    let start = tick();
    stall(Duration::from_millis(50)); // 采样 50ms
    let end = tick();

    let ticks_per_50ms = end - start;
    let hz = ticks_per_50ms * 20; // 换算为 1s 的频率

    // 预计算并存入静态变量
    FREQ.hz.store(hz, Ordering::Relaxed);
    FREQ.ms.store((hz / 1000).max(1), Ordering::Relaxed);
    FREQ.us.store((hz / 100_0000).max(1), Ordering::Relaxed);
    FREQ.ns.store((hz / 10_0000_0000).max(1), Ordering::Relaxed);
    // 皮秒 (ps) 只有在主频 > 1GHz 时才有意义，否则结果为 0
    FREQ.ps.store(hz / 1_0000_0000_0000, Ordering::Relaxed);

    hz
}
