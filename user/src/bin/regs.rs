#![no_std]
#![no_main]

//! Registers, FP state and the TLS register must survive preemption.

use core::arch::asm;

use zoraya_user::clock_monotonic;

const RUN_NS: u64 = 50_000_000;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mine = clock_monotonic() | 1;
    let end = clock_monotonic() + RUN_NS;
    // SAFETY: only this thread reads the register it sets.
    unsafe { asm!("msr tpidr_el0, {}", in(reg) mine) };
    while clock_monotonic() < end {
        let (tls, fp, gp): (u64, u64, u64);
        // SAFETY: only the named registers are used; the loop takes long
        // enough for interrupts to land in it.
        unsafe {
            asm!(
                "fmov d31, {v}",
                "mov {g}, {v}",
                "mov {n}, #2000",
                "2:",
                "subs {n}, {n}, #1",
                "b.ne 2b",
                "fmov {f}, d31",
                "mrs {t}, tpidr_el0",
                v = in(reg) mine,
                n = out(reg) _,
                f = out(reg) fp,
                g = out(reg) gp,
                t = out(reg) tls,
                out("v31") _,
            )
        };
        if tls != mine {
            return 1;
        }
        if fp != mine {
            return 2;
        }
        if gp != mine {
            return 3;
        }
    }
    0
}
