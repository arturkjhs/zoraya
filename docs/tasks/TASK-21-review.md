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

`make mutants-host` (cargo-mutants 27.1.0, under the watchdog), sorted by the `mutant-triage` subagent; the script's
re-run is the judge.

| Crate | First run: caught, missed, unviable | Holes / equivalent | After the new tests |
|---|---|---|---|
| `zoraya-tty` | 179, 82, 22 (of 283) | 79 / 3 | 257 caught, 3 missed: the 3 equivalent (`discipline.rs:192:49` ×2, the erase branch only differs in echo it does not show with ECHOE off; `263:17 <` → `<=`, the one byte it changes takes the same branch) |
| `zoraya-broker` | 209, 52, 118 (of 379, 35 min) | 48 / 4 | re-run after the eleven tests (architect, 2026-11-04): 255 caught, 6 missed, 118 unviable (35 min); 3 holes closed by `a_reap_gives_the_ending_request_and_a_retired_end_is_counted` (a re-run of the six: 5 caught), 3 equivalent, rechecked below |
| `zoraya-posix` `signals.rs`, `wait.rs` | 115, 15, 3 (of 133) | 12 / 3 | 127 caught, 3 missed: the 3 equivalent (`signals.rs:41` and `wait.rs:20`, `\|` → `^` on disjoint bits; `wait.rs:30`, the wildcard gives the same 11) |

The broker's second run. Survivors, each rechecked by the main model, not the triage subagent:

- `lib.rs:381:37` `+=` → `*=` (the refusal count of a retired end): a hole, the count stayed 0; killed by the new
  test, which counts three refusals after `exec_commit`.
- `lib.rs:480:73` `+` → `-` and `+` → `*` (`Reaped.kind` = `RequestKind::raw() + 1`): holes, no test ended a process
  by a request; killed by the new test (Interrupt 1, Terminate 2, Quit 4).
- `lib.rs:668:9` `client_child` → `false`: equivalent. Its one caller (line 661) asks it only when the record's
  parent is `BROKER_PID`; a client's child is recorded with parent 0 (line 842), and `orphan`, the only code that sets
  the parent to `BROKER_PID`, clears `client_child` in the same step (line 676), so the call is false wherever reached.
- `session.rs:30:32` `*` → `+`: equivalent. `MAX_SIZE` is read nowhere in the workspace (`grep`: its definition
  only), so its value changes nothing.
- `session.rs:30:32` `*` → `/`: equivalent, for the same reason.

The broker's holes of the first run were the counts, the refusals' handle closing, `sid_of`, `in_session`, the stop records from the
spawner and across `exec`, `commit` and `abort`'s refusals, a reap that waits for the whole job, a group and the
terminal's foreground going with their last member, `kill`'s bounds, `suspend`'s members, and a full queue's
`EAGAIN`. The tests are `statistics_count_by_ordinal_and_refusals`, `the_session_id_and_the_parent_are_reported`,
`stop_records_come_from_the_spawner_and_survive_exec_when_ignored`, `commit_abort_and_reap_keep_to_their_rules`,
`a_group_and_the_terminal_go_with_its_last_member`, `kill_bounds_suspend_members_and_a_full_queue` and
`a_first_process_that_did_not_start_leaves_no_pid`. The rest of `zoraya-posix` (`jobs.rs`, the C boundary) runs only
in QEMU and was not mutated (as in TASK-20, `ffi` is target-only).

## The check of the changed lines

Of the fixes (`645ee6a`): the broker's group resume and orphan removal, the terminal read's kept bytes and stash, the
exec and commit error paths, and `ttyd`'s queued output read correctly. One more found and fixed (`6890931`): a closed
terminal fd left its held read and kept bytes in the process's lists. No further round was needed.
