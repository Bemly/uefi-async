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
pub fn calc_freq_blocking() -> u64 {
    let start = tick();
    // Use the UEFI stall service (assumed provided by the environment)
    stall(Duration::from_millis(100));
    let end = tick();
    let ticks_per_100ms = end - start;

    // Scale 100ms up to 1000ms (1 second)
    ticks_per_100ms * 10
}