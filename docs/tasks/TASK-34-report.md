# TASK-34 report: foundations for big programs (F2 to F8)

## Status for the architect

Updated 2026-11-16.

- Merged and tagged: F2 (`stage-f2`, merge b2fb7c55). F3 merged (db938461) and **not tagged yet**: CLAUDE.md now needs a green scheduled full run of `main` that includes the commit before a stage tag (`tools/ci-last-full.rb --green-for SHA`); I tag `stage-f3` when it names one. `stage-f2` was tagged before that rule and has no such run yet. F1 (`poll`) was merged before this task.
- In progress: F4 (`posix_spawn` with what each child is given).
- Proposed ADRs: ADR-0430, Unix sockets as built; recommended as written. **Needs your decision:** item 7 defers ADR-0412 item 6 (a Unix socket crossing `spawn`) to F4, which deviates from the task's assignment of item 6 to F2; INV-0412-1 is `not yet applicable (until F4)` for its two-process in-QEMU half. ADR-0431 (a `SHARED` mapping stays shared across `snapshot_process`, kernel ABI) and ADR-0432 (memory descriptors as built), both proposed, recommended as written (F3). Numbers 0433 to 0449 are free.
- Machine: MacBook Air M1, 8 GB, so `CARGO_BUILD_JOBS=2`, one part with builds and one QEMU guest at a time.

## Parts

| Part | State | Tag | Time | Model |
|------|-------|-----|------|-------|
| F2 | merged | stage-f2 | about 3 sessions | Sonnet implementation; Opus review (1 error, 2 warnings, all fixed) and re-check; candidate run 43681721034 green (75 jobs) |
| F3 | merged | stage-f3 pending a green full run | about 3 sessions | Sonnet implementation; Opus review (1 error, 3 warnings, all fixed) and re-check; candidate run 43700119735 green after one rerun of the `w9-term` TCG job |
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

## F3 notes

- Built: memory descriptors as ADR-0413 specifies (`crates/zoraya-posix/src/memfd.rs`), and in the kernel a `SHARED` flag of `memory_map` that
  `snapshot_process` honours (ADR-0431: a shared mapping is not copied into a `fork` child; without it a fork child's `MAP_SHARED` mapping
  was a private copy). Proposed ADR-0431 and ADR-0432 record what the accepted ADRs leave silent; notes on ADR-0100, 0102, 0103, 0107 and 0413.
- Checked: INV-0413-1 to -5 and INV-0431-1 to -3, INV-0432-1 to -3 (`docs/adr/invariants.md`; Total 809).
- What waits for F4: a memory descriptor does not cross `spawn` or `exec` (`EBADF` without `O_CLOEXEC`), so the plan's "passes a 4 MiB memfd to a
  child" is checked with a `fork` child (`t_memfd fork`) and in one process over a socketpair (`t_unix memfd`), plus two processes over a fake
  kernel; seals are per holder; the in-QEMU "in another process" half of INV-0413-2 needs F4.
- Found by the in-QEMU tests, not the host's: a channel entry for a sealed memory handle must name the rights it has (`rights_of` in `ffi.rs`).
- Ports: CPython's `_posixshmem` stays unbuilt (`shm_open` and `shm_unlink` are in `PYTHON_ABSENT`, as F2's lesson); no other configure
  result changes (grep of `ports/` for `shm_open`, `memfd`, `mmap`, `mprotect`, `PROT_EXEC`: none).
- Review: `docs/tasks/TASK-34-review-f3.md`. The error was an unbounded snapshot step (skipped shared pages were not counted, INV-0270-1); fixed with a `t_bound` case and a mutant.
- CI: `quotas_from_a_process` failed under TCG/EL2/SMP4 because F3's own new `t_quota` case was too tight for slow machines (fixed). The `w9-term` TCG job timed out in two F3 runs and also in `main`'s scheduled run 43699859694 (without F3); its step time moves between 262 s and 589 s against a 600 s runner limit. **For the architect:** raise the limit, shrink the build tree in `w9-term`, or poll less often than every 0.1 s (each poll forks `ls` under one-CPU TCG). I changed no timeout.
- Mutation: host run cut at 30 minutes (about 60 of 388 mutants never run; later re-check: 14 of the 20 misses in new code are equivalent or untestable); 7 kernel hand mutants all caught. The never-run host mutants were not run afterwards (about 35 minutes).
- The merge into `main` had conflicts in `Makefile`, `ports/Makefile` and `invariants.md` (Total 820 recomputed from rows); the combined `t_tty` plus `t_memfd` lines were not run in CI before the merge.
