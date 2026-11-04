# ADR-0213: Corrections and statements from the review of stage 6e

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0110, ADR-0210, ADR-0211, ADR-0212
- **Evidence:** task-02b/parallel-surface.md#interruption-what-happens-to-the-build-w9, task-02/method.md#m1-libc-api-surface

## Context

TASK-22's independent review (`docs/tasks/TASK-22-review.md`) found that W9's checks passed without exercising
what they check, and that some of the harness's behaviour was not written down. ADR-0110, ADR-0211 and ADR-0212 are
accepted, so the changes and statements go here rather than into them.

- **W9's interruption point.** ADR-0110 item 4 interrupts W8 at `-j4` after the measurement's middle delay, 1.5 s,
  with make's output to `/dev/null`. On Linux 1.5 s was the middle of a 4 s build: 12 of 32 tables done, four partial
  targets deleted (`task-02b/parallel-surface.md#interruption-what-happens-to-the-build-w9`). Here, on one CPU, 1.5 s
  is before the first table under HVF and TCG alike: the first runs showed make still compiling `mktab`, so
  `W9-PARTIAL 0` would also pass if make never deleted an interrupted target.
- **INV-0212-1** says "`make test` stays within CI's `timeout-minutes` with `make test-posix` in it"; after
  ADR-0212's Decision, CI's `make test` leaves `test-posix` out and a second job runs it.

## Decision drivers

1. **A check must be able to fail** (ADR-0001 rule 6): W9's must see a partial target made and deleted.
2. **The measured point, not the measured delay** (ADR-0110 driver 6): what TASK-02b's 1.5 s reached is a build with
   tables being written.
3. **No behaviour left implicit** (CLAUDE.md).

## Considered options

### Option A: interrupt on progress (as built)

- **Description:** W9's Ctrl-C after make's output shows the leaves' tables under way (`./mktab 2 > t2.h`), make's
  output on the terminal as `w9_session.py` had it; W9's SIGTERM once a table exists. Both then require a "Deleting
  file" line, and the check prints the complete tables, as `w9_session.py` reported them.
- **Evidence for:** Drivers 1 and 2: the run is in the state the measurement's middle delay reached.
- **Evidence against:** The interruption point now depends on make's output; the runner's 300 ms quiet step before
  Ctrl-C is host time.

### Option B: a per-accelerator delay table (ADR-0110's open question)

- **Evidence against:** The point reached still varies with the host's load; a delay that suits the M4 under HVF does
  not suit CI under TCG.

### Option C: a timing-independent W9 (ADR-0110's deferred FIFO barrier)

- **Evidence against:** Changes W8's makefiles, which ADR-0110 keeps as measured.

## Recommendation

**Recommendation (not a decision): Option A**, and the statements below.

1. **W9** as Option A says (`tests/posix/w9-int.script`, `w9-term.script`, `/tests/w9-check.sh`).
2. **INV-0212-1** reads: "`make test` fits CI's `timeout-minutes` in the test job, and `make test-posix` in the posix
   job." (INV-0213-1 below, which replaces it once accepted.)
3. **Statements of what the harness does differently from the measurement**, beyond ADR-0210 and ADR-0211:
   - W1 does not run TASK-02's `W1-make-pdpmake`, a comparison run that fails on Linux (pdpmake cannot build tcc,
     ADR-0005); ADR-0110 item 2 does not list it.
   - W5's quiet step before Ctrl-C is `~ 1000` (one second, as `w5_session.py`'s `sleep(1.0)`), not ADR-0110 item 3's
     `~ 500`.
   - W10's check is TASK-02b's diff and, beyond it, both background statuses 0 and eight lines a run.
4. **What the call log counts** (ADR-0211 item 1), beyond TASK-02's interposer:
   - The union joins every process's report, the workloads' products (`lua`, `sqlite3`, `mktab`, stage-2 tcc) and the
     test aid `t_posix live` included, which TASK-02 did not count (`built:`, `task-02/method.md#m1-libc-api-surface`);
     a measured function only these call could hide a tool that no longer does.
   - A function pointer a program passes and the library calls (`qsort(..., strcmp)`) is counted here; the
     interposer did not see it.
   - A wrapper is a strong global definition even where musl's was weak, and the library's own references go to the
     real definition: a program that defines a libc function of its own does not change the library's internal
     calls to it, and one that defines a strong name the library also defines links instead of failing. No measured
     program does either.
5. **Counters:** `broker` counts a process's round trips before its report, so the report itself and an `exec`'s
   `ExecCommit` after it are not in it (about two an `exec` and one an exit fewer than `BROKER-STATS`);
   `peak_tmp_bytes` includes the temporary volume's own blocks, so it is the volume's peak use, not the files'.
6. **The call log's check** fails when a measured name is not logged at all, matches each run's logs by their exact
   names, and warns of an exemption for a function that is called. Each log carries the hash of the call log's
   order: `calllog` puts the FNV-1a hash of `zoraya-calls.txt` into `libc.a` (`__zoraya_calls_list`), every process
   reports it as `max_calls_list`, and `posix-stats --calls` refuses a log whose hash is not the hash of the list it
   decodes with (a log of another `libc.a` would decode to other names).

## Consequences

- **Positive.** W9 shows the deletion of partial targets on every run; the statements above are written.
- **Negative.** W9's point depends on make's output; one more step reads the build log.

## Invariants

- **INV-0213-1.** `make test` fits CI's `timeout-minutes` in the test job, and `make test-posix` in the posix job.
  *Check:* CI itself.
- **INV-0213-2.** W9 interrupts a build with tables being written, sees at least one target deleted, and then no
  partial target and no process left. *Check:* `make test-posix`'s `w9-int.script` and `w9-term.script`.

## Open questions

- None beyond ADR-0110's.

## Revisit triggers

- A W9 run that sees no deletion (the build already past every table when interrupted).

## Architect's decision
