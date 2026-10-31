#![no_std]
#![no_main]

//! System calls preserve everything but x0 and x1 (ADR-0032).

use core::arch::global_asm;

use zoraya_user as _;

global_asm!(
    r#"
    .text
    // check(number: x0) -> x0: 0, or the first register that changed.
check_registers:
    stp     x19, x20, [sp, #-96]!
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x26, [sp, #48]
    stp     x27, x28, [sp, #64]
    stp     x29, x30, [sp, #80]
    mov     x8, x0
    .irp    n, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30
    mov     x\n, #(\n * 257)
    .endr
    svc     #0
    .irp    n, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30
    mov     x1, #(\n * 257)
    mov     x0, #\n
    cmp     x\n, x1
    b.ne    1f
    .endr
    mov     x0, #0
1:
    ldp     x29, x30, [sp, #80]
    ldp     x27, x28, [sp, #64]
    ldp     x25, x26, [sp, #48]
    ldp     x23, x24, [sp, #32]
    ldp     x21, x22, [sp, #16]
    ldp     x19, x20, [sp], #96
    ret
"#
);

unsafe extern "C" {
    safe fn check_registers(number: u64) -> u64;
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    for number in [1, 3, 1000] {
        for _ in 0..500 {
            let changed = check_registers(number);
            if changed != 0 {
                return changed as i32;
            }
        }
    }
    0
}
