#![no_std]
#![no_main]

//! Every register, the FP state and the TLS register must survive preemption.

use core::arch::{asm, global_asm};

use zoraya_user::clock_monotonic;

const RUN_NS: u64 = 50_000_000;

global_asm!(
    r#"
    .text
    // check(pattern: x0) -> x0: 0, or a nonzero code. Runs with no system call.
check_preserved:
    stp     x19, x20, [sp, #-112]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x26, [sp, #48]
    stp     x27, x28, [sp, #64]
    stp     d8, d9, [sp, #80]
    stp     d10, d11, [sp, #96]
    sub     sp, sp, #32
    stp     d12, d13, [sp]
    stp     d14, d15, [sp, #16]
    mvn     x9, x0
    .irp    n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    mov     v\n\().d[0], x0
    mov     v\n\().d[1], x9
    .endr
    .irp    n, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28
    mov     x\n, x0
    .endr
    mov     x10, #(1 << 22)         // FPCR: round towards plus infinity
    msr     fpcr, x10
    mov     x11, #0x20000
1:
    subs    x11, x11, #1
    b.ne    1b
    mrs     x10, fpcr
    msr     fpcr, xzr
    mov     x11, #(1 << 22)
    mov     x12, #1
    cmp     x10, x11
    b.ne    3f
    .irp    n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
    mov     x12, #(\n + 100)
    umov    x10, v\n\().d[0]
    cmp     x10, x0
    b.ne    3f
    umov    x10, v\n\().d[1]
    cmp     x10, x9
    b.ne    3f
    .endr
    .irp    n, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28
    mov     x12, #\n
    cmp     x\n, x0
    b.ne    3f
    .endr
    mov     x12, #0
3:
    mov     x0, x12
    ldp     d12, d13, [sp]
    ldp     d14, d15, [sp, #16]
    add     sp, sp, #32
    ldp     d10, d11, [sp, #96]
    ldp     d8, d9, [sp, #80]
    ldp     x27, x28, [sp, #64]
    ldp     x25, x26, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #112
    ret
"#
);

unsafe extern "C" {
    safe fn check_preserved(pattern: u64) -> u64;
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let mine = clock_monotonic() | 1;
    let end = clock_monotonic() + RUN_NS;
    // SAFETY: only this thread reads the register it sets.
    unsafe { asm!("msr tpidr_el0, {}", in(reg) mine) };
    while clock_monotonic() < end {
        let changed = check_preserved(mine);
        if changed != 0 {
            return changed as i32;
        }
        let tls: u64;
        // SAFETY: reading a register.
        unsafe { asm!("mrs {}, tpidr_el0", out(reg) tls) };
        if tls != mine {
            return 1000;
        }
    }
    0
}
