# TASK-21 review: stage 6d, processes, the broker, the terminal, `/tmp`

One independent review by a subagent that wrote none of the code (on the main model, as `CLAUDE.md` asks), read-only,
of `main...stage-6d` at `4648399`, focused as the task asks on signal and process-lifetime races, the broker's state
across `exec` and `fork`, and `unsafe`. Then mutation testing of the new host crates, sorted by the `mutant-triage`
subagent; then one check of the changed lines. Fixes are in the commit after `4648399` and those named below.

## Findings and dispositions: the review

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | high | After Ctrl-Z (a `job_suspend` of the whole group job) `fg`'s `SIGCONT` resumed each member's job, so the group job stayed suspended and its next child started stopped. | Fixed: a group stopped whole is resumed whole, every member's record cleared; host test in `stop_signals_follow_each_targets_disposition`; in QEMU `scenarios.script`'s Ctrl-Z step (a stopped `sh` runs its next `sleep` after `fg`). |
| R2 | medium | A pid prepared and never committed, whose parent ended in between, was never watched or reaped, so the session could never finish. | Fixed: orphaning an uncommitted pid kills its job and removes it; host test `a_prepared_pid_whose_parent_ends_is_killed_and_removed`. |
| R3 | medium | A terminal read held across `EINTR` dropped what its answer carried past a smaller next buffer. | Fixed: the surplus is kept per connection and served first (`tty_rest`). |
| R4 | medium | Another request on a terminal connection while a read was held took the read's answer as a mismatch: `EIO`, and the line was lost. | Fixed: `call` keeps such an answer for the read (`tty_stash`) and waits on for its own. |
| R5 | medium | `exec` could return `-1` after the new image was already running in the pid's job. | Fixed: past the spawn, the old image ends whatever the commit does (127 if it failed). |
| R6 | medium | A failed `spawn_commit` leaked the child's handles (and left a `TCSETPGRP` child stopped); a too-long move list leaked every prepared handle. | Fixed: the child is resumed, its handles closed and its pid aborted; the move list's overflow cleans up as the other error paths do. |
| R7 | low-medium | The signal mask is not carried across `exec` or `posix_spawn`; `SETSIGMASK` is accepted and not applied; unstated. | Stated in ADR-0190 item 18 and design.md's limitations. |
| R8 | low | No `SIGCHLD` for a stopped child (ADR-0106 item 3's `SUSPENDED` subscription); unstated. | Stated in ADR-0190 item 19. |
| R9 | low | A background process can clone its terminal connection tagged with the foreground group and read. | Left: ADR-0105's "parent-declared group tag is the one trust gap" at stage 6, every process trusted; target 2 has the broker clone. |
| R10 | low | A held read answered `NotForeground` while in the background could give `EIO` after the group is foreground again. | Fixed: an answer to a read held from before is asked afresh once before `SIGTTIN` applies. |
| R11 | low | FIFO readers stay on the old stream after its writers come and go. | Left: ADR-0107 item 15's stated deviation; no measured program reopens a FIFO for writing. |
| R12 | low | The broker blocks without a deadline on `ttyd`'s replies, and `ttyd` could block on the window. | Fixed where it bit: `ttyd`'s window backend never blocks (the GUI test under TCG lost keys so); the control calls stay synchronous, answered at once. |
| R13 | low | `ttyd` ignores a failed reply write. | Left: a client has one request out at a time and a held read; a connection's queue holds both. |
| R14 | low | A doc comment of `is_within` was left on `posix_root`; `tmp_job` inherits `posix` from the broker's job. | Fixed the comment; `tmpd` in a `posix` job is ADR-0104 item 1's tree, harmless. |

The reviewer found no `unsafe` problem: every block has a `SAFETY` comment that matches its use (musl's struct
offsets, the `fd_set` words, the file actions' `Box`, `execl`'s registers, the handlers' `transmute`).

## The four risks of `plan.md`

- **The `exec` handover through job `TERMINATED`:** held. The new image is spawned into the pid's own job before the
  old one exits, so the job is never empty; `t_posix execwait` reaps once with the new image's 7, and a `#!` script's
  5 (INV-0104-2).
- **`SIGCHLD` through process jobs:** held for ends. dash's `wait` (sigsuspend) and make's `pselect` wake on a child's
  job `TERMINATED` (W10, W9); stops are not delivered as `SIGCHLD` (R8).
- **The FIFO end-of-file rule:** held: the service's own write end goes with the last writer (host tests); make's
  jobserver runs on its FIFO in `/tmp` through W9a and W9b.
- **`job_suspend` on Ctrl-Z:** held after R1: `ttyd` sends `suspend` to the broker, which suspends the group job
  whole; `fg` resumes it whole.

## Mutation testing

See below (filled in after the run).

## The check of the changed lines

See below.
