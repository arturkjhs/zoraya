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
| R2 | medium | `POSIX-CALLS` was decoded with the current `zoraya-calls.txt`, which might not be the list of the build that wrote the log. | Fixed: `posix-stats --calls` refuses a log older than the list (`calls_pass_fail_and_refuse_as_they_should`). |
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
