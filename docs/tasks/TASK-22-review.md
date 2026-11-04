# TASK-22 review: stage 6e, acceptance and measurement of the POSIX personality

One independent review by a subagent that wrote none of the code (on the main model, as `CLAUDE.md` asks),
read-only, of `main...stage-6e` at `b23b383` and the working tree. As the task asks, it focused on whether the
workloads are the measured ones, on the call-log check, and on every fix to 6c and 6d code. Then came mutation testing
of the new host code, sorted by the `mutant-triage` subagent and rechecked by the main model, and one check of the
changed lines. The fixes are in `3d532e1` and the commits named below.

## Findings and dispositions: the review

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | high | W9's "no partial target" passed without testing it. At a fixed 1.5 s, make had not begun a table under HVF or TCG: the logs show it compiling `mktab`. | Fixed: Ctrl-C comes once make's output shows `./mktab 2 > t2.h`, SIGTERM once a table exists, and both require a "Deleting file" line; `w9-check.sh` prints the complete tables (proposed ADR-0213, INV-0213-2). Under HVF and TCG: three and three deletions on Ctrl-C, three and two on SIGTERM, then `W9-PARTIAL 0`. Found on the way: `cd X && make -j4 ... &` backgrounded a subshell, so `$!` was not the top make and SIGTERM went to the subshell. The `cd` is now a line of its own. |
| R2 | medium | `POSIX-CALLS` was decoded with the current `zoraya-calls.txt`, which might not be the list of the build that wrote the log. | Fixed: every log carries the hash of the call log's order (`max_calls_list`, from `__zoraya_calls_list` in `libc.a`), and `posix-stats --calls` refuses a log whose hash is not its list's (`calls_pass_fail_and_refuse_as_they_should`). A first fix compared file times; the check of the changed lines (C3) showed that it refused logs of an unchanged list and proved nothing of a newer one. |
| R3 | medium | `posix-w5-*.log` also matched `posix-w5-jobs-*`, so a W5-extension log alone satisfied `--require w5`. | Fixed: logs are matched by `posix-NAME-{auto,hvf,tcg}.log` in the Makefile and in `posix-stats`. |
| R4 | low | INV-0212-1's wording no longer matches CI's two jobs. | Proposed ADR-0213's INV-0213-1 rewords it; INV-0212-1's status points there until the architect decides. |
| R5 | low | `scenarios.script`, `smoke.script` and `measure.script` lacked `! -FAIL` or `! ZORAYA-FAULT`. | Fixed: every script in `tests/posix/` forbids `-FAIL`, `panicked` and `ZORAYA-FAULT`. |
| R6 | low | `W1-make-pdpmake` was dropped, and W5's quiet step is `~ 1000` against ADR-0110's `~ 500`; neither was stated. | Stated in ADR-0213 item 3. |
| R7 | low | A measured name that the call log does not wrap was silently counted as data. | Fixed: it fails (`measured_not_logged`); none today. |
| R8 | low | A wrapper changes how a program can override libc functions. | Stated in ADR-0213 item 4; no measured program overrides one. |
| R9 | low | The union joins the products' and test aids' calls, which TASK-02 did not count. | Stated in ADR-0213 item 4. |
| R10 | low | `broker` does not count the report itself, or an `exec`'s commit after it. | Stated in ADR-0213 item 5. |
| R11 | low | `peak_tmp_bytes` includes the volume's own blocks. | Stated in ADR-0213 item 5 and `docs/design.md`. |
| R12 | low | `__zoraya_calls` was declared as one byte and read as `count`. | Fixed: `[u8; 0]`. |
| R13 | low | W10's diff passes if both modes fail alike. | Fixed: both background statuses must be 0 and each run must give eight lines; stated in ADR-0213 item 3. |
| R14 | low | Stale exemptions went unnoticed; a function named `calls` would have clashed with the bitmap's file. | Fixed: stale exemptions are warned of; the bitmap is `zoraya-calls.S` (no function name has a hyphen); `calllog` refuses a definition of `__zoraya_calls`. |
| R15 | low | `getrusage(RUSAGE_THREAD)` was `EINVAL`. | Fixed: accepted (one thread), with a test in `t_posix products`. |
| R16 | process | An agent set ADR-0210 to ADR-0212 to accepted. | The architect's message of 2026-11-04 told the agent to record the three decisions, with their conditions, in these words; it is not a standing permission. |

The reviewer found no problem in the wrappers' ABI (x16 and x17 only, a tail branch), in the bit order (wrapper
index, `pack_calls`, the broker's join, the hex, the decode), in the report around exit, `exec`, `fork` and
`posix_spawn`, or in the fixes to 6c and 6d code (every new `unsafe` with its `SAFETY`, each new function tested).

## Mutation testing

`make mutants-host` (cargo-mutants 27.1.0, under the watchdog), on the stage's host code: `zoraya-broker`'s
`totals.rs`, `posix-stats`, `posix-surface-check`'s `call_log`, `zoraya-posix`'s `Stats::counter`, `encode`,
`pack_calls`, `fsync` and the fd peak, and `zoraya-fs`'s `used_blocks`. The `mutant-triage` subagent sorted the
survivors, and each it called equivalent was rechecked by the main model. The script's re-run is the judge.

| Crate (filter) | First run: caught, missed | Holes / equivalent | After the new tests |
|---|---|---|---|
| `zoraya-broker` (`totals.rs`) | 19, 3 (of 25, 3 unviable) | 1 / 2 | 20 caught, 2 missed, both equivalent |
| `posix-stats` | 86, 41 (of 127) | 40 / 1 | 131 caught, 1 missed, equivalent |
| `posix-surface-check` (`call_log`) | 9, 2 (of 11) | 2 / 0 | 13 caught, 0 missed |
| `zoraya-posix` (the stats and `fsync`) | 51, 9 in this stage's code (of 60) | 9 / 0 | the `fsync` hole killed; `restart_stats`' kept field removed (below); `ffi.rs`'s `fsync` return values are target-only, killed by `make test-posix`'s `t_posix products` |
| `zoraya-fs` (`used_blocks`, `block_count`) | 4, 5 in this stage's code | 5 / 0 | 7 caught; the 2 missed are `check.rs`'s, outside the stage |

Most holes were in `posix-stats`' command line, which had no test; `tests/cli.rs` covers it now.

Survivors, each rechecked by the main model, not the triage subagent:

- `totals.rs:40:32` `<` → `<=`: equivalent. The guarded `resize(NAMES.len(), 0)` does nothing when the length is
  already `NAMES.len()`, and nothing else reads the condition.
- `totals.rs:52:29` `<` → `<=`: equivalent, for the same reason (`resize(calls.len(), 0)` at equal length).
- `posix-stats/src/lib.rs:350:37`, the guard `!reason.trim().is_empty()` → `true`: equivalent. Every line is
  trimmed before the split, so it ends in a non-space character, and the text after its first space always holds
  one.
- `ffi.rs:841:9`, the field `inherit_failed_at` deleted from `restart_stats`: a hole, which the triage traced to a
  debug line only (`ZORAYA_POSIX_DEBUG`). Keeping the field let a later failure's debug line show an earlier,
  unrelated fd, so the field is no longer kept: the mutant is gone with the defect.

The filters also caught older code (6d's `Facts` fields in `spawn_child` and `exec`, `zoraya-fs`'s `check.rs`). Its
survivors are not this stage's and are left for the areas' next mutation runs.

## The check of the changed lines

Of the fixes (`3d532e1` to `7308b85`), the reviewer confirmed R1 to R15 and found:

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| C1 | medium | `make test-posix-long` had not run since the fixes rebuilt `libc.a`. | Done: the clean clone's run below. |
| C2 | low | W9-term's wait for a table had no bound if make died first. | Fixed: the loop also ends when make is gone (`kill -0`). |
| C3 | low | R2's check by file time refused logs of an unchanged list and proved nothing of a newer one. | Fixed: the list's hash travels with the counts (R2's row above). |
| R1 residual | low | "Deleting file" matched any target, not only a table. | Fixed: `Deleting file 't`. |
| R13 residual | low | W10's check does not compare the counts' values. | Left: it is TASK-02b's check plus statuses and line counts; a wrong count in both modes is beyond what was measured. |
| R4, R5 | low | INV-0213-1's status said checked though CI has not run the branch; `gui.script` has no `!` lines. | INV-0213-1 is `not yet applicable` until the architect decides ADR-0213; INV-0110-4's status names the serial scripts. |

No further round was needed.
