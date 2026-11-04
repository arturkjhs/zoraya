//! PSCI calls (ARM DEN 0022): `SYSTEM_OFF` and `SYSTEM_RESET` (ADR-0159).

use core::arch::asm;

use zoraya_dtb::platform::{Psci, PsciMethod};

/// `SYSTEM_OFF`, PSCI 0.2 and later (SMC32 function ID).
const SYSTEM_OFF: u64 = 0x8400_0008;
/// `SYSTEM_RESET`, PSCI 0.2 and later (SMC32 function ID).
const SYSTEM_RESET: u64 = 0x8400_0009;

fn call(method: PsciMethod, function: u64) -> u64 {
    let mut x0 = function;
    match method {
        // SAFETY: a PSCI call through the conduit named by the device tree;
        // the firmware (here QEMU) handles it and preserves the registers
        // the SMC Calling Convention requires. x1..x17 may be clobbered.
        PsciMethod::Hvc => unsafe {
            asm!("hvc #0", inout("x0") x0, out("x1") _, out("x2") _, out("x3") _,
                 out("x4") _, out("x5") _, out("x6") _, out("x7") _, out("x8") _,
                 out("x9") _, out("x10") _, out("x11") _, out("x12") _, out("x13") _,
                 out("x14") _, out("x15") _, out("x16") _, out("x17") _, options(nostack))
        },
        // SAFETY: as above, through the secure monitor conduit.
        PsciMethod::Smc => unsafe {
            asm!("smc #0", inout("x0") x0, out("x1") _, out("x2") _, out("x3") _,
                 out("x4") _, out("x5") _, out("x6") _, out("x7") _, out("x8") _,
                 out("x9") _, out("x10") _, out("x11") _, out("x12") _, out("x13") _,
                 out("x14") _, out("x15") _, out("x16") _, out("x17") _, options(nostack))
        },
    }
    x0
}

/// Powers the machine off. Returns only if the call failed, with the PSCI
/// error code, or at once if the device tree's PSCI has no `SYSTEM_OFF`.
pub fn system_off(psci: &Psci) -> i32 {
    if !psci.has_system_off {
        return -1; // NOT_SUPPORTED
    }
    call(psci.method, SYSTEM_OFF) as i32
}

/// Resets the machine. Returns only if the call failed, with the PSCI error
/// code, or at once if the device tree's PSCI has no `SYSTEM_RESET`.
pub fn system_reset(psci: &Psci) -> i32 {
    if !psci.has_system_reset {
        return -1; // NOT_SUPPORTED
    }
    call(psci.method, SYSTEM_RESET) as i32
}
