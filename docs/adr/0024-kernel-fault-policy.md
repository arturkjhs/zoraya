# ADR-0024: Kernel fault policy

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0004, ADR-0020
- **Evidence:** alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4

## Context
Stage 1 replaces the stage-0 catch-all vectors with real ones. This ADR says what happens on an exception taken from EL1. Exceptions from EL0 follow ADR-0004 and arrive in stage 2; until then their vectors panic. The kernel runs on `SP_EL1` (`current EL with SP_ELx`), so a stack overflow takes its exception on the same exhausted stack unless something intervenes.

## Decision drivers
1. A kernel in an unknown state must not go on to corrupt more (driver of fail-stop designs; compare ADR-0004, where faults stop the faulting thread).
2. The report must be enough to find the bug from a serial log alone, because there is no debugger in the usual flow and tests read that log.
3. A stack overflow must be reported, which needs a working stack (ADR-0020).
4. Stage 2 will need a defined way to survive a fault during a copy from user memory.

## Considered options

### Option A: Oops and continue (Linux baseline)
- **Description:** Report, kill the current task, and keep running unless `panic_on_oops` is set.
- **Evidence for:** A desktop or server stays up; a developer can read the log [outside evidence: Linux `Documentation/admin-guide/bug-hunting.rst`].
- **Evidence against:** Locks held by the killed task stay held and invariants stay broken [assumption: reasoning from the design]. Linux also offers `panic_on_oops`, which stops instead [outside evidence: Linux `Documentation/admin-guide/sysctl/kernel.rst`]. Nothing can be killed at stage 1, which has no tasks.
- **Cost:** High (estimate: needs a task model and a recovery story).
- **Precedent:** Linux.

### Option B: Fail-stop with a full report
- **Description:** Print a report once, panic, halt. No resumption of any kernel fault. Hubris restarts faulted tasks through a supervisor task (alternatives-postmortem.md#hubris), which leaves nothing to restart its kernel [assumption]. seL4 has a machine-checked proof (alternatives-postmortem.md#l4-family-and-sel4), which aims to rule such faults out rather than recover from them [assumption].
- **Evidence for:** Simple and honest about what is known. Keeps state for a post-mortem.
- **Evidence against:** One kernel bug takes the whole system down, which matters more with drivers in the kernel. The roadmap keeps drivers in services [assumption].
- **Cost:** Low (estimate: 200 lines).
- **Precedent:** Hubris, seL4, and Zircon, which panics on a kernel fault (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon `kernel/arch/arm64/exceptions_c.cc`].

### Option C: Restart the kernel from a checkpoint
- **Description:** A fault resets the kernel, as a microkernel might with its services (a supervisor model applied to the kernel itself).
- **Evidence for:** Availability.
- **Evidence against:** Needs persistent state that survives, and a design for it that does not exist. No precedent in our evidence applies this to the kernel proper.
- **Cost:** Unknown, large.
- **Precedent:** None found.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

| Exception taken from EL1 | Action |
|---|---|
| Synchronous (any class) | Report and panic |
| Synchronous with the stack pointer out of room | Move to the emergency stack, report as stack overflow, panic |
| IRQ with a registered handler | Handle, end of interrupt, `eret` |
| Spurious IRQ (INTID 1020 to 1023) | Count and `eret` |
| IRQ with no registered handler | Report and panic: the kernel enabled it, so this is a bug |
| FIQ | Report and panic: nothing is configured to raise one |
| SError | Report and panic: it may be asynchronous and imprecise, so nothing can be trusted to resume |
| From EL0, or on `SP_EL0` | Report and panic with a message that these are not wired up until stage 2 |

- **Report.** Vector kind, `ELR_EL1`, `SPSR_EL1`, and x0 to x30 with SP. For a synchronous exception or SError also the exception class by number and name, `ESR_EL1`, and `FAR_EL1` when the syndrome says it is valid. An interrupt sets neither `ESR_EL1` nor `FAR_EL1`, so its report says so instead.
- **Frame.** Every entry saves x0 to x30, SP, `ELR`, `SPSR`, `ESR`, `FAR`, and the FP/SIMD state: the target lets the compiler use NEON in any function, so an interrupt that uses it would otherwise corrupt the code it interrupted.
- **Stack check.** Before touching memory, the vector compares SP with a limit held in `SP_EL0`, which is free while the kernel runs and costs no memory access. `TPIDR_EL1` holds a scratch register for those few instructions. Stage 2 must keep the limit of the current thread's stack there on kernel entry. A second overflow, on the emergency stack, halts silently: there is no stack left to report with.
- **Interrupts in handlers.** All handlers run with IRQ masked; there is no nesting at stage 1.
- **Unmasked at run time:** SError (`A`) from the moment the vectors are installed, IRQ and FIQ after the GIC is ready. Debug (`D`) stays masked.
- **Test kernel.** Tests may catch a fault for inspection. That path is compiled only into the test kernel; the normal kernel has no way to resume a kernel fault.
- **Stage 2.** The one planned exception is a fault in a marked copy-from-user routine, which will be a new ADR.

## Consequences
- **Positive.** A single, tested code path for all bad events, and a report good enough to read in a CI log.
- **Negative.** Larger exception frames, about 800 bytes, on every entry. No fault tolerance in the kernel.
- **Follow-up.** Fault fix-ups for user copies in stage 2; a crash record that survives reboot if it is ever needed.

## Invariants
- **INV-0024-1.** A kernel fault prints the exception class, `ESR_EL1`, `ELR_EL1`, `SPSR_EL1` and the registers, then panics. `FAR_EL1` is printed when valid. *Check:* expected-failure in-QEMU tests with `--expect-line` on the report.
- **INV-0024-2.** The normal kernel contains no path that resumes after a kernel fault. *Check:* the resume path is `#[cfg(feature = "qemu-test")]`, and `make test` builds the normal kernel without the feature.
- **INV-0024-3.** A vector entry can always report: a stack overflow reaches the report on the emergency stack. *Check:* expected-failure in-QEMU test.

## Open questions
- Whether SError can ever be expected from QEMU devices. None has been seen.
- How the stack check survives stage 2. `SP_EL0` holds the user stack at EL0 and `TPIDR_EL1` is the natural per-CPU pointer, so lower-EL vectors must swap the limit in before any Rust, and SMP needs a limit and an emergency stack per CPU. To be settled in the stage 2 ADR.

## Revisit triggers
- Kernel bugs in production-like use, which would argue for more containment.
- Drivers moving into the kernel, which would raise the cost of fail-stop.

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-05-review.md`.
