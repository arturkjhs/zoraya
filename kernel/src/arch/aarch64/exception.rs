//! Exception vectors and the kernel fault policy (ADR-0024).
//!
//! The kernel runs on `SP_EL1`. Each entry of the "current EL with SP_ELx"
//! group first compares SP with the limit kept in `SP_EL0`, without touching
//! memory, because the exception may be a stack overflow. If there is no room
//! for a frame, the entry moves to the emergency stack. `TPIDR_EL1` is a
//! scratch register for those few instructions only.
//!
//! The limit's bit 0 is set while on the emergency stack. An overflow of that
//! stack cannot be reported with no stack left, so it halts.
//!
//! Every entry saves a full [`Frame`], FP/SIMD included: the target lets the
//! compiler use NEON in any function, so a handler may clobber it.

use core::arch::global_asm;
use core::mem::{offset_of, size_of};

use crate::{print, println};

/// Saved state of the interrupted context, laid out for the assembly below.
#[repr(C)]
pub struct Frame {
    pub x: [u64; 31],
    pub sp: u64,
    pub elr: u64,
    pub spsr: u64,
    pub esr: u64,
    pub far: u64,
    pub fpcr: u64,
    pub fpsr: u64,
    pub q: [u128; 32],
}

pub const FRAME_SIZE: usize = size_of::<Frame>();
const _: () = assert!(FRAME_SIZE.is_multiple_of(16));
const _: () = assert!(offset_of!(Frame, sp) == 248 && offset_of!(Frame, elr) == 256);
const _: () = assert!(offset_of!(Frame, esr) == 272 && offset_of!(Frame, fpcr) == 288);
const _: () = assert!(offset_of!(Frame, q) == 304);

const KIND_OVERFLOW: u64 = 16;

global_asm!(
    r#"
    .macro  vector_current_spx n
    .balign 128
    msr     tpidr_el1, x0
    mrs     x0, sp_el0
    cmp     sp, x0
    mrs     x0, tpidr_el1
    b.lo    __stack_overflow
    sub     sp, sp, #{frame_size}
    stp     x0, x1, [sp]
    mov     x0, #\n
    b       __exception_save
    .endm

    .macro  vector_other n
    .balign 128
    sub     sp, sp, #{frame_size}
    stp     x0, x1, [sp]
    mov     x0, #\n
    b       __exception_save
    .endm

    .section .text.vectors, "ax"
    .balign 2048
    .global __exception_vectors
__exception_vectors:
    // Current EL with SP_EL0: 0 to 3. The kernel never runs on SP_EL0.
    .irp    n, 0, 1, 2, 3
    vector_other \n
    .endr
    // Current EL with SP_ELx: 4 to 7.
    .irp    n, 4, 5, 6, 7
    vector_current_spx \n
    .endr
    // Lower EL, AArch64 then AArch32: 8 to 15. Wired up in stage 2.
    .irp    n, 8, 9, 10, 11, 12, 13, 14, 15
    vector_other \n
    .endr

    // x0 holds its original value and nothing has been saved.
__stack_overflow:
    msr     tpidr_el1, x0
    mrs     x0, sp_el0
    tbnz    x0, #0, .Lsecond_overflow
    mov     x0, sp
    msr     sp_el0, x0
    adrp    x0, __emergency_stack_top
    add     x0, x0, :lo12:__emergency_stack_top
    mov     sp, x0
    sub     sp, sp, #{frame_size}
    mrs     x0, tpidr_el1
    stp     x0, x1, [sp]
    mrs     x1, sp_el0
    str     x1, [sp, #{off_sp}]
    adrp    x1, __emergency_stack_bottom
    add     x1, x1, :lo12:__emergency_stack_bottom
    add     x1, x1, #({frame_size} + 1)
    msr     sp_el0, x1
    mov     x0, #{kind_overflow}
    b       __exception_save_rest
.Lsecond_overflow:
    wfe
    b       .Lsecond_overflow

    // x0 is the vector number; the original x0 and x1 are in the frame.
__exception_save:
    add     x1, sp, #{frame_size}
    str     x1, [sp, #{off_sp}]
__exception_save_rest:
    stp     x2, x3, [sp, #16]
    stp     x4, x5, [sp, #32]
    stp     x6, x7, [sp, #48]
    stp     x8, x9, [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    stp     x18, x19, [sp, #144]
    stp     x20, x21, [sp, #160]
    stp     x22, x23, [sp, #176]
    stp     x24, x25, [sp, #192]
    stp     x26, x27, [sp, #208]
    stp     x28, x29, [sp, #224]
    str     x30, [sp, #240]
    mrs     x1, elr_el1
    mrs     x2, spsr_el1
    stp     x1, x2, [sp, #{off_elr}]
    mrs     x1, esr_el1
    mrs     x2, far_el1
    stp     x1, x2, [sp, #{off_esr}]
    mrs     x1, fpcr
    mrs     x2, fpsr
    stp     x1, x2, [sp, #{off_fpcr}]
    add     x1, sp, #{off_q}
    stp     q0, q1, [x1], #32
    stp     q2, q3, [x1], #32
    stp     q4, q5, [x1], #32
    stp     q6, q7, [x1], #32
    stp     q8, q9, [x1], #32
    stp     q10, q11, [x1], #32
    stp     q12, q13, [x1], #32
    stp     q14, q15, [x1], #32
    stp     q16, q17, [x1], #32
    stp     q18, q19, [x1], #32
    stp     q20, q21, [x1], #32
    stp     q22, q23, [x1], #32
    stp     q24, q25, [x1], #32
    stp     q26, q27, [x1], #32
    stp     q28, q29, [x1], #32
    stp     q30, q31, [x1]

    mov     x1, x0
    mov     x0, sp
    bl      exception_dispatch

    add     x1, sp, #{off_q}
    ldp     q0, q1, [x1], #32
    ldp     q2, q3, [x1], #32
    ldp     q4, q5, [x1], #32
    ldp     q6, q7, [x1], #32
    ldp     q8, q9, [x1], #32
    ldp     q10, q11, [x1], #32
    ldp     q12, q13, [x1], #32
    ldp     q14, q15, [x1], #32
    ldp     q16, q17, [x1], #32
    ldp     q18, q19, [x1], #32
    ldp     q20, q21, [x1], #32
    ldp     q22, q23, [x1], #32
    ldp     q24, q25, [x1], #32
    ldp     q26, q27, [x1], #32
    ldp     q28, q29, [x1], #32
    ldp     q30, q31, [x1]
    ldp     x1, x2, [sp, #{off_fpcr}]
    msr     fpcr, x1
    msr     fpsr, x2
    ldp     x1, x2, [sp, #{off_elr}]
    msr     elr_el1, x1
    msr     spsr_el1, x2
    ldp     x2, x3, [sp, #16]
    ldp     x4, x5, [sp, #32]
    ldp     x6, x7, [sp, #48]
    ldp     x8, x9, [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldp     x18, x19, [sp, #144]
    ldp     x20, x21, [sp, #160]
    ldp     x22, x23, [sp, #176]
    ldp     x24, x25, [sp, #192]
    ldp     x26, x27, [sp, #208]
    ldp     x28, x29, [sp, #224]
    ldr     x30, [sp, #240]
    ldp     x0, x1, [sp]
    add     sp, sp, #{frame_size}
    eret
"#,
    frame_size = const FRAME_SIZE,
    kind_overflow = const KIND_OVERFLOW,
    off_sp = const offset_of!(Frame, sp),
    off_elr = const offset_of!(Frame, elr),
    off_esr = const offset_of!(Frame, esr),
    off_fpcr = const offset_of!(Frame, fpcr),
    off_q = const offset_of!(Frame, q),
);

/// Allows SError from here on (ADR-0024). The vectors are installed by the
/// boot code.
pub fn init() {
    // SAFETY: clearing a mask bit changes no memory; an SError now reaches
    // the vectors, which are installed.
    unsafe { core::arch::asm!("msr daifclr, #4", options(nomem, nostack, preserves_flags)) };
}

/// Exception class name (`ESR_ELx.EC`, "ESR_EL1, Exception Syndrome Register").
fn class_name(ec: u64) -> &'static str {
    match ec {
        0x00 => "unknown reason",
        0x01 => "WFI or WFE trapped",
        0x07 => "FP/SIMD access trapped",
        0x0e => "illegal execution state",
        0x15 => "SVC from AArch64",
        0x16 => "HVC from AArch64",
        0x17 => "SMC from AArch64",
        0x18 => "system register access trapped",
        0x19 => "SVE access trapped",
        0x20 => "instruction abort, lower EL",
        0x21 => "instruction abort, same EL",
        0x22 => "PC alignment fault",
        0x24 => "data abort, lower EL",
        0x25 => "data abort, same EL",
        0x26 => "SP alignment fault",
        0x2c => "FP exception",
        0x2f => "SError",
        0x30 | 0x31 => "breakpoint",
        0x32 | 0x33 => "software step",
        0x34 | 0x35 => "watchpoint",
        0x3c => "BRK instruction",
        _ => "other",
    }
}

/// Exception class of a syndrome.
pub const fn exception_class(esr: u64) -> u64 {
    (esr >> 26) & 0x3f
}

/// Whether `FAR_EL1` holds the faulting address for this syndrome.
fn far_is_valid(esr: u64) -> bool {
    const FNV: u64 = 1 << 10; // ISS.FnV of instruction and data aborts
    match exception_class(esr) {
        0x20 | 0x21 | 0x24 | 0x25 => esr & FNV == 0,
        0x22 | 0x34 | 0x35 => true,
        _ => false,
    }
}

fn origin(kind: u64) -> &'static str {
    match kind {
        0..=3 => "EL1 on SP_EL0, which the kernel does not use",
        4..=7 | KIND_OVERFLOW => "EL1",
        8..=11 => "EL0 (AArch64), not wired up until stage 2",
        _ => "EL0 (AArch32), not wired up until stage 2",
    }
}

fn exception_name(kind: u64) -> &'static str {
    if kind == KIND_OVERFLOW {
        "kernel stack overflow"
    } else {
        ["synchronous", "IRQ", "FIQ", "SError"][(kind & 3) as usize]
    }
}

/// Prints the report of ADR-0024.
fn report(frame: &Frame, kind: u64) {
    let ec = exception_class(frame.esr);
    println!();
    println!(
        "ZORAYA-FAULT {} from {}",
        exception_name(kind),
        origin(kind)
    );
    println!("  EC {ec:#x}: {}", class_name(ec));
    print!(
        "  ESR_EL1 {:#x}  ELR_EL1 {:#x}  SPSR_EL1 {:#x}",
        frame.esr, frame.elr, frame.spsr
    );
    if far_is_valid(frame.esr) {
        println!("  FAR_EL1 {:#x}", frame.far);
    } else {
        println!("  FAR_EL1 not valid");
    }
    for (i, r) in frame.x.iter().enumerate() {
        print!("  x{i:<2} {r:#018x}");
        if i % 3 == 2 {
            println!();
        }
    }
    println!("  sp  {:#018x}", frame.sp);
}

fn fatal(frame: &Frame, kind: u64) -> ! {
    report(frame, kind);
    let ec = exception_class(frame.esr);
    let name = exception_name(kind);
    if far_is_valid(frame.esr) {
        panic!(
            "{name} from {}: EC={ec:#x} ({}) ESR_EL1={:#x} ELR_EL1={:#x} FAR_EL1={:#x}",
            origin(kind),
            class_name(ec),
            frame.esr,
            frame.elr,
            frame.far,
        );
    }
    panic!(
        "{name} from {}: EC={ec:#x} ({}) ESR_EL1={:#x} ELR_EL1={:#x}",
        origin(kind),
        class_name(ec),
        frame.esr,
        frame.elr,
    );
}

/// Called by the vectors, with `kind` the vector number (0 to 15) or
/// [`KIND_OVERFLOW`]. Returning resumes the context in `frame`.
#[unsafe(no_mangle)]
extern "C" fn exception_dispatch(frame: &mut Frame, kind: u64) {
    #[cfg(feature = "qemu-test")]
    if kind == 4 && catching::intercept(frame) {
        return;
    }
    fatal(frame, kind)
}

#[cfg(feature = "qemu-test")]
pub use catching::{catch, recover_to};

/// Test-only ways out of a fault (ADR-0024: the normal kernel has none).
#[cfg(feature = "qemu-test")]
mod catching {
    use core::arch::{asm, global_asm};
    use core::ptr;
    use core::sync::atomic::{AtomicPtr, Ordering};

    use super::{Frame, exception_class, far_is_valid};

    /// What a caught fault looked like.
    #[derive(Debug, Clone, Copy)]
    pub struct Fault {
        pub ec: u64,
        pub esr: u64,
        pub elr: u64,
        /// `None` if the syndrome says `FAR_EL1` is not valid.
        pub far: Option<u64>,
    }

    /// Callee-saved registers of [`catch`]'s caller: x19..x30, sp, d8..d15.
    #[repr(C, align(16))]
    struct Saved {
        regs: [u64; 22],
        fault: Option<Fault>,
    }

    global_asm!(
        r#"
    .text
    // __catch_call(saved: x0, f: x1, arg: x2) -> x0 = 0, or 1 after a fault.
__catch_call:
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
    mov     x19, x0
    mov     x0, x2
    blr     x1
    mov     x0, x19
    mov     x9, #0
    b       1f
    // The fault handler resumes here with x0 = saved.
__catch_landing:
    mov     x9, #1
1:
    ldp     x19, x20, [x0, #0]
    ldp     x21, x22, [x0, #16]
    ldp     x23, x24, [x0, #32]
    ldp     x25, x26, [x0, #48]
    ldp     x27, x28, [x0, #64]
    ldp     x29, x30, [x0, #80]
    ldr     x10, [x0, #96]
    mov     sp, x10
    ldp     d8, d9, [x0, #104]
    ldp     d10, d11, [x0, #120]
    ldp     d12, d13, [x0, #136]
    ldp     d14, d15, [x0, #152]
    mov     x0, x9
    ret
"#
    );

    unsafe extern "C" {
        fn __catch_call(saved: *mut u64, f: extern "C" fn(usize), arg: usize) -> u64;
        fn __catch_landing();
    }

    static ACTIVE: AtomicPtr<Saved> = AtomicPtr::new(ptr::null_mut());

    /// In the fault path: if a [`catch`] is active, records the fault and
    /// arranges to resume at its landing pad.
    pub(super) fn intercept(frame: &mut Frame) -> bool {
        let saved = ACTIVE.swap(ptr::null_mut(), Ordering::Relaxed);
        if saved.is_null() {
            return false;
        }
        let fault = Fault {
            ec: exception_class(frame.esr),
            esr: frame.esr,
            elr: frame.elr,
            far: far_is_valid(frame.esr).then_some(frame.far),
        };
        // SAFETY: `saved` is the live local of the `catch` that set ACTIVE,
        // which has not returned: it clears ACTIVE before it does.
        unsafe { (*saved).fault = Some(fault) };
        frame.x[0] = saved as u64;
        frame.elr = __catch_landing as *const () as u64;
        true
    }

    /// Runs `f(arg)` and returns the fault that ended it, if one did.
    ///
    /// `f` is abandoned at the fault, so it must hold nothing that needs
    /// dropping. Only a synchronous exception is caught.
    pub fn catch(f: extern "C" fn(usize), arg: usize) -> Option<Fault> {
        let mut saved = Saved {
            regs: [0; 22],
            fault: None,
        };
        ACTIVE.store(&raw mut saved, Ordering::Relaxed);
        // SAFETY: `__catch_call` saves every callee-saved register in `saved`
        // and restores them on both ways out, so to this code it is a normal
        // call. `saved` outlives the call.
        let faulted = unsafe { __catch_call((&raw mut saved).cast(), f, arg) };
        ACTIVE.store(ptr::null_mut(), Ordering::Relaxed);
        if faulted == 0 {
            None
        } else {
            // SAFETY: written by `intercept` during the call above, on this
            // CPU, so the volatile read sees it.
            unsafe { ptr::read_volatile(&raw const saved.fault) }
        }
    }

    unsafe extern "C" {
        static __stack_top: u8;
        static __stack_bottom: u8;
    }

    /// Leaves whatever context the CPU is in through `eret`, and calls
    /// `f(arg)` on an empty boot stack with no exception in progress. IRQ,
    /// FIQ and SError are as `SPSR` below says; debug stays masked.
    ///
    /// # Safety
    ///
    /// Every frame on the current stack is discarded without running any code
    /// in it, so nothing may still reference data on the stack, and no other
    /// code may be running on it.
    pub unsafe fn recover_to(f: extern "C" fn(usize) -> !, arg: usize) -> ! {
        /// EL1h with D, I and F masked, as stage 1 boot runs before the GIC.
        const SPSR: u64 = 0x5 | (1 << 9) | (1 << 7) | (1 << 6);
        let limit = &raw const __stack_bottom as u64 + super::FRAME_SIZE as u64;
        // SAFETY: the caller guarantees the old frames are dead. __stack_top
        // is the 16-byte aligned top of the boot stack from linker.ld, the
        // limit is the one the boot code sets, and `f` is a valid EL1 entry
        // that takes x0 and never returns. All interrupts are masked until
        // `eret` installs the new state, so the registers written here
        // cannot be overwritten by an exception.
        unsafe {
            asm!(
                "msr daifset, #0xf",
                "msr sp_el0, {limit}",
                "mov sp, {top}",
                "msr elr_el1, {f}",
                "msr spsr_el1, {spsr}",
                "eret",
                limit = in(reg) limit,
                top = in(reg) &raw const __stack_top,
                f = in(reg) f,
                spsr = in(reg) SPSR,
                in("x0") arg,
                options(noreturn),
            )
        }
    }
}
