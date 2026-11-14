# ADR-0362: What `std::terminate` and an uncaught exception print and return: libc++abi's message on standard error, then `abort()`

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0035, ADR-0106, ADR-0153, ADR-0360
- **Evidence:** CP-27

## Context
An exception that no handler catches, one that leaves a `noexcept` function, and an explicit `std::terminate()` call the
terminate handler. libc++abi's default handler writes one line to standard error (`libc++abi: terminating due to uncaught
exception of type std::runtime_error: WHAT`, or `libc++abi: terminating`) and calls `abort()`. In libposix `abort()` raises
`SIGABRT`, whose default action ends the process (ADR-0106); the exit record carries the signal (ADR-0035). Measured (`make
test-cxx`): through the shell the line is shown and the shell says `cxxtest: ended by signal 6`; `status` says `ended by signal 6`;
through `posixrun`, `posixrun: ended Terminated detail 6`.

## Decision drivers
1. A program's failure is seen as a failure, with its reason, by the shell and by a parent's `waitpid` (ADR-0153).
2. What C++ programs and their test suites expect: `WIFSIGNALED` with `SIGABRT` (GoogleTest's death tests match on it).
3. No change to the libraries where their behaviour already fits (CP-27).

## Considered options
### Option A: libc++abi's default handler: the message, then `abort()` (the recommendation)
- **Evidence for:** drivers 1 to 3; the same as Linux, macOS and the BSDs with libc++.
- **Evidence against:** the message names the type in its mangled-then-demangled form only when `what()` exists; nothing else
  (no backtrace: libunwind has no names here, ADR-0361).
- **Precedent:** every system that ships libc++abi.
### Option B: a handler that prints and calls `exit(134)` (a status, not a signal)
- **Evidence against:** a parent sees a normal exit with an odd status; death tests and `WIFSIGNALED` checks fail (driver 2).
### Option C: a silent abort (`LIBCXXABI_SILENT_TERMINATE`)
- **Evidence against:** the reason is lost (driver 1).
### Option D: a backtrace on standard error (Fuchsia's crash reporter does it from outside the process)
- **Evidence against:** needs symbol names in the program or a crash service; neither exists. Fuchsia's way (alternatives-postmortem:
  its exception channel) would be a system service, not a library choice.

## Recommendation
**Recommendation (not a decision): Option A**, as built: the line on standard error, then `abort()`, so the shell reports
"ended by signal 6" and a parent's `waitpid` sees `WIFSIGNALED` with `SIGABRT`. A hardening check of libc++ (ADR-0360) is a
`brk` trap instead, which the shell reports as "crashed (breakpoint, class 0x3c)".

## Consequences
- **Positive.** The same failure as elsewhere; tests that expect `SIGABRT` pass.
- **Negative.** No backtrace.
- **Follow-up.** None.

## Invariants
- **INV-0362-1.** An uncaught exception prints libc++abi's line with the type and `what()`, and the program ends by `SIGABRT`.
  *Check:* `make test-cxx` (`cxxtest uncaught`, `noexcept`, `terminate`, then `status`).

## Open questions
- None.

## Revisit triggers
- A crash service that can print a backtrace of a POSIX program.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
