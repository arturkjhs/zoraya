# TASK-05: Kernel stage 1 — exceptions, interrupts, timer, MMU, memory allocation

## Context

Stage 0 is on `main` (TASK-04): the kernel boots on QEMU `virt` under HVF and TCG, parses the device tree with `zoraya-dtb`, prints over PL011, shuts down via PSCI, and `make test` runs host tests, in-QEMU tests (`ZORAYA-TEST` protocol), fmt, clippy and the ADR-0001 checks.

Read first: `CLAUDE.md`, `docs/design.md` (including Known limitations), `docs/tasks/TASK-04-review.md`, `docs/adr/index.md`, and ADR-0006 (deadlines and monotonic time) in full. Read the Recommendation sections of the other accepted ADRs.

This task is roadmap stage 1:

| Stage | Content | Done when |
|---|---|---|
| 1 | Exception vectors, GIC, timer, MMU (kernel in upper half), physical frame allocator, kernel heap | timer ticks; `Box`/`Vec` work in the kernel |

Still single-core. No threads, no EL0, no syscalls (stage 2).

## Step 1: Proposed ADRs for the decisions this stage makes

Memory layout conventions and similar choices are architectural decisions (`CLAUDE.md`). Before writing the code that depends on them, write proposed ADRs from `docs/adr/0000-template.md`. **ADR numbers 0020 to 0029 are reserved for this task.** Status `proposed`, "Architect's decision" empty, at least three options each, a Linux baseline and a non-Unix precedent where one exists (seL4, Zircon, Redox, Hubris and others from `docs/research/alternatives-postmortem.md`). Keep each ADR short: these are engineering decisions, not research.

At least:

1. **Kernel virtual address space layout.** Translation granule; VA size for TTBR0 and TTBR1; where the kernel image, kernel stacks, heap, device mappings and (if any) a direct map of physical memory live; whether a direct map exists at all and the security cost of having one; guard pages; how the kernel gets from physical addresses at boot to its high-half link address (position-independent early code, a separate early stub, or another method); KASLR now or later. State what the user half (TTBR0) will be reserved for in stage 2.
2. **Physical memory management.** Allocator structure (bitmap, buddy, free lists), the interface the rest of the kernel uses, how reserved regions are excluded (kernel image, device tree blob, `/memreserve/` entries, `reserved-memory` nodes, firmware areas), and what happens on exhaustion.
3. **Kernel heap.** Allocator design, how it grows from the frame allocator, alignment and fragmentation behaviour, and the policy on allocation failure in kernel code (panic, fallible allocation APIs, or both, and where each is allowed).
4. **Interrupts and time.** Supported GIC versions (the device tree decides at boot; say which versions are supported and tested), which generic timer the kernel uses under HVF and TCG and why, periodic tick vs one-shot deadline programming (relate it to ADR-0006's `Deadline`), and the kernel's monotonic clock: unit, source, and guarantees.
5. **Kernel fault policy.** What the kernel does on a synchronous exception, SError, or unexpected interrupt taken from EL1, and what it prints. (EL0 faults follow ADR-0004 and come in stage 2.)

Commit the ADRs as one step **before** the code that depends on them.

Then implement the option each ADR recommends. The architect accepts or changes these ADRs when reviewing this branch; merging the branch is conditional on that. If the architect changes a decision, the branch is reworked to match. Keep code that depends on a decision behind a clear module boundary so a change stays local.

## Step 2: Host-tested crates

Following `CLAUDE.md`, logic that does not need the machine goes in `no_std` crates with host tests:

- **`crates/zoraya-pmm`** — physical frame allocator over a list of regions with reservations. Tests: never returns a frame inside a reserved region or outside RAM; no frame is returned twice while allocated; freeing an unallocated frame is detected; exhaustion behaves as the ADR says; property tests over random allocate/free sequences against a simple model; works with the region sets parsed from the committed `zoraya-dtb` fixtures.
- **`crates/zoraya-heap`** (or the name the ADR implies) — the kernel heap allocator logic, independent of `GlobalAlloc` plumbing. Tests: alignment of every returned block, no overlap between live blocks, reuse after free, fragmentation under a stated workload, behaviour when the backing source is exhausted; property tests against a model.
- **`crates/zoraya-pagetable`** (if it fits the ADR) — building and editing translation tables in a buffer, with descriptor encoding for the granule chosen. Tests: mapping, unmapping, permission bits (read/write/execute/user, never write+execute in kernel mappings), block vs page mappings, walking a table back to the mapping set; tests decode the produced descriptors bit by bit against the architecture's field definitions, cited in comments.

No external crates in these without the `CLAUDE.md` procedure. Property testing as a dev-dependency is acceptable if recorded in `docs/dependencies.md`.

## Step 3: Kernel

### Exceptions
- Real vector table for the current EL with SP_ELx: synchronous, IRQ, FIQ, SError. Save a full register frame, decode `ESR_EL1`, and handle per the fault-policy ADR. A kernel fault prints at least the exception class, `ESR_EL1`, `FAR_EL1` (when valid), `ELR_EL1`, `SPSR_EL1` and a register dump, then panics.
- Lower-EL vectors exist and panic with a clear message (they are wired up in stage 2).
- Return from exceptions with `eret`. Replace the stage-0 test recovery that restarts tests inside the exception context (TASK-04 review R-10) with recovery through `eret` to a trampoline.

### EL2 leftovers from stage 0
- Set `MDCR_EL2` explicitly before `eret` to EL1 (TASK-04 review R-04), so debug and PMU behaviour no longer depends on reset values. Document the chosen value.

### MMU
- Build the kernel tables with `zoraya-pagetable` (or equivalent) according to the layout ADR. Map kernel sections with separate permissions: text read+execute, read-only data read-only and non-executable, data and bss read-write and non-executable. Map device regions (UART, GIC, anything else used) from the device tree as Device memory, never executable.
- Turn on the MMU and caches with the barrier and TLB maintenance sequence the architecture requires; cite the relevant Arm ARM sections in comments. Remove any temporary identity mapping once it is no longer needed.
- Kernel stack with an unmapped guard page below it; a stack overflow must fault and be reported, not corrupt memory. The stage-0 canary may stay as a second check.
- After the MMU is on, memory is Normal, so the stage-0 restriction on atomic read-modify-write ends. Update `CLAUDE.md`'s Kernel code section to state what is now allowed.

### Physical memory, heap, synchronization
- Initialise `zoraya-pmm` from the device tree's memory regions minus every reserved region named in the ADR.
- Provide `GlobalAlloc` backed by the heap allocator, so `alloc::boxed::Box`, `Vec`, `BTreeMap` and `String` work in the kernel.
- Provide the minimal lock the kernel needs on a single core with interrupts (for example a spinlock that masks interrupts while held), with a short note on what changes for SMP.

### Interrupts and timer
- GIC driver for the version(s) the ADR supports, discovered from the device tree, including the interrupt numbers of the timer and the UART.
- Generic timer driven as the ADR says. A monotonic clock API returning the unit the ADR defines, and the primitive for "interrupt me at deadline D" that later stages will use for `Deadline`.
- The boot banner prints the GIC version, timer frequency, and memory statistics (total, reserved, free) after the allocators are set up.

## Tests

In-QEMU tests (`ZORAYA-TEST`), run under every configuration `make test-qemu` already uses (HVF and TCG, EL1 and EL2 entry, GICv2 and GICv3 where QEMU and the accelerator allow; state which combinations run and why any is skipped):

- a `brk` and an access to an unmapped address are taken, reported with the correct exception class and `FAR_EL1`, and recovered by the test harness (expected failures where appropriate);
- writing to kernel text faults; executing from data faults (W^X holds);
- a deliberate stack overflow hits the guard page and is reported (expected failure);
- the timer delivers interrupts; over a measured interval, the count of interrupts and the monotonic clock agree with the timer frequency within a stated tolerance (state it and why, given HVF);
- a one-shot deadline fires no earlier than requested;
- `Box`, `Vec`, `BTreeMap` and `String` work; a stress test allocates and frees enough to exercise heap growth;
- the frame allocator never returns a frame overlapping the kernel image, the device tree blob or any reserved region (checked against the live device tree);
- after paging is on, the UART and GIC are reached through their new mappings and the old identity mapping is gone (an access through it faults).

Host tests: everything in Step 2.

`make test` must pass from a clean clone on the architect's Mac. CI (Linux/TCG) must stay green.

## Invariants and documentation

- `docs/adr/invariants.md`: add the invariants of the new ADRs with status `checked: <command>` where this task implements the check. Leave the ADR-0002…0006 statuses unchanged unless a check genuinely becomes possible.
- `docs/design.md`: update the "as built" sections (boot, memory, interrupts, Known limitations) and the repository map. **Do not change the Vision section or the roadmap table**; the architect marks stage progress.
- `docs/dependencies.md`: any new dependency.

## Process

1. Read the context files.
2. Step 1: proposed ADRs 0020+. Commit.
3. Step 2 crates with host tests. Commit per crate.
4. Exceptions and EL2 fix. Commit.
5. MMU, guard page. Commit.
6. Frame allocator and heap in the kernel. Commit.
7. GIC and timer. Commit.
8. Tests, invariants, documentation. Commit.
9. Independent review by a subagent that did not write the code: run `make test` from a fresh clone; check the MMU enable sequence, barriers and TLB maintenance against the Arm ARM; check every `unsafe` block and `SAFETY` comment; check that the allocators cannot hand out reserved or already-allocated memory; check that each ADR's recommendation is what the code does. Record findings in `docs/tasks/TASK-05-review.md` and fix them, with a disposition table.
10. Final report: what was built, the exact `make test` summary, boot banner output under HVF and TCG, the proposed ADRs with their recommendations in one line each, known limitations.

## Git

- Branch `kernel/stage-1`, from the latest `main`.
- Follow the Git section of `CLAUDE.md`: configured identity only, commit after each step, never merge, never push.

## Definition of done

- Proposed ADRs 0020+ for the five decisions, committed before the dependent code.
- Exceptions, MMU with W^X and a guard page, frame allocator, heap, GIC, timer and monotonic clock work under HVF and TCG.
- The roadmap criterion holds: timer interrupts arrive, and `Box`/`Vec` work in the kernel.
- All listed tests exist and pass; `make test` passes from a clean clone.
- Independent review done and addressed.

## Out of scope

- Secondary CPUs (SMP), threads, scheduling, EL0, syscalls, handles, ELF loading (stage 2 and later).
- Changing accepted ADRs, `docs/research/`, the Vision section or the roadmap.
