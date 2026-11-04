# TASK-22: Stage 6e — acceptance and measurement of the POSIX personality

## Where and how this runs

Locally on the architect's Mac mini, **in its own git worktree**. One build-and-test agent at a time on this machine; follow the memory limits in `CLAUDE.md` and `docs/backlog.md`. Free disk space is low (about 11 GB): remove what you create and do not need, and say in the report how much the task used.

## Precondition

`main` contains stage 6d (tag `stage-6d`) and CI is green on it. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-6/plan.md` (row 6e and the closing paragraphs), `docs/tasks/TASK-21-review.md`, and TASK-21's final report as recorded in the repository.

Read **in full**: ADR-0005, ADR-0100, ADR-0108, ADR-0109, ADR-0110, ADR-0180, ADR-0190, ADR-0191, ADR-0192. Read the workload definitions and measured results in `docs/research/task-02/method.md`, `posix-surface.md`, and `docs/research/task-02b/` (`parallel-surface.md`, `event-timelines.md`, `research-log.md`): W1 to W5 and W8 to W10 are the measured runs you reproduce, with the same commands and the same checks.

This task builds row 6e of `plan.md`: it proves that what 6a to 6d built runs real programs, and measures it. After it, stage 6 is closed: inside Zoraya, tcc builds itself, Lua and SQLite, with GNU make, pdpmake and dash.

ADR numbers **0210 to 0219** are reserved for this task. Where an accepted ADR is silent or wrong once implemented, write a proposed ADR or a proposed superseding one; never fill a gap silently.

## Start from what 6d built

6d already made `make posix-image`, `make test-posix` and `make test-posix-gui`, with scripts under `tests/posix-session/`; ADR-0110 names `tests/posix/` and `build/posix-disk.img`. Extend what exists rather than building a second harness. If the layout 6d chose differs from ADR-0110 and is the better one, keep it and record the difference in a proposed ADR; otherwise move it to ADR-0110's layout in its own commit.

## What to build

As ADR-0110 specifies (its items 1 to 12):

- **The image and the scripts:** the sources of tcc, Lua and SQLite in the image as `ports/sources.lock` pins them; `/tests/w1.sh` to `/tests/w3.sh` with the measured commands and checks; W2 once with pdpmake and once with GNU make; W2-j and W3-j.
- **The roadmap criterion:** `tcc hello.c && ./a.out` inside Zoraya (ADR-0110 item 5).
- **Two targets:** `make test-posix` (in `make test`) runs W5, W8, W9, W10 and the criterion; `make test-posix-long` (outside it) runs W1, W2, W3, W2-j and W3-j. W5 and W9 may already exist from 6d; make them the ADR's scripts, not a second copy.
- **Observation:** the `stats` feature of `zoraya-posix` and `tools/posix-stats`; INV-0005-3's call-log check, the union over every run compared with TASK-02's measured surface; the export check as the first step of `make test-posix` (INV-0005-1, -2).
- **The W4 measurement** (`make measure-w4`, not in `make test`): GNU hello's real `configure` and `make`, with GNU make and pdpmake; the report of what failed and why, for the T2 decision.
- **The W5 extension** (ADR-0110 item 12): Ctrl-Z, `fg`, `bg`.
- **Documentation:** `docs/design.md`'s stage 6 sections rewritten for a reader who uses the system, with the measured table; the commands to run each workload by hand; the backlog.

## Things known to bite

- **`exec` costs 25 ms under HVF and 195 ms under TCG**, almost all of it reading the program through the file protocol each time (TASK-21). `configure` and `make` exec hundreds of times. `plan.md` puts an image cache out of stage 6 with a revisit trigger. Measure first: report the exec count and the share of wall time spent in `exec` for every workload. If `make test-posix` does not fit CI's time budget under TCG because of it, stop and report with the numbers and a proposed ADR (image cache, or `image_create` reading from a file connection); do not build it without a decision.
- **Run time under TCG against CI's budget** (`plan.md`'s risk): measure each workload under HVF and TCG before deciding what goes into `make test`.
- **Whatever the real tcc self-build finds in `libposix`.** Every fix to 6c or 6d code gets its own commit with a test that fails without it, and its reason in the commit message.
- **The TCG key-burst loss** in the GUI (backlog): W5 and W9 run on the serial console, not the GUI; do not route them through the GUI to work around it, and do not try to fix it here.
- **ADR-0106's stated deviation** (a blocked `SIG_DFL` terminating signal ends the process at once) stays. If a workload breaks on it, report the workload and the call site; do not change the rule.

## Tests and measurements

- `make test` passes with `make test-posix` in it, under HVF and TCG, from a clean clone (with the documented EL2 skip on Macs that lack it).
- `make test-posix-long` passes with W1, W2, W3, W2-j, W3-j, and the call-log union check.
- Every invariant `plan.md` and ADR-0110 assign to 6e, statuses in `invariants.md`; INV-0005-1 to -4.
- Everything ADR-0110 item 11 lists, under HVF and TCG, next to the Linux figures from TASK-02/02b: wall time per workload, `exec` and `fork` counts and their time, broker round trips, peak live pids, peak `/tmp`, path-resolution cost on W1.
- The W4 report.

## Process

Follow `CLAUDE.md`: code and the independent review on the main model; test runs through the scripts; mutant triage through the subagent, with the main model rechecking every survivor called equivalent (one line each in the review file); memory limits. Where parts are independent (the harness and scripts, `posix-stats`, the W4 measurement), you may use subagents as `CLAUDE.md` allows; only the main agent runs QEMU and `make test`. Independent review by a subagent that did not write the code, focused on whether the workloads are the measured ones (same commands, same checks, nothing made easier), the call-log check, and every fix to 6c and 6d code; findings and dispositions in `docs/tasks/TASK-22-review.md`; one check of the changed lines; at most one more round, then report.

## Git

Branch `stage-6e` from the latest `main`, in its own worktree. Configured git identity only. Commit after each step. **Never merge, never push.**

## Definition of done

- The workloads, the two targets, the observation and the W4 measurement exist as ADR-0110 specifies; `make test` and `make test-posix-long` pass from a clean clone.
- Review and the check of its fixes done.
- Final report: the table of measurements beside Linux, the call-log union against TASK-02's surface (anything called that was never measured, and anything measured never called), the W4 report in brief with a recommendation on T2, every fix to 6c/6d code, the invariant delta, proposed ADRs, open questions, the time per step (writing, builds, host tests, QEMU runs, review and mutants), the disk space the task used, and the exact commands to run W1 to W3 by hand.

## Out of scope

T2's rows (only W4's measurement and the recommendation); an image cache and copy-on-write snapshots (measure, propose, do not build); threads; `tcc -run`; the GUI key-burst bug; `zosh` running POSIX programs (TASK-18); networking.
