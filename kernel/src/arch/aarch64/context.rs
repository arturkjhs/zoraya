//! Context switch (ADR-0030).
//!
//! A switch happens at a call boundary in kernel code, so only what the
//! calling convention preserves is saved here, plus `FPCR`, `FPSR`, the user
//! TLS register and the stack limit. A user thread's registers are in the
//! exception frame on its stack.

use core::arch::global_asm;
use core::mem::{offset_of, size_of};

/// Saved state of a thread that is not running.
#[repr(C)]
#[derive(Debug, Default)]
pub struct Context {
    x19_x28: [u64; 10],
    x29: u64,
    x30: u64,
    sp: u64,
    d8_d15: [u64; 8],
    fpcr: u64,
    fpsr: u64,
    tpidr_el0: u64,
    /// Stack limit for the vectors, in `SP_EL0` while the kernel runs.
    limit: u64,
}

const _: () = assert!(offset_of!(Context, sp) == 96 && offset_of!(Context, fpcr) == 168);
const _: () = assert!(size_of::<Context>() == 200);

global_asm!(
    r#"
    .text
    // __context_switch(prev: x0, next: x1)
    .global __context_switch
__context_switch:
    stp     x19, x20, [x0, #0]
    stp     x21, x22, [x0, #16]
    stp     x23, x24, [x0, #32]
    stp     x25, x26, [x0, #48]
    stp     x27, x28, [x0, #64]
    stp     x29, x30, [x0, #80]
    mov     x9, sp
    str     x9, [x0, #96]
    stp     d8, d9, [x0, #104]
    stp     d10, d11, [x0, #120]
    stp     d12, d13, [x0, #136]
    stp     d14, d15, [x0, #152]
    mrs     x9, fpcr
    mrs     x10, fpsr
    stp     x9, x10, [x0, #168]
    mrs     x9, tpidr_el0
    mrs     x10, sp_el0
    stp     x9, x10, [x0, #184]

    ldp     x19, x20, [x1, #0]
    ldp     x21, x22, [x1, #16]
    ldp     x23, x24, [x1, #32]
    ldp     x25, x26, [x1, #48]
    ldp     x27, x28, [x1, #64]
    ldp     x29, x30, [x1, #80]
    ldr     x9, [x1, #96]
    mov     sp, x9
    ldp     d8, d9, [x1, #104]
    ldp     d10, d11, [x1, #120]
    ldp     d12, d13, [x1, #136]
    ldp     d14, d15, [x1, #152]
    ldp     x9, x10, [x1, #168]
    msr     fpcr, x9
    msr     fpsr, x10
    ldp     x9, x10, [x1, #184]
    msr     tpidr_el0, x9
    msr     sp_el0, x10
    ret

    // First switch to a kernel thread: x19 is the entry, x20 its argument.
__thread_start:
    mov     x0, x19
    mov     x1, x20
    bl      kernel_thread_main
"#
);

unsafe extern "C" {
    fn __context_switch(prev: *mut Context, next: *const Context);
    fn __thread_start();
    fn __exception_return();
}

impl Context {
    /// A kernel thread that will call `kernel_thread_main(entry, arg)` on the
    /// stack ending at `top`, with stack limit `limit`.
    pub fn new_kernel(entry: usize, arg: usize, top: u64, limit: u64) -> Context {
        let mut c = Context {
            x30: __thread_start as *const () as u64,
            sp: top,
            limit,
            ..Context::default()
        };
        c.x19_x28[0] = entry as u64;
        c.x19_x28[1] = arg as u64;
        c
    }

    /// A user thread whose first return to EL0 uses the exception frame at
    /// `frame`.
    pub fn new_user(frame: u64, limit: u64) -> Context {
        Context {
            x30: __exception_return as *const () as u64,
            sp: frame,
            limit,
            ..Context::default()
        }
    }
}

/// Saves the running thread in `prev` and continues `next`.
///
/// # Safety
///
/// `prev` and `next` stay valid until the switch back, `next` holds a context
/// made by this module or saved here, on a stack that is mapped, and every
/// exception is masked or cannot reschedule.
pub unsafe fn switch(prev: *mut Context, next: *const Context) {
    // SAFETY: the caller's contract; the routine follows the C convention.
    unsafe { __context_switch(prev, next) }
}
