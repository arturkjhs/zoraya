//! Entry point: from the first instruction to `kernel_main`.
//!
//! QEMU starts the image at `_start` (the ELF entry) on CPU 0 with the MMU
//! and caches off and interrupts masked, at EL1, or at EL2 with
//! `virtualization=on`. The other CPUs stay powered off under PSCI until
//! someone calls `CPU_ON` (QEMU `hw/arm/virt.c` sets `start-powered-off` for
//! every CPU but the first when it emulates PSCI). The code still parks any
//! CPU whose MPIDR affinity is not 0, in case a loader starts them all.
//!
//! Steps:
//! 1. Keep `x0` (a possible DTB address from the loader) in `x19`.
//! 2. Park secondary CPUs.
//! 3. Record the exception level at entry in `x20`.
//! 4. At EL2: the minimal configuration below, then `eret` to EL1h.
//!    At EL1: continue. Anything else: park (no console exists yet).
//! 5. At EL1: known `SCTLR_EL1`, FP/SIMD enabled (the `aarch64-unknown-none`
//!    target lets the compiler use NEON registers), catch-all exception
//!    vectors, stack, `.bss` cleared.
//! 6. `kernel_main(x19, x20)`.
//!
//! Minimal EL2 configuration (everything not listed is left at its reset
//! value, and EL2 is never re-entered):
//! - `HCR_EL2 = RW`: EL1 is AArch64; no stage-2 translation, no traps to
//!   EL2, no interrupt routing to EL2, `E2H = 0`.
//! - `CNTHCTL_EL2 |= EL1PCTEN | EL1PCEN`: EL1 may use the physical counter and
//!   timer. `CNTVOFF_EL2 = 0`: virtual and physical counts agree.
//! - `CPTR_EL2 = 0x33ff`: the RES1 bits, with `TFP = 0` (FP/SIMD not trapped)
//!   and `TTA = TCPAC = 0`; SVE and SME stay trapped.
//! - `HSTR_EL2 = 0`: no AArch32 system register traps.
//! - `VPIDR_EL2`/`VMPIDR_EL2` = `MIDR_EL1`/`MPIDR_EL1`, so EL1 reads its real IDs.
//! - If the CPU has a GIC system register interface (`ID_AA64PFR0_EL1.GIC`):
//!   `ICC_SRE_EL2 = SRE | DFB | DIB | Enable`, so EL1 can use it later.
//! - `SPSR_EL2 = EL1h` with D, A, I, F masked; `ELR_EL2` = the EL1 path.

use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.boot, "ax"
    .global _start
_start:
    mov     x19, x0

    // Park every CPU except affinity 0.0.0.0 (MPIDR Aff3 at 39:32, Aff2..0
    // at 23:0).
    mrs     x1, mpidr_el1
    mov     x2, #0xffffff
    movk    x2, #0xff, lsl #32
    and     x1, x1, x2
    cbnz    x1, .Lpark

    mrs     x20, CurrentEL
    ubfx    x20, x20, #2, #2
    cmp     x20, #1
    b.eq    .Lel1
    cmp     x20, #2
    b.ne    .Lpark

    // EL2 -> EL1, see the module documentation.
    mov     x1, #(1 << 31)
    msr     hcr_el2, x1
    mrs     x1, cnthctl_el2
    orr     x1, x1, #3
    msr     cnthctl_el2, x1
    msr     cntvoff_el2, xzr
    mov     x1, #0x33ff
    msr     cptr_el2, x1
    msr     hstr_el2, xzr
    mrs     x1, midr_el1
    msr     vpidr_el2, x1
    mrs     x1, mpidr_el1
    msr     vmpidr_el2, x1
    mrs     x1, id_aa64pfr0_el1
    ubfx    x1, x1, #24, #4
    cbz     x1, 1f
    mov     x1, #0xf
    msr     S3_4_C12_C9_5, x1       // ICC_SRE_EL2
1:
    mov     x1, #0x3c5              // EL1h, DAIF masked
    msr     spsr_el2, x1
    adr     x1, .Lel1
    msr     elr_el2, x1
    isb
    eret

.Lel1:
    // SCTLR_EL1: RES1 bits of ARMv8.0 (29, 28, 23, 22, 20, 11); MMU, caches,
    // alignment and stack-alignment checks off; little-endian.
    mov     x1, #0x0800
    movk    x1, #0x30d0, lsl #16
    msr     sctlr_el1, x1
    // CPACR_EL1.FPEN = 0b11: no FP/SIMD traps at EL1.
    mov     x1, #(3 << 20)
    msr     cpacr_el1, x1
    adr     x1, __exception_vectors
    msr     vbar_el1, x1
    isb

    adrp    x1, __stack_top
    add     x1, x1, :lo12:__stack_top
    mov     sp, x1

    // Clear .bss; both ends are 16-byte aligned (linker.ld).
    adrp    x1, __bss_start
    add     x1, x1, :lo12:__bss_start
    adrp    x2, __bss_end
    add     x2, x2, :lo12:__bss_end
2:
    cmp     x1, x2
    b.hs    3f
    stp     xzr, xzr, [x1], #16
    b       2b
3:
    mov     x0, x19
    mov     x1, x20
    bl      kernel_main

.Lpark:
    wfe
    b       .Lpark

    // Catch-all vector table: every entry reports and halts through
    // `unhandled_exception`. Stage 0 expects no exception at all; this only
    // turns a fault into a message instead of a silent hang.
    .balign 2048
__exception_vectors:
    .irp    n, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
    .balign 128
    mov     x0, #\n
    mrs     x1, esr_el1
    mrs     x2, elr_el1
    mrs     x3, far_el1
    b       unhandled_exception
    .endr
"#
);
