# TASK-21: Stage 6d — processes, the broker, the terminal, `/tmp`

## Where and how this runs

Locally on the architect's Mac mini, **in its own git worktree**. One build-and-test agent at a time on this machine; follow the memory limits in `CLAUDE.md` and `docs/backlog.md`.

## Precondition

`main` contains stage 6c (TASK-20), its ADRs are accepted, and CI is green on `main`. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-6/plan.md` (row 6d) and `inputs.md`, `docs/tasks/TASK-20-review.md`, the measured event timelines in `docs/research/task-02b/event-timelines.md`.

Read **in full**: ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0046, ADR-0096, ADR-0101 to ADR-0107, ADR-0130, ADR-0142, ADR-0153, ADR-0155, ADR-0158, and TASK-20's accepted ADRs. Note the changes the shell decisions made to ADR-0104 (a mode with no first shell, a client connection for `zosh`, a `Group(job)` form of `spawn_prepare`) and ADR-0105 item 9.

This task builds row 6d of `plan.md`: the process half of `libposix` and the services POSIX programs need. After it, dash from 6b runs interactively and `make` runs recipes through `/bin/sh`.

ADR numbers **0190 to 0199** are reserved for this task. Where an accepted ADR is silent or wrong once implemented, write a proposed ADR or a proposed superseding one; never fill a gap silently.

## What to build

As the accepted ADRs specify:

- **The broker (ADR-0104)** with `zoraya-proto-posix`: pids, process groups as sub-jobs, sessions, `spawn_prepare`/`commit`/`reap`, `exec` keeping the pid through job `TERMINATED`, `kill` as `job_signal`, including the modes and the `Group(job)` form the shell decisions added.
- **The terminal service `ttyd` (ADR-0105)** with `zoraya-tty` and `zoraya-proto-tty`: line discipline, foreground job, Ctrl-C and Ctrl-Z through `job_signal` and `job_suspend`, `tcsetpgrp` and related calls. Backends: the serial console, and the stage 5 GUI terminal window if ADR-0105 provides for it (if it does not, write a proposed ADR for it — dash in the GUI window is the visible goal of this stage).
- **The temporary directory service (ADR-0107):** `fsd --memory` over a `MemoryDevice`, per session, with the FIFO nodes and the three FIFO open behaviours GNU make relies on (INV-0006-9).
- **One time base for every volume.** Today a volume's times are its own latest stored time plus seconds since boot (ADR-0080, ADR-0014 rule 3), and `CLOCK_REALTIME` in `libposix` follows the root volume's (ADR-0180). A memory volume has no stored time, so `/tmp` would start near zero and its files would look decades older than files in `/home`: `make` comparing a target in one with a prerequisite in the other would decide wrongly. Every `fsd` instance and `libposix` must take time from one source, never lower than any time already assigned on any mounted volume. Write a proposed ADR for where that source lives (for example `init` passing the root volume's base to every service it starts), with an invariant and a test: a file written in `/tmp` after a file in `/home` has the later `mtime`, and `clock_gettime(CLOCK_REALTIME)` is never below either.
- **`init`'s session path** with `/etc/zoraya/session` as `plan.md` describes.
- **The process half of `libposix`**, replacing the remaining ENOSYS stubs of ADR-0140: `posix_spawn` (with ADR-0142's `POSIX_SPAWN_TCSETPGRP` and `zoraya_fork_group`), `execve` with `#!` scripts (ADR-0108), `fork` through `snapshot_process` and the move list (ADR-0102), `wait*`, `kill`, `setpgid`, `tcsetpgrp`, signals delivered at wait points (ADR-0106), `nanosleep`, `pselect`, `sigsuspend`.
- dash's patches 0002 and 0003 from 6b get their first real run here; fix them if the broker shows they are wrong, with the reason in the commit.

## Tests

- **The `plan.md` criterion:** dash from 6b runs interactively on the serial console — pipes, redirections, background jobs, and Ctrl-C of a foreground `sleep` give `status=130`; `make` runs a two-rule makefile through `/bin/sh`.
- **The same dash session in the GUI terminal window**, checked with the stage 5 screendump-to-text test, if the window backend is built.
- The W9 and W10 event sequences from TASK-02b as scripted scenarios: Ctrl-C reaching the whole foreground job at once, each `make` deleting its partial target and re-raising; background jobs with `wait`; a pipeline whose writer gets `PeerClosed`.
- Every invariant `plan.md` lists for 6d (INV-0102-1, -2, INV-0103-2, -4, INV-0104-1 to -5, INV-0105-1 to -5, INV-0106-1 to -5, INV-0107-4, -5) plus INV-0006-7 and INV-0006-9; statuses in `invariants.md`.
- The four risks `plan.md` names, each with a recorded outcome: the `exec` handover through job `TERMINATED`; `SIGCHLD` through process jobs; the FIFO end-of-file rule; `job_suspend` when Ctrl-Z is pressed.
- Hostile cases: a process that ignores every catchable signal (Ctrl-C must still end the job through the documented path, `job_kill` always works); a fork bomb in a sub-job (bounded by what exists today; record what ADR-0056 will have to add); malformed broker and terminal messages. Nothing crashes the broker, `ttyd` or the kernel.
- Measurements under HVF and TCG: the broker's `spawn_prepare`/`commit`/`reap` round trips, the time of an `exec`, the time of a `fork` of dash.

`make test` passes from a clean clone in every configuration (with the documented EL2 skip on Macs that lack it), and CI is green after the merge.

## Process

Follow `CLAUDE.md`: code and the independent review on the main model; test runs through the scripts; mutant triage through the subagent with the script as the final judge, under the memory limits. Independent review by a subagent that did not write the code, focused on signal and process-lifetime races, the broker's state across `exec` and `fork`, and `unsafe`; findings and dispositions in `docs/tasks/TASK-21-review.md`; one check of the changed lines; at most one more round, then report.

## Git

Branch `stage-6d` from the latest `main`, in its own worktree. Configured git identity only. Commit after each step. **Never merge, never push.** The stage tag goes on only after CI is green on `main`.

## Definition of done

- The broker, `ttyd`, the temporary directory service and the process half of `libposix` work as the accepted ADRs specify.
- The criterion tests, the W9/W10 scenarios, the listed checks and measurements exist; `make test` passes from a clean clone.
- Review and the check of its fixes done and addressed. Final report with measurements, invariant delta, proposed ADRs, open questions, and the exact commands to try dash by hand on the console and in the GUI.

## Out of scope

The acceptance workloads W1–W3 and their harness (6e); `zosh` running POSIX programs (TASK-18 Part 2, after this task); T2; threads; exception channels; ADR-0056 quotas.
