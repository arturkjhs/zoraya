# ADR-0034: Faults from user processes at stage 2

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0004, ADR-0024, ADR-0031
- **Evidence:** CP-06, CP-10, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris

## Context
ADR-0004 rule 5 says a faulting thread is suspended, and the fault goes to an exception channel if one is bound. Rule 1 defines `ExitRecord` with reason `Fault`. Channels arrive with stage 3, so at this stage no handler can exist. This ADR says what happens until then, and does not change ADR-0004.

## Decision drivers
1. A user fault must never stop the kernel (ADR-0024 covers only kernel faults).
2. The exit status lives on the process object (CP-06, ADR-0004 rule 1).
3. No code runs asynchronously in the faulting process (CP-10).
4. Stage 3 must be able to add channels without changing this behaviour for a process with none bound.

## Considered options

### Option A: Deliver a signal (Linux baseline)
- **Description:** `SIGSEGV` with a handler frame on the user stack.
- **Evidence for:** Familiar [outside evidence: POSIX `sigaction`].
- **Evidence against:** Asynchronous code in the process, which CP-10 and ADR-0004 rule out.
- **Cost:** High.
- **Precedent:** Linux.

### Option B: End the process with an exit record
- **Description:** The behaviour ADR-0004 implies when no exception channel is bound: `ExitRecord { reason: Fault, detail }`, and the whole process ends.
- **Evidence for:** Matches ADR-0004 exactly for the no-handler case, so stage 3 only adds the bound case.
- **Evidence against:** No debugging of a stopped process yet.
- **Cost:** Low (estimate: 100 lines).
- **Precedent:** Zircon ends a process when no exception channel handles the fault (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon exception docs]. Hubris's supervisor is told, and restarts tasks (alternatives-postmortem.md#hubris).

### Option C: Suspend forever and report
- **Description:** Keep the thread suspended for an operator to inspect.
- **Evidence for:** Keeps state for debugging.
- **Evidence against:** Nothing can inspect it at this stage, and memory stays pinned.
- **Cost:** Low.
- **Precedent:** None in our evidence.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **What counts as a fault from EL0.** Anything synchronous other than `svc`: aborts, undefined or privileged instructions, alignment faults, `brk`.
- **`detail`.** The value of `ESR_EL1`'s exception class. `ExitRecord.code` is 0, which is meaningless for a fault.
- **Effects.** All the process's threads end, its memory and ASID are released, and its exit record stays readable by the kernel until reaped.
- **Log.** One line on the console: process, class, `ELR_EL1` and `FAR_EL1` when valid. It is a diagnostic, not an ABI. It carries no user data, and stops being unconditional when exception channels exist.
- **SError from EL0.** Remains a kernel fault (ADR-0024): it may be asynchronous, so its cause cannot be pinned on the process.

## Consequences
- **Positive.** The kernel survives any user program. The exit record is right from the first day.
- **Negative.** No handling and no debugger yet.
- **Follow-up.** Exception channels in stage 3.

## Invariants
- **INV-0034-1.** A synchronous exception from EL0 other than `svc` ends that process with reason `Fault` and the exception class in `detail`, and other threads and the kernel go on. *Check:* in-QEMU tests with a fault program for each class.
- **INV-0034-2.** A process's frames and ASID are released at exit, by any cause. *Check:* in-QEMU test comparing frame counts before and after many processes.

## Open questions
- Whether a fault should also be readable by a parent, which depends on stage 3's process handles.

## Revisit triggers
- Exception channels in stage 3.

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-08-review.md`.
