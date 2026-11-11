//! Bounded kernel work (ADR-0270): a long call proceeds in steps of at most
//! [`STEP`] pages or objects and calls [`between`] after each, holding no
//! spinlock. The kernel stays non-preemptive (ADR-0030): a call yields only
//! at these points, as a blocking call does.
//!
//! The test kernel also keeps a counter of the longest time the CPU stayed
//! with IRQ masked (INV-0270-1), which `masked_max_ns` reads.

use zoraya_abi::Status;

use crate::sched;

/// Pages, page-table entries' worth of range or objects in one step
/// (ADR-0270 point 2, `[assumption]` until measured).
#[allow(dead_code)] // used from the next commit, which steps the long calls
pub const STEP: usize = 64;

/// The end of a step: lets a pending interrupt run, switches away if the
/// slice ended, and fails with `Cancelled` if the process was asked to end
/// meanwhile (`job_kill`), so that the call frees its partial work and
/// returns (ADR-0270 point 3). A caller that is not a user process (boot
/// code, kernel threads) is not in a system call and does nothing.
///
/// No spinlock may be held: the test kernel asserts it (INV-0270-2).
#[allow(dead_code)] // used from the next commit, which steps the long calls
pub fn between() -> Result<(), Status> {
    let thread = sched::current();
    let Some(process) = thread.process() else {
        return Ok(());
    };
    #[cfg(feature = "qemu-test")]
    assert_eq!(
        crate::sync::held(),
        0,
        "a spinlock is held across a step (ADR-0270 point 2)"
    );
    // SAFETY: only a user process's system call gets here, which runs after
    // the vectors and the interrupt controller are set up. Interrupts are
    // masked again below, before anything that expects them masked.
    unsafe { crate::arch::unmask_interrupts() };
    // SAFETY: `isb` only synchronises the instruction stream, so that an
    // interrupt that is pending is taken here and not later.
    unsafe { core::arch::asm!("isb", options(nomem, nostack, preserves_flags)) };
    crate::arch::mask_interrupts();
    mask_began();
    sched::preempt_point();
    if process.exit_requested() {
        return Err(Status::Cancelled);
    }
    Ok(())
}

/// Counts the longest time with IRQ masked, in the test kernel.
#[cfg(feature = "qemu-test")]
mod masked {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Counter ticks when the mask began, or 0 while unmasked.
    static SINCE: AtomicU64 = AtomicU64::new(0);
    static MAX_TICKS: AtomicU64 = AtomicU64::new(0);

    fn ticks() -> u64 {
        let t: u64;
        // SAFETY: reading the virtual counter has no side effects.
        unsafe {
            core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t,
                options(nomem, nostack, preserves_flags))
        };
        t.max(1)
    }

    pub fn begin() {
        if SINCE.load(Ordering::Relaxed) == 0 {
            SINCE.store(ticks(), Ordering::Relaxed);
        }
    }

    pub fn end() {
        let since = SINCE.swap(0, Ordering::Relaxed);
        if since != 0 {
            MAX_TICKS.fetch_max(ticks().saturating_sub(since), Ordering::Relaxed);
        }
    }

    pub fn max_ns(reset: bool) -> u64 {
        let freq: u64;
        // SAFETY: reading the counter frequency has no side effects.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq,
                options(nomem, nostack, preserves_flags))
        };
        let t = if reset {
            MAX_TICKS.swap(0, Ordering::Relaxed)
        } else {
            MAX_TICKS.load(Ordering::Relaxed)
        };
        (u128::from(t) * 1_000_000_000 / u128::from(freq.max(1))) as u64
    }
}

/// The mask began (the CPU took an exception or code masked IRQ).
#[inline(always)]
pub fn mask_began() {
    #[cfg(feature = "qemu-test")]
    masked::begin();
}

/// The mask ended: IRQ is on again.
#[inline(always)]
pub fn mask_ended() {
    #[cfg(feature = "qemu-test")]
    masked::end();
}

/// The longest time with IRQ masked since the last reset, in nanoseconds.
#[cfg(feature = "qemu-test")]
pub fn masked_max_ns(reset: bool) -> u64 {
    masked::max_ns(reset)
}
