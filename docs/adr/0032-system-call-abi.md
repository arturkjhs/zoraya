# ADR-0032: System call ABI

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0002, ADR-0004, ADR-0006, ADR-0023
- **Evidence:** CP-01, CP-11, CP-12, CP-13, CP-22, CP-25, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context
Stage 2's criterion is a user process printing through a native system call. ADR-0002's INV-0002-1 requires that every object is named by a `Handle`, and its checks over the `abi` crate become possible with this stage, with those of INV-0004-1 and INV-0006-2. Stage 3 adds handles, ports and `spawn`, so this ABI starts small and has to grow without breaking (CP-11, CP-13).

## Decision drivers
1. Objects are named by handles, with no ambient authority (CP-01, INV-0002-1).
2. Errors are typed values at the call site (CP-12).
3. The boundary is one crate, checkable mechanically (CP-13).
4. Unknown calls and flags fail, so that the ABI can grow (CP-11).

## Considered options

### Option A: Stable Linux-style syscall numbers with `-errno`
- **Description:** Register convention as on Linux arm64: number in x8, arguments in x0 to x5, a negative errno in x0.
- **Evidence for:** Well known; tools understand it [outside evidence: Linux `Documentation/arch/arm64/`].
- **Evidence against:** The status shares a register with the value, which is what CP-12 tries to avoid. A global console `write` by descriptor number would break driver 1.
- **Cost:** Low.
- **Precedent:** Linux.

### Option B: Status in x0, values in out-registers, typed crate
- **Description:** Same trap and argument registers. x0 returns a typed status, x1 a value. The `zoraya-abi` crate defines the numbers, `Status`, `Handle` and a table of signatures that tests read.
- **Evidence for:** Meets all four drivers. Status never mixes with a value. Zircon also returns a status and passes results out separately (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon syscall docs].
- **Evidence against:** Needs a user library for every call. A fixed number of registers forces structs for larger calls.
- **Cost:** Low (estimate: 200 lines).
- **Precedent:** Zircon.

### Option C: One invocation call on capabilities
- **Description:** seL4 has few system calls, and most operations are invocations of a capability with a message [outside evidence: seL4 reference manual].
- **Evidence for:** A very small trap surface, with authority always explicit.
- **Evidence against:** Fixes the message format before stage 3 designs it.
- **Cost:** Moderate.
- **Precedent:** seL4.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Trap.** `svc #0`; any other immediate returns `BadSyscall`. Number in x8, arguments in x0 to x5. Return: x0 is `Status` (0 is success), x1 a value. x2 to x18 are preserved, unlike SMCCC, so the wrappers stay simple.
- **`Handle`.** A `u32` index into a per-process table, with 0 never valid. At this stage a process starts with a console handle in a fixed slot, and the table is otherwise fixed; stage 3 replaces this with rights and duplication.
- **Time.** `Deadline` goes over the ABI as a `u64` nanosecond instant, with `u64::MAX` meaning `Infinite`, as ADR-0006 rule 4 fixes.
- **First calls.**

| Number | Call | Arguments | Result |
|---|---|---|---|
| 0 | `process_exit` | `code: i32` | does not return |
| 1 | `thread_yield` | none | `Ok` |
| 2 | `console_write` | `handle`, `buffer` | bytes written |
| 3 | `clock_monotonic` | none | `Instant` nanoseconds |
| 4 | `sleep_until` | `deadline` | `Ok` |

- **`console_write`** names its console by handle, so a process with no console handle cannot print (driver 1). Bytes must be valid UTF-8 (CP-22) or the call returns `InvalidArgument`.
- **`Status`.** `Ok`, `BadSyscall`, `BadHandle`, `BadAddress`, `InvalidArgument`, `NoMemory`. Unknown numbers return `BadSyscall`.
- **Checks.** `zoraya-abi` holds `SYSCALLS`, a const table of each call's parameter kinds. Unit tests implement the lints of INV-0004-1 and INV-0006-2 on it. That table is written by hand and is not connected to the dispatcher. INV-0002-1's lint is partial: there is no `RelativePath` yet, and `process_exit`'s `code` is an integer outside that invariant's allow-list, as ADR-0004 rule 1 requires. ADR-0002's open questions record both.
- **Failure policy.** System call paths use fallible allocation (ADR-0022).

## Consequences
- **Positive.** The three existing invariant checks stop being "not yet applicable". Status and value cannot be confused. ADR-0006's `Deadline` reaches user space from the first blocking call.
- **Negative.** `console_write` is a stage-2 stand-in: a real terminal is a service (CP-25) and this call should go. Numbers are not stable yet.
- **Follow-up.** Rights on handles, `spawn` and ports in stage 3, which may renumber everything: nothing outside this repository depends on the numbers.

## Invariants
- **INV-0032-1.** Every call in `SYSCALLS` has a number that the kernel's dispatcher handles, and any other number returns `BadSyscall`. *Check:* `zoraya-abi` unit test, and an in-QEMU test with an unknown number.
- **INV-0032-2.** A system call never panics the kernel on any argument values. *Check:* an in-QEMU test where a user program makes every call except `process_exit` with random register values, with `sleep_until` given near deadlines.

## Open questions
- How a struct-passing convention should look for `spawn`, which has too many parameters for registers.
- Whether `console_write` survives past the terminal service.

## Revisit triggers
- A call that needs more than six arguments.
- The stage 3 handle design requiring a different `Handle` width.

## Architect's decision
