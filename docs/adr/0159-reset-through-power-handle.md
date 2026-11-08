# ADR-0159: Resetting the machine through the `Power` handle

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0062, ADR-0155
- **Evidence:** CP-01, CP-02

## Context
ADR-0155 makes `reboot` a shell builtin, and the architect's decision on it (ADR-0062's note, 2026-11-03) says the
kernel needs a way for a `Power` holder to reset the machine. Today the kernel has one power call, `power_off`
(number 26), which needs the `POWER` right on the `PowerObject` that only the first process receives (ADR-0062,
INV-0062-1). The kernel already calls PSCI `SYSTEM_OFF` through the conduit the device tree names
(`kernel/src/psci.rs`); PSCI also defines `SYSTEM_RESET` (function 0x8400_0009, PSCI 0.2 and later)
`[outside evidence: ARM DEN 0022, PSCI specification]`, which QEMU `virt` implements `[assumption: not yet run here]`.
Until this exists `reboot` says it is not available (INV-0155-7, accepted status "until a kernel reset call").

`tools/run-qemu.sh` passes `-no-reboot`, so under the test runner a reset ends QEMU instead of restarting it.

## Decision drivers
1. No ambient authority (CP-01): resetting the machine needs a handle, as powering it off does.
2. Rights only decrease (CP-02): whoever may turn the machine off may restart it, and no one else.
3. Small: no new object, the least new ABI.

## Considered options

### Option A: `reboot(2)` with a command argument and a privilege check (Linux)
- **Description:** One call taking a command (`RB_POWER_OFF`, `RB_AUTOBOOT`, ...), checked against a privilege of the
  caller's identity.
- **Evidence for:** Familiar; one call for every power action.
- **Evidence against:** Zoraya has no identity to check (ADR-0002); the check is ambient, against driver 1. ADR-0062
  rejected it for power-off.
- **Cost:** *(estimate)* Small.
- **Precedent:** Linux, the BSDs.

### Option B: One call with an action argument on the `Power` handle (Zircon's `zx_system_powerctl`)
- **Description:** `power_off(power)` is replaced by `power_control(power, action)`, with actions `OFF` and `RESET`.
- **Evidence for:** One call for every future power action; authority is a handle (driver 1)
  `[outside evidence: fuchsia.dev zx_system_powerctl]`.
- **Evidence against:** Changes an existing call's shape, or keeps two calls for power-off; an unknown action value is
  one more refusal path to test.
- **Cost:** *(estimate)* 40 lines, plus changing every caller of `power_off`.
- **Precedent:** Fuchsia.

### Option C: A second call `power_reset(power)` needing `POWER`
- **Description:** A new call, the next free number, that takes the `Power` handle, needs the same `POWER` right as
  `power_off`, and on success does not return. If the device tree's PSCI has no `SYSTEM_RESET`, or the call fails,
  it returns `NotSupported` and the machine goes on.
- **Evidence for:** Drivers 1 to 3: no new object, no new right, `power_off` unchanged. The right that lets a process
  turn the machine off is the one that lets it restart it.
- **Evidence against:** One call per power action; a third action would argue for Option B.
- **Cost:** *(estimate)* 30 lines in the kernel and `zoraya-abi`, and the device-tree parser learning whether
  `SYSTEM_RESET` is present.
- **Precedent:** ADR-0062's own shape.

### Option D: A separate `RESET` right on the `Power` object
- **Description:** As C, but `power_reset` needs a new right `RESET`, so a holder could be given restart without
  power-off or the reverse.
- **Evidence for:** Finer authority (CP-02).
- **Evidence against:** Nothing today needs the split: the only holder is `init`, then the shell, and both need both.
  One more right in `Rights`.
- **Cost:** *(estimate)* As C, plus a right.
- **Precedent:** none here.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Call.** `power_reset(power: Handle)`, needing `POWER`. On success it does not return. Without `POWER` it is
  refused as `power_off` is (`AccessDenied`/`BadHandle` by the usual handle rules). If PSCI has no `SYSTEM_RESET`
  or the firmware call returns, it returns `NotSupported`.
- **Platform.** `zoraya-dtb`'s `Psci` gains `has_system_reset`, set as `has_system_off` is (PSCI 0.2 and later
  compatibles have both; a PSCI 0.1 node names its functions).
- **Shell.** `reboot` calls it; on `NotSupported` it prints `reboot: not supported by this machine` and exits 1.
- **Tests.** `tools/run-qemu.sh` passes `-no-reboot`, so a reset under the runner ends QEMU. The check of
  INV-0155-7 either runs with a new run-qemu.sh option that drops `-no-reboot` and expects the boot banner a second
  time, or expects QEMU to exit with status 0 right after `reboot` and the kernel's `resetting` line. The first is
  what INV-0155-7 says; it needs the option (CLAUDE.md: only run-qemu.sh invokes QEMU).

**Tensions with accepted ADRs.** *ADR-0062:* a second call on its object; its open question's note asked for this.
*ADR-0040:* a new call number in its list.

## Consequences
- **Positive.** `reboot` works; who can restart the machine is visible in handle lists.
- **Negative.** One more call.
- **Follow-up.** Kernel and `zoraya-abi` work, the `zoraya-dtb` field, the run-qemu.sh option, the `reboot` builtin,
  INV-0155-7's status.

## Invariants
- **INV-0159-1.** `power_reset` without `POWER` is refused, and the machine goes on. *Check:* in-QEMU test with a
  wrong handle and with a `Power` handle duplicated without `POWER`.
- **INV-0159-2.** `reboot` in the shell restarts the machine. *Check:* INV-0155-7's scripted serial session.

## Open questions
- Whether QEMU `virt` with HVF and with TCG both implement `SYSTEM_RESET` as a restart; measured by the first test.

## Revisit triggers
- A third power action (suspend, a firmware update): fold the calls into Option B.
- A holder that should restart but not power off, or the reverse: Option D.

## Architect's decision

Accepted as recommended, 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. Built
as TASK-18 plan step 12.
