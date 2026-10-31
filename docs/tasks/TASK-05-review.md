# TASK-05 independent review

- **Reviewed:** branch `kernel/stage-1`, commits `f716793..555d2c0` (`git log main..kernel/stage-1`, 10 commits), HEAD `555d2c04c026488069ed0a567b26c8b976764c04`.
- **Reviewer:** independent subagent that did not write the code (TASK-05 Process item 9).
- **Date:** 2026-10-31.
- **Environment:** Apple Silicon M4 Mac, 10 CPUs, macOS (Darwin 25.4.0), HVF available; Rust 1.99.1 (pinned), QEMU 11.1.2. All builds, mutations and probes ran in fresh clones under the scratch directory. The real repository was not modified: `git status` there shows only the untracked task file, as before, and HEAD is unchanged.
- **Not available:** a copy of the Arm ARM. Register fields, encodings and sequences below were checked from my knowledge of DDI 0487, not against the text. No section numbers are cited. The Linux CI workflow was not run.

## make test from a fresh clone

```
git clone --branch kernel/stage-1 /Users/svitlana/workspace/os clone && cd clone && make test
```

**Exit status: 0** (wall time 28.8 s, fresh `target/`).

Summary lines, verbatim (`test result` lines with 0 tests left out):

```
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.92s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.09s
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
adr-check: 11 ADRs checked against INV-0001-1..5
adr-check: warning: 34 invariants of accepted ADRs have no check yet (ADR-0001 rule 6):
adr-check: 0 errors, 34 warnings
[auto-default] PASS: 47 passed, 7 expected failures
[auto-smp4-2g] PASS: 47 passed, 7 expected failures
[auto-el2] PASS: 47 passed, 7 expected failures
[tcg-default] PASS: 47 passed, 7 expected failures
[tcg-smp4-gicv2] PASS: 47 passed, 7 expected failures
[tcg-el2] PASS: 47 passed, 7 expected failures
[tcg-smp4-gicv3] PASS: 47 passed, 7 expected failures
[boot-auto] PASS: 0 passed, 0 expected failures
[boot-tcg] PASS: 0 passed, 0 expected failures
make test: all checks passed
```

The only `warning` lines in the log are adr-check's. fmt and the three clippy runs printed nothing.

**Flakiness.** `make test-qemu` was run 10 more times from the same clone: 3 idle, 4 with 20 spinning processes, 3 with 40 spinning processes (4 times the CPU count). All 10 exited 0, with all 7 configurations passing, taking 9 to 10 s idle, 22 s and about 40 s. The timing tests did not flake. This covers HVF and TCG on this Mac only, not a 2-core CI runner.

**Configurations outside the Makefile,** with the 7 expected panics and no line checks:

| Configuration | Result |
|---|---|
| TCG, EL2, GICv3 | PASS 47 + 7 |
| HVF, EL2, 4 CPUs | PASS 47 + 7 |
| HVF, 8 GiB; TCG, 8 GiB (RAM past 4 GiB) | PASS 47 + 7 |
| HVF, 32 MiB | PASS 47 + 7 |
| TCG `-cpu max` (52-bit PA), GICv3, EL1 and EL2 | end line `pass=47 fail=7` |
| TCG, 16 MiB and 3 MiB | Boots. `heap_stress_grows_and_reuses` runs out of memory, as expected of a 16 MiB stress, and tests after it cascade. Not a bug |
| HVF, `gic-version=2` | QEMU exits with status 1, as the design says |

## Findings

| ID | Area | Finding | Severity | Suggested fix |
|---|---|---|---|---|
| R-01 | Fault report | `exception.rs:281-329` decodes `frame.esr` and `frame.far` for every kind, but `ESR_EL1` and `FAR_EL1` are only written by a synchronous exception or SError. An IRQ report therefore shows the previous fault as if it were this one. Verbatim from `auto-default`: `ZORAYA-FAULT IRQ from EL1` / `EC 0x25: data abort, same EL` / `ESR_EL1 0x96000004 ... FAR_EL1 0xffff820000002000`, and the panic says `IRQ from EL1: EC=0x25 (data abort, same EL)`. That address and class belong to the earlier `fault_report_for_unmapped_access` test. This breaks INV-0024-1 ("`FAR_EL1` is printed when valid"), which is marked `checked`, and ADR-0024 driver 2. FIQ is the same, and so is a stack overflow entered through the IRQ vector (`KIND_OVERFLOW` with stale ESR). SError has a valid ESR but its `FAR_EL1` is judged by the same function, which correctly says no. | error | Pass the vector kind to `report`, and print EC, ESR and FAR only for synchronous kinds and SError; for IRQ and FIQ print "no syndrome". For `KIND_OVERFLOW` record which vector entered, or say that ESR may be stale. Add `--expect-line` for the corrected IRQ report. |
| R-02 | `zoraya-pagetable` | `map_leaf` (`lib.rs:388-411`) writes a block over a valid table descriptor. `unmap` keeps empty tables (`lib.rs:419`), and `coverage` sees only holes through them, so `map` of a 2 MiB or 1 GiB range over a previously unmapped page range passes the `AlreadyMapped` check, then replaces the table with a block. Reproduced on the host: `map_pages` 4 KiB, `unmap`, then `map` 2 MiB at the same aligned VA gives a level 2 block (`0x60000040200705`), 1 write, **0 invalidations**. A valid table to block change needs break-before-make, or the walk cache can keep using the old table. The old table is also leaked, and the doc comment "They are reused by later mappings" is false for this path. `tables_of_an_unmapped_range_are_reused` covers only same-level reuse. Not reachable in today's kernel, because only `map_page` maps after boot and devices are never unmapped. Stage 2 can reach it through any `map` after an `unmap`. | error | In `map_with`, stop at a level that holds a table: either descend and use smaller leaves, or do break-before-make on the empty table and hand it back. Add this case to the model test. |
| R-03 | Table memory barriers | A new table from frames is zeroed through the direct map (`frames.rs:69-71`), and its address is written into a live parent (`zoraya-pagetable/src/lib.rs:403-404`) with no barrier between. To the table walker those are unordered stores: only `mem.sync()` at the end of `map_with` has a `dsb ishst`. A speculative walk through the new parent can read old frame contents as descriptors and cache a bogus translation. Frames are explicitly not zeroed on free, so that content is arbitrary. The first 64 tables come from `.bss` and are safe; every later table, so heap growth once the pool is used up, is exposed. By reasoning, not reproduced: QEMU does not show it. | error | Add `dsb ishst` after zeroing in `KernelTables::alloc_table` (or a `TableMemory::publish` step before the parent write), and record the event in the host mock so a test can assert the order. |
| R-04 | Boot cache maintenance | `boot.rs:145-146,195-210` invalidates the boot tables only after writing them. The comment says that "drops any stale line", but a dirty line present on entry can be evicted between the non-cacheable write and the `dc ivac` and overwrite a descriptor. `dc ivac` may also be performed as clean-and-invalidate, notably under a hypervisor, which writes such a line back. `.boot_tables` is `NOLOAD`, so nothing guarantees it is clean at entry. As I recall, Linux's `head.S` has invalidated before and after for this reason. Harmless on fresh QEMU RAM. | imprecision | Invalidate the range before the writes as well as after, and correct the comment. |
| R-05 | Interrupt masking | `mask_interrupts` and `save_and_mask_interrupts` (`arch/aarch64/mod.rs:83-86,172-178`) use `options(nomem)`, so the compiler may move memory operations across the mask. `SpinLock::lock` needs its compare-exchange to stay after the mask, or an IRQ in the window panics with "lock taken twice". `restore_interrupts` has no `nomem` and its comment explains why; the entry side lacks that. Not observed: the kernel is built in the debug profile. | imprecision | Drop `nomem` from every mask and unmask, so they act as compiler barriers. |
| R-06 | Heap | `search_spans` calls `start.next_multiple_of(align)` (`zoraya-heap/src/lib.rs:222`) unchecked. Heap addresses are near the top of the address space, so `align >= 2^47` overflows once any span exists. A probe in the test kernel with `Layout(8192, 1 << 47)` panicked with `attempt to add with overflow`, holding `HEAP`, and later tests failed with "lock taken twice". `GlobalAlloc` must return null, as ADR-0022 says. With overflow checks off the value wraps to 0, and the `at + bytes <= end` check then passes, returning address 0 and corrupting the list (by reading the code). No type has such an alignment; only a run-time `Layout` could. Sizes and `try_reserve` are fine. The host test cannot show this because host addresses are low. | imprecision | Use `checked_next_multiple_of`, and treat `None` as no fit. |
| R-07 | Lifetimes | `dtb::locate` is a safe function returning `Fdt<'static>` over the identity map (`dtb.rs:58-88`), and `PlatformInfo<'static>` follows from it. Those references die at `drop_identity_map`. `main.rs` shadows them and does not use them again, which I checked, but the type claims the opposite, the compiler cannot help, and the SAFETY comments at `dtb.rs:62-65,72-74` state a validity they do not have. | imprecision | Return a lifetime that ends before paging takes over (for example a borrow of a token that `paging::init` consumes), or make `locate` unsafe and state the rule. |
| R-08 | SAFETY comments | `testing.rs:320-323` says the panic machinery's frames "are all on the boot stack". After the overflow test they are on the emergency stack. The outcome is still sound, because `recover_to` sets SP itself, but the invariant as stated is false. `arch/aarch64/mod.rs:90-92`: the SAFETY comment of the safe, public `unmask_interrupts` asserts the vectors and GIC are ready, which nothing enforces beyond its single caller. | imprecision | Reword the first. For the second, make the function `unsafe` with a `# Safety` section. |
| R-09 | Boot comment | `boot.rs:312` says `sctlr_mmu` is the `.Lel1` value plus M, C, SA and I. That would be `0x30d0_180d`. The constant `0x30d0_190d` also sets bit 8 (SED). Harmless, since there is no AArch32 EL0, but the only hand-computed value in the stub does not match its comment. | imprecision | Use `0x30d0_180d`, or name SED in the comment. |
| R-10 | ADR-0021, design | ADR-0021 Option B and `docs/design.md:176` say 64 KiB of metadata per 2 GiB. Two bits per 4 KiB frame is **128 KiB** per 2 GiB. Measured: reserved goes from 1816 KiB at 128 MiB (8 KiB of map) to 1936 KiB at 2 GiB (128 KiB of map). | imprecision | Correct both. |
| R-11 | Test `deadline_wakes_a_waiting_cpu` | `testing.rs:722-724` checks `FIRED` and then runs `wfi` with IRQ unmasked. If the interrupt lands between the load and the `wfi`, the CPU sleeps with an empty queue until the runner's timeout. The window is very small and it did not occur in 11 runs, but the pattern is the classic lost wake-up, and stage 2's idle loop will copy it. | imprecision | Mask IRQ, check, `wfi` (which still wakes on a masked pending interrupt), unmask, loop. |
| R-12 | INV-0024-1 check strength | `invariants.md` marks INV-0024-1 `checked`, including `SPSR_EL1` and the registers, but no `--expect-line` covers them. Mutation `m09`, deleting the register dump, gives `make test-qemu` exit 0. The expected lines also cannot see R-01. | imprecision | Add expected lines for `SPSR_EL1`, `x30` and `sp`. |
| R-13 | ADR evidence (rule 5) | (a) ADR-0024 Option B cites `alternatives-postmortem.md#hubris` for a kernel fault being fatal and `#l4-family-and-sel4` for seL4's position on kernel faults. Those sections support task restarts and proofs, not those statements, and the claims have no `[outside evidence]` tag. (b) ADR-0021 Option A cites `Documentation/mm/page_allocator.rst`, which does not exist (docs.kernel.org 404); the file is `page_allocation.rst`, and that page has only a heading, so it does not support the claim either. (c) Several evidence bullets carry no tag, for example ADR-0024 Option A "Locks held by the killed task stay held" and "Linux itself offers `panic_on_oops` because of that", ADR-0021 Options A and B, ADR-0022 Option B. They are reasoning, so `[assumption]` would do. | imprecision | Tag or re-cite. Use `mm/page_alloc.c` or the buddy section of a verified source for (b). |
| R-14 | Stage 2 | `SP_EL0` as the stack limit and `TPIDR_EL1` as scratch collide with EL0: `SP_EL0` is the user stack, and `TPIDR_EL1` is the natural per-CPU pointer. ADR-0024 says the limit must be restored on kernel entry, but not how: lower-EL vectors run with no scratch and must swap `SP_EL0` before any Rust. A single emergency stack and a single limit also do not extend to SMP. | note | Settle in the stage 2 ADR; say so in ADR-0024's open questions. |
| R-15 | GICv2 and SMP | `gic::acknowledge` returns all of `GICC_IAR`. For an SGI from another CPU, bits 12:10 hold the source, so `dispatch` would see an ID of 1020 or more and count it as spurious without an EOI, leaving it active. Harmless on one CPU. | note | Keep the raw value for the EOI and mask to 10 bits for the ID, when SGIs arrive. |
| R-16 | GICv3 | `init_v3` and `gic::disable` do not poll `GICR_CTLR.RWP` after writing `GICR_ICENABLER0`, and `disable` does not poll `GICD_CTLR.RWP` for SPIs, so a disable may not have taken effect on return. `irq::unregister` clears the handler right after, so a late interrupt would be reported as unexpected. No effect under QEMU. | note | Poll RWP in `disable` and after the redistributor write. |
| R-17 | Weak or absent checks | Mutants that pass all of `make test-qemu`: `m15`, FPCR and FPSR not restored; `m12`, no `MDCR_EL2` write (QEMU's reset value is harmless, while `m11`, setting TPM and TDA, is caught by a timeout). FPCR and FPSR are saved correctly by reading the code. | note | Add FPCR to `interrupt_preserves_fp_registers`. `m12` cannot be seen from EL1; accept it. |
| R-18 | EL2 entry | `MDCR_EL2.HPMN = PMCR_EL0.N`: if a CPU has no PMU, `PMCR_EL0` may not be readable and `N = 0` makes `HPMN = 0`, which needs FEAT_HPMN0. TCG with `pmu=off` boots, so QEMU tolerates it. | note | Gate on `ID_AA64DFR0_EL1.PMUVer`, or record the assumption beside the E2H one. |
| R-19 | Arm ARM citations | The code cites sections by title as asked. I cannot confirm titles without the text, and two look unlike the titles I remember: "Preferred Execute-never" (`mmu.rs:45`) and "TLB maintenance requirements and the Break-Before-Make sequence" (`boot.rs:287`, ADR-0020). The facts they support are right: `SCTLR_EL1.WXN` may be cached in a TLB, and break-before-make applies. | note | Check titles against the revision in use and name the revision once. |
| R-20 | Branch base | `main` moved on with five TASK-06 commits (now `50b3cea`). `git merge-tree` shows a content conflict in `docs/adr/index.md` only. The task's "from the latest `main`" was true when the branch started, I assume. | note | The architect resolves it at merge by keeping both sets of rows. |

## Checked and found correct

**make test and repository rules**
- `make test` passes from a fresh clone, and timing tests did not flake (above).
- `Cargo.lock` has no `source =` lines, and `docs/dependencies.md` lists no crates, so no build needs the network.
- No change on the branch to `docs/research/`, `docs/tasks/`, `tools/posix-surface/` or `tools/run-qemu.sh`. The Vision section and roadmap table are unchanged.
- `allow(` appears nowhere in `kernel`, `crates` or the two tools.
- Every `unsafe` block has a `// SAFETY:` comment and every `unsafe fn` a `# Safety` section; clippy passes. `zoraya-pmm`, `zoraya-pagetable` and `zoraya-time` have `forbid(unsafe_code)`.
- ADRs were committed first (`42d4331`), status `proposed`, "Architect's decision" empty. There is one commit per process step. `CLAUDE.md`'s Kernel code section was updated as the task told it to be.
- Git identity is the configured one on all 10 commits.

**MMU enable sequence (`boot.rs`, `mmu.rs`)**
- `TCR_EL1 = 0xb510_3510` plus IPS: T0SZ = T1SZ = 16; IRGN = ORGN = 01, SH = 11 for both; TG0 = 00, TG1 = 10; A1 = 0, TBI = 0, HA = HD = 0, AS = 0. IPS comes from PARange capped at 5, and `-cpu max` with 52-bit PA boots.
- `MAIR_EL1`: Attr0 `0x00` Device-nGnRnE, Attr1 `0xff`. Descriptors `0x705` and `0x401 | 3 << 53` are as commented.
- `SCTLR_EL1` RES1 set is right; `.Lel1` bit by bit is correct, apart from R-09.
- Boot table indices for `0xffffffff80000000` are L0 511, L1 510, L2 0. The cache line size from `CTR_EL0.DminLine` is right.
- Order is right: MAIR, TCR, TTBRs, ISB, `tlbi vmalle1`, `dsb nsh`, `ic iallu`, `dsb nsh`, ISB, SCTLR, ISB, then a branch by absolute address. The code at that point is identity-mapped Normal and executable.
- TTBR1 replacement runs from the identity map, touches no memory, has all of DAIF masked, and goes through an empty table with TLB invalidation on both sides. That is a sound form of break-before-make. `dsb ishst` precedes it, and walks are cacheable, so no clean is needed.
- WXN is set after the RWX mappings are gone and followed by TLB invalidation, since it may be cached. The boot identity block is RWX too, but no code runs through it by then.
- `drop_identity_map`: EPD0, ISB, TTBR0 = 0, ISB, invalidation, including EPD0's own possible caching. The identity map is really gone: `identity_map_is_gone` and `null_pointer_read_faults` fail under `m07`.
- TLB scope: local at boot, correct on one CPU; `vmalle1is` and `vaae1is` afterwards. The operand keeps VA[55:12]; VAAE covers all ASIDs and all levels; the 2 MiB threshold is safe.
- No mismatched attributes: the identity Device blocks and the device window are both nGnRnE, and the identity Normal block and direct map are both write-back.
- ELF: three `PT_LOAD`s, RX, R, RW, no orphan sections, physical addresses right.

**Exceptions**
- Frame layout: offsets match the `const` asserts and the assembly, 816 bytes, 16-byte aligned. All registers are restored, and `ELR` and `SPSR` are restored with exceptions masked, so nested exceptions are safe.
- Each vector fits 128 bytes; the table is 2 KiB aligned.
- The limit check is correct: `sp >= bottom + FRAME_SIZE` leaves a frame's room, flags are only used before `SPSR` matters, and `SPSR` holds the interrupted flags. `TPIDR_EL1` use cannot be interrupted.
- The overflow path recovers x0, x1 and SP, and bit 0 marks the emergency stack. The fault in `auto-default` had FAR `0xffffffff800a3f78`, inside the guard page at `0xffffffff800a3000`. Emergency use is 5,552 of 32,768 bytes.
- `catch` (callee-saved x19 to x30, SP, d8 to d15 fit `Saved`), `recover_to` and `enter_el0` do what they say and mask before changing `ELR` and `SPSR`.
- The normal ELF has no `recover_to`, `__catch` or `enter_el0` symbols (INV-0024-2).
- `far_is_valid` is right for the classes listed, FnV included.

**Allocators**
- `zoraya-pmm`: no bug found. An extra adversarial test passed: 4,000 rounds of random overlapping regions at addresses 0, `0x40000000`, near 2^52 and near 2^64, with unaligned, zero, huge and edge reservations. Each drain found no duplicate, no reserved byte, nothing outside RAM, and an exact free count. `align` of 2^62 and 2^63 in `alloc_contig` causes no overflow. `reserve`, `seal_padding`, next-fit wrap and all-or-nothing `free_contig` read correct.
- The kernel's use is right: reserved covers every device tree reservation, image, blob and metadata; `assert!(reserved_dropped == 0)` stops the boot; the direct map contains every free frame; the image has no alias.
- `zoraya-heap`: 200,000 random operations with alignments to 2 MiB and edge sizes gave no misalignment or overlap, and writing every block corrupted nothing. Huge sizes return null. Only R-06 was found. The fragmentation bound test exists and matches ADR-0022. `GlobalAlloc` plumbing and `try_box` are correct.
- `zoraya-pagetable`: descriptors are right bit by bit. Block selection, split order, rollback after failed map and failed split behave as documented. `h3`, reversing the split order, and `h5`, removing UXN, are caught.
- Lock order heap, tables, frames holds on every path. Nothing allocates under `FRAMES` or `KERNEL`.

**GIC and timer**
- v2 and v3 init sequences and system register encodings are right, including affinity, `IROUTER`, group 1, `WAKER`, byte priorities and the redistributor walk.
- EOI order is right: the handler changes the timer, then the EOI, so a level still high pends again. Spurious IDs get no EOI. An unexpected interrupt is disabled and ended before the report.
- Under HVF, by my reading of how QEMU handles the vtimer, a level left high is delivered again and a late deassertion only causes a spurious acknowledge, which the kernel counts. No run showed a hang or a tick shortfall.
- `arm` and the handler cannot race on one CPU, because the lock masks IRQ. Callbacks run with no lock held. `pop_due` is the real guard against early firing; `ticks_at` rounds up. Under `m10` (pop everything) the tick test fails.
- FP/SIMD is saved on every entry, and `m01` is caught.

**ADR conformance**
- ADR-0020 to 0024: the code does what each Recommendation says, apart from R-01 for ADR-0024's report: layout, five boot tables, WXN, EPD0, guard; two-bit bitmap, reserved list, exhaustion as a return; classes, 64 KiB chunks, 1 MiB growth, failure policy; GIC selection, virtual timer, one-shot deadlines, `Instant`; the fault table.
- Each has at least three options, a Linux baseline and a non-Unix precedent. The target spec claim (`relocation-model: static`, `stack-probes: inline`) is true. CP-14, CP-16, CP-20, DM-030, DM-032, DM-065, DM-067 and the Redox leak, Hubris and Barrelfish references say what is claimed. Exceptions are in R-13.
- `invariants.md` matches the tests, apart from R-12.

**Spec coverage and mutation results**
- Every listed test exists. GICv2 under HVF is skipped, with the reason in `docs/design.md`.
- Caught by `make test-qemu`: `m01` q registers not restored; `m03` guard mapped; `m04` no stack check (`no output line contains "ZORAYA-FAULT kernel stack overflow from EL1"`); `m05` image not reserved; `m06` blob not reserved; `m07` identity kept; `m08` no WXN; `m10`; `m11`; `m13` lock does not mask; `m14` no EOI; `m16` rodata writable; `m18` image in the direct map; `m24` no EOI for an unexpected interrupt; `m25` FAR always valid; `m27` cancel does not reprogram.
- Caught by host tests: `h1` reservations rounded inward, `h2` alignment ignored, `h3`, `h4` ticks rounded down, `h5`.
- Not caught: `m09` (R-12), `m12` and `m15` (R-17).

## Resolution

The implementing agent applied these after the review, in the commit that adds this file. `make test` was rerun afterwards: it passes, with 47 passed and 7 expected failures in each of the seven QEMU configurations, and both boot checks. The review text above is unchanged.

| ID | Resolution |
|---|---|
| R-01 | Fixed. The report and the panic message give the class, `ESR_EL1` and `FAR_EL1` only for exceptions that set them; an IRQ or FIQ report says it has no syndrome. A stack overflow says its syndrome is stale if an interrupt found the stack full. `--expect-line` covers the IRQ wording. ADR-0024 and `design.md` updated. |
| R-02 | Fixed. `map` no longer chooses a level above an existing table, so it uses the kept table, with no write over a valid entry. New host test `a_block_never_replaces_an_empty_table`. |
| R-03 | Fixed in `zoraya-pagetable`: `sync()` comes between a new table and the parent's link to it. New host test `a_new_table_is_visible_before_its_parent_links_it` asserts the order. As in the finding, the failure itself cannot be reproduced under QEMU. |
| R-04 | Fixed. The boot tables are invalidated before and after writing, and the comment says why. |
| R-05 | Fixed. `nomem` is gone from every mask and unmask, so they are compiler barriers. |
| R-06 | Fixed with `checked_next_multiple_of`. The in-QEMU test `fallible_allocation_returns_failure` now asks for this layout; heap addresses on the host are too low to show it. |
| R-07 | Fixed. `dtb::locate` is `unsafe`, with the identity map's lifetime as its contract, and the SAFETY comments state the real limit. |
| R-08 | Fixed. The SAFETY comment names the emergency stack, and `unmask_interrupts` is `unsafe` with a `# Safety` section. |
| R-09 | Fixed: `sctlr_mmu` is `0x30d0_180d`, matching its comment. SED is clear. |
| R-10 | Fixed in ADR-0021 and `design.md`: 128 KiB per 2 GiB. |
| R-11 | Fixed. The test masks interrupts around the check and `wfi`. Stage 2's idle loop should do the same. |
| R-12 | Fixed: `--expect-line` covers `SPSR_EL1`, `x30` and `sp`. |
| R-13 | Fixed. ADR-0024 Option B no longer attributes those positions to the sections, ADR-0021 cites `mm/page_alloc.c`, and the untagged reasoning named in (c) is tagged. |
| R-14 | Recorded in ADR-0024's open questions for the stage 2 ADR. |
| R-15 | Deferred to SMP, recorded in `design.md`, Known limitations. There are no SGIs on one CPU. |
| R-16 | Fixed. `disable` and `init_v3` wait for `RWP`, on GICv3. |
| R-17 | `interrupt_preserves_fp_registers` now checks FPCR. `m12` is accepted: an unset `MDCR_EL2` is not visible from EL1 under QEMU. |
| R-18 | Recorded as an assumption in the `boot.rs` header. No PMU-less CPU is in scope. |
| R-19 | Fixed by removing the two doubtful titles; the facts stay. The other titles are as unverified as the review says: no copy of the Arm ARM was available. |
| R-20 | Open, for the merge. `main` has since moved again (ADR-0010 to ADR-0015 accepted), so a merge now conflicts in `docs/adr/index.md`, `docs/adr/invariants.md` and `docs/design.md`. Each needs both sides kept. This branch was not rebased. |
