# TASK-34 report: foundations for big programs (F2 to F8)

## Status for the architect

Updated 2026-11-16.

- Merged and tagged: F2 (`stage-f2`, merge b2fb7c55). F1 (`poll`) was merged before this task.
- In progress: F3 (shared memory) is next.
- Proposed ADRs: ADR-0430, Unix sockets as built; recommended as written. **Needs your decision:** item 7 defers ADR-0412 item 6 (a Unix socket crossing `spawn`) to F4, which deviates from the task's assignment of item 6 to F2; INV-0412-1 is `not yet applicable (until F4)` for its two-process in-QEMU half. Numbers 0431 to 0449 are free.
- Machine: MacBook Air M1, 8 GB, so `CARGO_BUILD_JOBS=2`, one part with builds and one QEMU guest at a time.

## Parts

| Part | State | Tag | Time | Model |
|------|-------|-----|------|-------|
| F2 | merged | stage-f2 | about 3 sessions | Sonnet implementation; Opus review (1 error, 2 warnings, all fixed) and re-check; candidate run 43681721034 green (75 jobs) |
| F3 | not started | | | |
| F4 | not started | | | |
| F8 | not started | | | |
| F5 | not started | | | |
| F6 | not started | | | |
| F7 | not started | | | |

## F2 notes

- Review: `docs/tasks/TASK-34-review-f2.md`. The error (a shared stream socket sent from a fork parent was lost) and the fork-child handle leak are fixed with host tests.
- CI found one more effect: `usleep` in libposix made wget's configure use `sleep`, which libc lacks. wget is configured with `ac_cv_func_usleep=no` and curl with `--disable-socketpair`, in `ports/Makefile`.
- Known limitations are in ADR-0430: no `bind`/`listen`/`accept` (F7); a packet socket cannot be half-closed; the send-then-peer-closes window can lose a moved socket. Guest console output stalled with `-v` or `-W` in regrtest; not diagnosed.
- I tagged `candidate/work-f2-b` on the same commit as the green run; its duplicate run is not needed.
- Files outside the task's scope: `ports/` (wget, curl, the CPython port), `tools/libposix-widths.txt`, ADR-0225 and ADR-0342 notes.
