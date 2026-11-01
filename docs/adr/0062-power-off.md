# ADR-0062: Power off through a handle

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0040, ADR-0043
- **Evidence:** CP-01, CP-02

## Context
The normal kernel powered off when its work was done. With a shell the person decides when, so a process must be able to ask. The kernel has PSCI `SYSTEM_OFF` behind `shutdown`.

## Decision drivers
1. No ambient authority (CP-01): a call anyone can make is wrong.
2. Rights only decrease (CP-02).
3. Small: one object, one call.

## Considered options

### Option A: `reboot(2)` with a privilege check (Linux)
- **Description:** A system call any process can try, checked against a capability bit of its identity.
- **Evidence for:** Familiar and simple.
- **Evidence against:** Zoraya has no identity to check (ADR-0002 rule 6). Ambient, against driver 1.
- **Cost:** Small.
- **Precedent:** Unix.

### Option B: A resource handle (Zircon)
- **Description:** A call that takes a system resource handle of the power kind `[outside evidence: fuchsia.dev zx_system_powerctl]`.
- **Evidence for:** Authority is a handle. A general resource object would serve others.
- **Evidence against:** A resource hierarchy for one use.
- **Cost:** *(estimate)* Larger than needed.
- **Precedent:** Fuchsia.

### Option C: A `Power` object with one right
- **Description:** A kernel object made once, with right `POWER`, handed to the first process.
- **Evidence for:** Drivers 1 to 3. It has Option B's shape without its hierarchy.
- **Evidence against:** One more object type, for one call.
- **Cost:** *(estimate)* 30 lines.
- **Precedent:** Option B.

### Option D: The kernel powers off when the first process ends
- **Description:** No call: the shell exits, and the kernel stops.
- **Evidence for:** No new ABI.
- **Evidence against:** A shell crash and a request look alike, and no other process could ever ask.
- **Cost:** None.
- **Precedent:** `init` on Linux panics instead.

## Recommendation
Recommended: **Option C**, with Option D's behaviour kept for a first process that ends. It is a recommendation, not a decision.

- **Object.** `PowerObject`, `NotWaitable`. Rights: `POWER`, `DUPLICATE`, `TRANSFER`, so that it can be handed on deliberately. It is a new right in `Rights`.
- **Call.** `power_off(power)` needs `POWER`. On success it does not return.
- **Where it comes from.** The kernel installs it at slot 4, `POWER`, in the first process alone. `kernel_handles` for any other process does not. Nothing else can make one.

**Tensions with accepted ADRs.** *ADR-0040:* a new right and object type in its lists. *ADR-0043:* slot 4 extends its startup convention, for the first process alone. Accepting this ADR replaces both rows (ADR-0001 rule 1).

## Consequences
- **Positive.** Who can stop the machine is visible in handle lists.
- **Negative.** A new type and right.
- **Follow-up.** `init` takes this handle when ADR-0053 is accepted.

## Invariants
- **INV-0062-1.** Only the first process starts with a `Power` handle, and no call makes one. *Check:* in-QEMU test that a kernel-started process has no valid handle at slot 4, and a unit test over `OBJECT_TYPES` that `PowerObject` alone carries `POWER`. That no call makes one is held by review.
- **INV-0062-2.** `power_off` without `POWER` is refused, and the machine goes on. *Check:* in-QEMU test with a wrong handle.
- **INV-0062-3.** `poweroff` in the shell ends the machine with QEMU status 0. *Check:* scripted serial test.

## Open questions
- Whether a reboot call is needed.

## Revisit triggers
- A second system-wide power action.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
