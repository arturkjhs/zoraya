# ADR-0036: The user copy fix-up as the one resumed kernel fault

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0024, ADR-0031
- **Evidence:** CP-12, CP-15, alternatives-postmortem.md#l4-family-and-sel4

## Context
INV-0024-2 says the normal kernel has no path that resumes after a kernel fault. ADR-0024's text planned one exception, a fault in a marked copy-from-user routine, and ADR-0031 built it as `usercopy::fixup`. The invariant was not updated, so its `checked` status says more than is true. TASK-08's review also found that `fixup` looked only at the PC, so a fault on the kernel's own destination in that routine would have been taken for a bad user pointer (R-24).

## Decision drivers
1. A bad user pointer is an error for the call, not a kernel fault (CP-12, CP-15).
2. A real kernel fault stops the kernel (ADR-0024). The exception must not swallow one.
3. An invariant must say what the code does (ADR-0001 rule 6).

## Considered options

### Option A: State the exception, and narrow it
- **Description:** A new invariant replaces INV-0024-2, naming the one exception: a data abort from EL1, with the PC in the copy routine and `FAR_EL1` in the user range.
- **Evidence for:** Matches ADR-0024's plan. The `FAR` check serves driver 2, and it can be tested.
- **Evidence against:** Replaces accepted text for one sentence.
- **Cost:** Low (estimate: a check of one line, a test).
- **Precedent:** Linux on arm64 resumes through an exception table keyed by PC [outside evidence: Linux `arch/arm64/mm/extable.c`].

### Option B: Walk the tables first, and resume nothing
- **Description:** Validate the user range by walking its tables before copying, so that no fault can happen, and remove the fix-up. seL4 has no general user access in the kernel (alternatives-postmortem.md#l4-family-and-sel4), which is the same aim of never faulting.
- **Evidence for:** INV-0024-2 becomes true word for word.
- **Evidence against:** Reopens ADR-0031. Slower. With SMP, another thread can unmap between the walk and the copy, so the fault path comes back.
- **Cost:** Moderate.
- **Precedent:** seL4, by a different route.

### Option C: Define the fault as not being a kernel fault
- **Description:** Leave the text, and read the fix-up as outside "kernel fault".
- **Evidence for:** No work.
- **Evidence against:** The exception is taken from EL1 by kernel code, so this is wordplay, and driver 3 fails.
- **Cost:** None now.
- **Precedent:** None in our evidence.

## Recommendation
Recommended: **Option A**. It is a recommendation, not a decision.

- INV-0036-1 replaces INV-0024-2 when this ADR is accepted, and INV-0024-2's row is marked as replaced.
- `fixup` takes the PC and `FAR_EL1`, and resumes only for a data abort from EL1 with the PC in the routine and `FAR_EL1` below the end of the user range. Any other fault there follows ADR-0024's table. This part is built on this branch, since the architect asked for it.
- The rest of ADR-0024 and ADR-0031 stands.

## Consequences
- **Positive.** The invariant says what the code does, and the exception cannot hide a kernel fault.
- **Negative.** The record of this rule now spans three ADRs.
- **Follow-up.** The same check applies to a future `sttrb` form.

## Invariants
- **INV-0036-1.** The normal kernel resumes after a fault from EL1 in exactly one case: a data abort with the PC in the user copy routine and `FAR_EL1` in the user range. *Check:* the test-only paths are `#[cfg(feature = "qemu-test")]`, and an in-QEMU test faults on a kernel destination inside the routine and sees that it is not resumed, while `badptr` shows user faults are.

## Open questions
- None beyond ADR-0031's.

## Revisit triggers
- A second routine that needs a fix-up.

## Architect's decision
