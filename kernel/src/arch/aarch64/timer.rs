//! The EL1 virtual timer (`CNTV_*`, "The Generic Timer in AArch64 state").

use core::arch::asm;

const CTL_ENABLE: u64 = 1 << 0;

/// `CNTFRQ_EL0`: the counter's frequency in hertz.
pub fn frequency() -> u64 {
    let v: u64;
    // SAFETY: reading CNTFRQ_EL0 has no side effects.
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// `CNTVCT_EL0`. The `isb` keeps the read from being taken before earlier
/// instructions finish ("Reading the generic timer counters"), so two calls
/// in order read in order.
pub fn counter() -> u64 {
    let v: u64;
    // SAFETY: reading CNTVCT_EL0 has no side effects, and EL1 may read it:
    // CNTKCTL_EL1 traps only EL0, and CNTHCTL_EL2 is set up by boot.rs.
    unsafe {
        asm!("isb", "mrs {}, cntvct_el0", out(reg) v, options(nomem, nostack, preserves_flags))
    };
    v
}

/// Asserts the timer interrupt from counter value `ticks` on.
pub fn fire_at(ticks: u64) {
    // SAFETY: the virtual timer belongs to this kernel; the writes take
    // effect by the `isb`.
    unsafe {
        asm!(
            "msr cntv_cval_el0, {cval}",
            "msr cntv_ctl_el0, {ctl}",
            "isb",
            cval = in(reg) ticks,
            ctl = in(reg) CTL_ENABLE,
            options(nomem, nostack, preserves_flags),
        )
    };
}

/// Stops the timer and drops its interrupt.
pub fn disable() {
    // SAFETY: as in `fire_at`.
    unsafe {
        asm!(
            "msr cntv_ctl_el0, xzr",
            "isb",
            options(nomem, nostack, preserves_flags)
        )
    };
}

/// Whether the timer is on.
#[cfg(feature = "qemu-test")]
pub fn is_enabled() -> bool {
    let v: u64;
    // SAFETY: reading CNTV_CTL_EL0 has no side effects.
    unsafe { asm!("mrs {}, cntv_ctl_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v & CTL_ENABLE != 0
}
