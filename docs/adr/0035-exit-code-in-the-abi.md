# ADR-0035: The exit code as a parameter of the native ABI

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0002, ADR-0004, ADR-0032
- **Evidence:** CP-01, CP-06, CP-13, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context
INV-0002-1 says that the only integer payloads of a native system call are `detail` fields, opaque to the kernel. ADR-0004 rule 1 gives every process an `ExitRecord { code: i32, reason, detail }`, and ADR-0032's `process_exit` passes `code` as a parameter. Both ADRs are accepted, so the invariant cannot be true of the ABI they require between them. TASK-08's review found this (R-12).

## Decision drivers
1. The point of INV-0002-1 is that objects are never named by integers (CP-01). A code that names nothing does not touch that point.
2. Exit status lives on the process object (CP-06, ADR-0004 rule 1).
3. An invariant that is false cannot be checked (ADR-0001 rule 6).
4. The ABI boundary changes as little as possible (CP-13).

## Considered options

### Option A: Allow `ExitRecord.code` by name
- **Description:** A new invariant replaces INV-0002-1, adding `ExitRecord.code` to the integer payloads, documented as opaque to the kernel.
- **Evidence for:** Matches what ADR-0004 and the code already do. No ABI change. The rule about naming objects stands unchanged (driver 1).
- **Evidence against:** One more entry on an allow-list, and each entry weakens the rule a little.
- **Cost:** A text change and the lint's list (estimate: minutes).
- **Precedent:** Zircon's process exit takes a return code that the kernel only stores (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon `zx_process_exit` docs].

### Option B: Carry the status in `detail`
- **Description:** Remove `code`. `process_exit` takes `detail: u32`, and `ExitRecord` loses a field.
- **Evidence for:** INV-0002-1 stays word for word.
- **Evidence against:** Supersedes ADR-0004 rule 1, and changes the `libposix` mapping of `WEXITSTATUS`. `detail` means something else for `Terminated` and `Fault`, so one field would carry two meanings.
- **Cost:** Moderate (estimate: ADR-0004 text, `zoraya-abi`, the kernel, tests).
- **Precedent:** None in our evidence.

### Option C: Linux baseline, a plain integer with no rule
- **Description:** `exit_group(int)` on Linux takes an integer, and no rule constrains it [outside evidence: Linux `exit_group(2)`]. Here: keep the exception in the lint only, and leave the invariant's text alone.
- **Evidence for:** No work.
- **Evidence against:** The invariant stays false, and ADR-0001 forbids deviating silently.
- **Cost:** None now, and a false `checked` later.
- **Precedent:** Linux.

## Recommendation
Recommended: **Option A**. It is a recommendation, not a decision.

- INV-0035-1 replaces INV-0002-1 when this ADR is accepted, and INV-0002-1's row is marked as replaced. Everything else in ADR-0002 and ADR-0004 stands.
- `code` is stored in `ExitRecord` and never interpreted by the kernel.
- `Param::ExitCode` in `zoraya-abi` stays, as the one named exception.
- `RelativePath` still does not exist, since no call takes a path, so the check stays `not yet applicable (stage 4)`.

## Consequences
- **Positive.** The invariant can become true. No code changes.
- **Negative.** A second replacement of accepted text in one stage, which is a cost in reading the record.
- **Follow-up.** Stage 3 connects `SYSCALLS` to the dispatcher (ADR-0032's open questions), without which this lint is weak.

## Invariants
- **INV-0035-1.** Every object a native syscall acts on is designated by a `Handle` parameter; every path parameter has the `abi` type `RelativePath`, whose constructor rejects a leading `/`; the only integer payloads are `detail` fields and `ExitRecord.code`, each documented as opaque to the kernel (ADR-0004). *Check:* a lint over the `abi` crate's syscall signatures that allows parameter types only from an allow-list (`Handle`, `RelativePath`, `Deadline`, flags, sizes, buffers, `detail: u32`, `code: i32`), plus a unit test that `RelativePath::new("/x")` fails.

## Open questions
- Whether a signal-style status needs `code` at all once `libposix` exists. ADR-0004's mapping says it does.

## Revisit triggers
- A call that needs another integer payload.

## Architect's decision
