//! Entry point: from the first instruction to `kernel_main` (ADR-0020).
//!
//! All of this runs position-independently at the physical load address until
//! the MMU is on, then at the link address. Rust never runs with the MMU off:
//! pointers stored in its data are link addresses.
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
//!    target lets the compiler use NEON registers).
//! 6. Build the boot tables, enable the MMU and caches, jump to the link
//!    address.
//! 7. Exception vectors, stack and its limit in `SP_EL0`, `.bss` cleared.
//! 8. `kernel_main(x19, x20, physical base)`.
//!
//! Boot tables, replaced by `memory::paging` as soon as Rust is up:
//! - TTBR0: an identity map of the first 4 GiB with 1 GiB blocks. The block
//!   holding the image is Normal memory, the others Device, never executable.
//!   They cover the device tree, the UART and the code that replaces TTBR1.
//! - TTBR1: the image at its link address with 2 MiB blocks, writable and
//!   executable until the final tables replace them, with one CPU and
//!   interrupts masked.
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
//! - `MDCR_EL2 = HPMN = PMCR_EL0.N`: EL1 and EL0 own every PMU counter, and
//!   no debug or PMU access traps to EL2 (`TDE`, `TDA`, `TDRA`, `TDOSA`,
//!   `TPM`, `TPMCR` and `HPME` are all 0), whatever the reset value was.
//! - `SCTLR_EL1` gets its known value before `eret`, so EL1 never runs with
//!   the reset value.
//! - `SPSR_EL2 = EL1h` with D, A, I, F masked; `ELR_EL2` = the EL1 path.
//!
//! Left at reset: on CPUs that have them, the fine-grained trap registers
//! (`HFG*_EL2`, `HCRX_EL2`).
//!
//! Assumption: the CPU has a PMU, so `PMCR_EL0` reads; with `N = 0` the
//! `HPMN` value also needs FEAT_HPMN0. Both hold on the CPUs QEMU gives us.
//!
//! Assumption: `HCR_EL2.E2H` can be 0 (FEAT_E2H0). On a CPU where E2H is
//! RES1, `CNTHCTL_EL2` and `CPTR_EL2` have the E2H=1 layout and the values
//! above would trap FP/SIMD to EL2. Checked under QEMU 11.1.2: `HCR_EL2` reads
//! back as `RW` only, under both TCG (cortex-a72) and HVF (M4).

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
    mrs     x1, pmcr_el0
    ubfx    x1, x1, #11, #5         // PMCR_EL0.N
    msr     mdcr_el2, x1
    mrs     x1, id_aa64pfr0_el1
    ubfx    x1, x1, #24, #4
    cbz     x1, 1f
    mov     x1, #0xf
    msr     S3_4_C12_C9_5, x1       // ICC_SRE_EL2
1:
    mov     x1, #0x0800             // SCTLR_EL1, as at .Lel1
    movk    x1, #0x30d0, lsl #16
    msr     sctlr_el1, x1
    mov     x1, #0x3c5              // EL1h, DAIF masked
    msr     spsr_el2, x1
    adr     x1, .Lel1
    msr     elr_el2, x1
    isb
    eret

.Lel1:
    // SCTLR_EL1: RES1 bits of ARMv8.0 (29, 28, 23, 22, 20, 11); MMU, caches,
    // alignment and stack-alignment checks off; little-endian. Until the
    // vectors are installed at the link address, a fault here cannot be
    // reported.
    mov     x1, #0x0800
    movk    x1, #0x30d0, lsl #16
    msr     sctlr_el1, x1
    // CPACR_EL1.FPEN = 0b11: no FP/SIMD traps at EL1.
    mov     x1, #(3 << 20)
    msr     cpacr_el1, x1
    isb

    // The tables below need the 4 KiB granule (ID_AA64MMFR0_EL1.TGran4 is
    // 0xf if it is absent). There is no console yet to complain on.
    mrs     x10, id_aa64mmfr0_el1
    ubfx    x1, x10, #28, #4
    cmp     x1, #0xf
    b.eq    .Lpark

    // Where the image really is, from the PC.
    adrp    x21, __kernel_start
    lsr     x1, x21, #30
    cmp     x1, #4
    b.hs    .Lpark

    // Invalidate the tables' lines to the point of coherency, before and
    // after writing them with the MMU off. Before: a dirty line present on
    // entry could otherwise be evicted over the new contents. After: nothing
    // speculated meanwhile may hide them from cached accesses, and `dc ivac`
    // may act as clean and invalidate, so the first pass is what makes the
    // second harmless. CTR_EL0.DminLine is the log2 of the line size in
    // words.
    mrs     x1, ctr_el0
    ubfx    x1, x1, #16, #4
    mov     x11, #4
    lsl     x11, x11, x1
    adrp    x1, __boot_tables_start
    adrp    x3, __boot_tables_end
7:
    dc      ivac, x1
    add     x1, x1, x11
    cmp     x1, x3
    b.lo    7b
    dsb     sy

    adrp    x1, __boot_tables_start
    adrp    x2, __boot_tables_end
1:
    stp     xzr, xzr, [x1], #16
    cmp     x1, x2
    b.lo    1b

    // TTBR0: L0[0] -> L1, then four 1 GiB blocks.
    adrp    x3, __boot_l0_low
    adrp    x4, __boot_l1_low
    orr     x1, x4, #{table}
    str     x1, [x3]
    ldr     x5, ={block_device}
    mov     x6, #0
2:
    lsl     x1, x6, #30
    lsr     x2, x21, #30
    cmp     x2, x6
    mov     x2, #{block_normal}
    csel    x2, x2, x5, eq
    orr     x1, x1, x2
    str     x1, [x4, x6, lsl #3]
    add     x6, x6, #1
    cmp     x6, #4
    b.lo    2b

    // TTBR1: L0[511] -> L1, L1[510] -> L2, 2 MiB blocks for the image, whose
    // link address is the base of L2[0].
    adrp    x3, __boot_l0_high
    adrp    x4, __boot_l1_high
    adrp    x5, __boot_l2_high
    orr     x1, x4, #{table}
    str     x1, [x3, #(511 * 8)]
    orr     x1, x5, #{table}
    str     x1, [x4, #(510 * 8)]
    adrp    x6, __kernel_end
    mov     x1, #(0x200000 - 1)
    add     x6, x6, x1
    bic     x6, x6, x1              // end of the image, rounded up to a block
    mov     x7, x21
    mov     x2, #{block_normal}
3:
    orr     x1, x7, x2
    str     x1, [x5], #8
    add     x7, x7, #0x200000
    cmp     x7, x6
    b.lo    3b

    // The second pass, now that the tables are written.
    adrp    x1, __boot_tables_start
    adrp    x3, __boot_tables_end
4:
    dc      ivac, x1
    add     x1, x1, x11
    cmp     x1, x3
    b.lo    4b
    dsb     sy

    // MAIR, TCR and the table bases before the enable. TCR_EL1: T0SZ = T1SZ
    // = 16 (48-bit VA), TG0 = 4 KiB, TG1 = 4 KiB, walks inner shareable,
    // write-back, IPS from ID_AA64MMFR0_EL1.PARange capped at 48 bits (the
    // largest size with this granule). No hardware Access flag or dirty
    // state; 8-bit ASIDs taken from TTBR0, unused until stage 2.
    ldr     x1, ={mair}
    msr     mair_el1, x1
    ldr     x1, ={tcr}
    and     x2, x10, #0xf
    cmp     x2, #5
    mov     x3, #5
    csel    x2, x2, x3, lo
    bfi     x1, x2, #32, #3
    msr     tcr_el1, x1
    adrp    x1, __boot_l0_low
    msr     ttbr0_el1, x1
    adrp    x1, __boot_l0_high
    msr     ttbr1_el1, x1
    isb

    // Nothing translated may be cached from before ("TLB maintenance
    // instructions"), and instructions fetched with the MMU off may not
    // match memory once fetches are cacheable. With SCTLR_EL1.M = 1 taking
    // effect at the ISB, the next fetch goes through the identity map.
    tlbi    vmalle1
    dsb     nsh
    ic      iallu
    dsb     nsh
    isb
    ldr     x1, ={sctlr_mmu}
    msr     sctlr_el1, x1
    isb
    ldr     x1, =__high_entry
    br      x1
    .ltorg

    // At the link address from here on.
__high_entry:
    adr     x1, __exception_vectors
    msr     vbar_el1, x1
    isb

    adrp    x1, __stack_top
    add     x1, x1, :lo12:__stack_top
    mov     sp, x1
    adrp    x1, __stack_bottom
    add     x1, x1, :lo12:__stack_bottom
    add     x1, x1, #{frame_size}
    msr     sp_el0, x1              // the vectors' stack limit

    // Clear .bss; both ends are 16-byte aligned (linker.ld).
    adrp    x1, __bss_start
    add     x1, x1, :lo12:__bss_start
    adrp    x2, __bss_end
    add     x2, x2, :lo12:__bss_end
5:
    cmp     x1, x2
    b.hs    6f
    stp     xzr, xzr, [x1], #16
    b       5b
6:
    mov     x0, x19
    mov     x1, x20
    mov     x2, x21
    bl      kernel_main

.Lpark:
    wfe
    b       .Lpark

    // __replace_ttbr1(new: x0, empty: x1). Runs at its physical address,
    // through the identity map, and touches no memory, so nothing it needs
    // goes away while TTBR1 changes. Passing through a table with nothing
    // mapped means translations from the old and the new tables never exist
    // together, which break-before-make requires when block and page sizes
    // differ (break-before-make). The caller has masked every exception.
    .global __replace_ttbr1
__replace_ttbr1:
    msr     ttbr1_el1, x1
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    msr     ttbr1_el1, x0
    isb
    tlbi    vmalle1
    dsb     nsh
    isb
    ret
"#,
    frame_size = const super::exception::FRAME_SIZE,
    table = const 0b11,
    // Valid block, AF, inner shareable, AttrIndx = 1 (Normal), executable.
    block_normal = const 0x705,
    // Valid block, AF, AttrIndx = 0 (Device), PXN and UXN.
    block_device = const (0x401u64 | (3 << 53)),
    mair = const zoraya_pagetable::MAIR_VALUE,
    // T0SZ, T1SZ = 16; IRGNn = ORGNn = 1, SHn = 3, TG0 = 0, TG1 = 2.
    tcr = const 0xb510_3510u64,
    // The value at .Lel1 plus M (0), C (2), SA (3) and I (12).
    sctlr_mmu = const 0x30d0_180du64,
);
