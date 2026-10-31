# ADR-0020: Kernel virtual address space layout

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001
- **Evidence:** CP-16, DM-032, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4

## Context
Stage 1 turns the MMU on with the kernel in the upper half (`docs/design.md`, roadmap). The layout fixes the translation granule, what is mapped where, and how the kernel gets from the physical addresses QEMU loads it at to its link address. Stage 2 adds EL0 in the lower half, so the split has to be settled now.

Measured here: the build target (`aarch64-unknown-none`) has `relocation-model: static` and inline stack probes (`rustc --print target-spec-json`, TASK-05). Static relocation means stored pointers (vtables, `&str` constants, `fmt` tables) are absolute link addresses, so ordinary Rust cannot run before the MMU maps the link address. Inline probes touch every 4 KiB of a large frame, so a one-page guard cannot be stepped over.

## Decision drivers
1. No mapping is writable and executable at once, from the first stage that has mappings (CP-16, DM-032).
2. Early code must be small enough to review against the Arm ARM, because it runs before any exception can be reported.
3. Later stages need to reach any physical frame cheaply: page tables, zeroing, and the copy-on-write snapshot of ADR-0003 [assumption].
4. A stack overflow must fault and be reported, not corrupt memory (TASK-05).
5. The layout must not depend on a particular RAM size or address beyond what the device tree says.

## Considered options

The options differ in how physical memory is reached. All of them share these points, which have no serious alternative here: 4 KiB granule (supported by TCG and by HVF on Apple silicon, and the only one whose page size matches common ELF alignment [outside evidence: Arm ARM DDI 0487, "Translation granule"]); 48-bit VA in both halves (`T0SZ = T1SZ = 16`, four levels); the image linked at `0xFFFF_FFFF_8000_0000`, so PC-relative `adrp` reaches all of it; TTBR0 reserved for EL0 from stage 2.

### Option A: Full direct map of RAM (Linux baseline)
- **Description:** All RAM at a fixed offset, read-write, including the frames of the kernel image.
- **Evidence for:** Simplest; any frame reachable by adding a constant [outside evidence: Linux `Documentation/arch/arm64/memory.rst`].
- **Evidence against:** The image's text frames get a writable alias, which defeats driver 1 for anyone who can write through the direct map. Every frame in the machine is reachable from a kernel write primitive (the `ret2dir` class of attack) [outside evidence: Kemerlis et al., USENIX Security 2014].
- **Cost:** Lowest (estimate: no code beyond the mapping loop).
- **Precedent:** Linux. Zircon also has a physmap (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon `kernel/vm` sources].

### Option B: No direct map; temporary mappings only
- **Description:** A frame is mapped into a small window when needed and unmapped after. Hubris goes further and has no paging at all (alternatives-postmortem.md#hubris).
- **Evidence for:** Smallest window for a write primitive; each access to a frame is deliberate.
- **Evidence against:** Each access costs a page-table update and TLB invalidation; page-table walks need recursion or temporary maps; the snapshot of ADR-0003 touches many frames [assumption]. seL4 keeps a kernel window over physical memory despite its minimalism (alternatives-postmortem.md#l4-family-and-sel4) [outside evidence: seL4 reference manual, kernel memory].
- **Cost:** Highest (estimate: a mapping cache and its invalidation rules; more code in the fault-prone path).
- **Precedent:** 32-bit Linux `kmap` for high memory, abandoned where a 64-bit address space made it unnecessary [outside evidence: Linux `Documentation/mm/highmem.rst`].

### Option C: Direct map with holes
- **Description:** A direct map of RAM, read-write and never executable, minus the kernel image and every region the device tree reserves. The device tree blob is mapped read-only in it. Fixed separate windows hold devices, the heap, and later thread stacks.
- **Evidence for:** Keeps the cost profile of Option A for drivers 3 and 5, while the image has exactly one mapping, so driver 1 holds for text with no writable alias.
- **Evidence against:** Still exposes all ordinary RAM to a kernel write primitive. Tables built in the image must be reached through the image mapping, because the direct map does not cover them.
- **Cost:** Small (estimate: a range subtraction and one extra lookup for table frames).
- **Precedent:** Linux marks its own linear alias of text read-only on arm64 (`rodata=full`) to get a similar result [outside evidence: Linux `arch/arm64/mm/mmu.c`].

## Recommendation
Recommended: **Option C**, with this layout. It is a recommendation, not a decision.

| Virtual address | Size | Contents |
|---|---|---|
| `0x0000_0000_0000_0000` | 256 TiB | TTBR0: unused and disabled (`TCR_EL1.EPD0 = 1`) at stage 1; EL0 from stage 2 |
| `0xFFFF_0000_0000_0000` | 128 TiB | Direct map: `VA = base + PA`, RW, NX. RAM above 128 TiB is rejected at boot |
| `0xFFFF_8000_0000_0000` | 1 TiB | Device window, Device-nGnRnE, RW, NX, allocated upward |
| `0xFFFF_8100_0000_0000` | 1 TiB | Kernel heap window (ADR-0022), RW, NX |
| `0xFFFF_8200_0000_0000` | 1 TiB | Thread stacks with guard pages, reserved for stage 2 |
| `0xFFFF_FFFF_8000_0000` | 2 GiB | Kernel image: text RX, rodata R, data and bss RW-NX, boot stack |

- **Boot.** A small assembly stub runs at the physical load address. It builds five boot tables: identity map of the first 4 GiB with 1 GiB blocks (the block holding the image Normal, the others Device), and the image at its link address with 2 MiB blocks. It enables the MMU and jumps to the link address. Rust then starts, on the boot tables; it never runs with the MMU off. It builds the final tables and replaces the boot tables from identity-mapped code, with TTBR1 going through an empty table so that no block and page translation of one address ever coexist in the TLB (break-before-make [outside evidence: Arm ARM DDI 0487]). Finally it sets `EPD0`, clears TTBR0 and invalidates the TLB. The boot image mapping is writable and executable for that short window, with one CPU and interrupts masked; driver 1 applies from the final tables onward.
- **Hardware W^X.** `SCTLR_EL1.WXN = 1` from the final tables on, as a second layer behind the table attributes.
- **Guard.** One unmapped page below the boot stack. A check in the exception vector moves to an emergency stack when the stack pointer cannot hold a frame, so the overflow can be reported (ADR-0024). Inline stack probes make one page sufficient.
- **KASLR.** Later. There is nothing at EL0 to defend against until stage 2, and the ELF is loaded at fixed physical addresses. The stub derives the physical base from the PC, so the layout does not preclude it.

## Consequences
- **Positive.** Exactly one mapping of each image frame. Early code is one assembly stub. Rust code only ever runs with its link addresses mapped.
- **Negative.** A short RWX window at boot. Any kernel write primitive still reaches ordinary RAM through the direct map. The device tree has to be parsed twice: once through the identity map, and again through the direct map so no pointer into the identity map survives.
- **Follow-up.** Stage 2 decides ASID use and enables TTBR0 per process. KASLR gets its own ADR.

## Invariants
- **INV-0020-1.** No kernel mapping in the final tables is both writable and executable, and none is accessible from EL0. *Check:* `zoraya-pagetable` rejects such attributes at the API (host test) and an in-QEMU test walks the live tables.
- **INV-0020-2.** After boot, TTBR0 translation is disabled and an access through the old identity mapping faults. *Check:* in-QEMU test on the old UART address, plus a read of `TCR_EL1.EPD0`.
- **INV-0020-3.** No page of the kernel image is mapped twice. *Check:* in-QEMU test walks the live tables and finds each image frame at most once, and an access to the image through the direct map faults.
- **INV-0020-4.** Every device mapping is Device memory and never executable. *Check:* in-QEMU test walks the device window of the live tables.
- **INV-0020-5.** The page below the boot stack is unmapped, and overrunning the stack is reported as a stack overflow. *Check:* expected-failure in-QEMU test.

## Open questions
- Whether a separate read-write alias is needed for code patching. Nothing needs one at stage 1.
- Whether the 128 TiB limit on the direct map matters: QEMU `virt` puts RAM far below it.

## Revisit triggers
- A kernel write primitive found through the direct map in a review or an exploit.
- A workload where reaching frames dominates and huge direct-map blocks stop being enough.
- Real hardware coming into scope (the boot identity assumptions are QEMU `virt` facts).

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-05-review.md`.
