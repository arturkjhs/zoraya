# TASK-34 report: foundations for big programs (F2 to F8)

## Status for the architect

Updated 2026-11-16.

- Merged: F2 (merge b2fb7c55, tagged `stage-f2`), F3 (db938461) and F4 (2955dcea). **`stage-f3` and `stage-f4` are not tagged**: CLAUDE.md needs a green scheduled full run of `main` that includes the commit (`tools/ci-last-full.rb --green-for SHA`); the `w9-term` red scheduled runs are TASK-31's to fix and the stage tags wait for them (architect, 2026-11-15). `stage-f2` was tagged before that rule. F1 (`poll`) was merged before this task.
- In progress: F8 (the 2 GiB guest), then F5, F6, F7.
- ADRs: ADR-0430 (item 7), ADR-0433 and ADR-0435 were accepted by the architect on 2026-11-15 and recorded; sockets surviving `exec` (`execve` with a Unix socket open is `EBADF`) are in the backlog. ADR-0431 (a `SHARED` bit in `memory_map`, kernel ABI), ADR-0432 (memory descriptors as built) and ADR-0434 (netd `Duplicate` makes a connection of the caller's own rights, bounded to 8 live per family) are **proposed**, each recommended as written; ADR-0431 first, as a kernel ABI change. Numbers 0436 to 0449 are free.
- Machine: MacBook Air M1, 8 GB, so `CARGO_BUILD_JOBS=2`, one part with builds and one QEMU guest at a time.

## Parts

| Part | State | Tag | Time | Model |
|------|-------|-----|------|-------|
| F2 | merged | stage-f2 | about 3 sessions | Sonnet implementation; Opus review (1 error, 2 warnings, all fixed) and re-check; candidate run 43681721034 green (75 jobs) |
| F3 | merged | stage-f3 pending a green full run | about 3 sessions | Sonnet implementation; Opus review (1 error, 3 warnings, all fixed) and re-check; candidate run 43700119735 green after one rerun of the `w9-term` TCG job |
| F4 | merged | stage-f4 pending a green full run | about 4 sessions | Sonnet implementation; Opus review (3 errors, 4 warnings), re-check (2 errors) and re-check 2, all fixed; candidate run 43732969721 green on the 3rd candidate after one rerun of the `test-tour-terminal` TCG job |
| F8 | in progress on `work/f8`, not merged | | one session | Sonnet; review not yet done |
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

## F4 notes

- Review: `docs/tasks/TASK-34-review-f4.md`. Errors found and fixed: `addopen` gave the child the wrong file (parent fd read as a child fd); a spawn with no attributes moved inherited Unix sockets (now only a socket the file actions list moves, and a stream socket moves whole); netd `Duplicate` had no bound, and the first bound reset when the asker closed (now a family token, 8 live per family); a refused spawn leaked entry handles (one `abandon()` closes them on every refusal path).
- Closes what F2 and F3 left: a Unix socket and a memory descriptor now cross `spawn` (`t_spawn`, `t_memfd` as real two-process tests); INV-0412-1, INV-0413-2 and INV-0432-2 are `checked`.
- CI effects found: `nvi`'s compile log lost a line to interleaved `sh -x` traces (one whole log line per compile now); `posix_spawn_file_actions_addclose` was outside the tier list, so CPython's `posixmodule.c` failed to link (it joins ADR-0433's surface block). The python lanes (`test-python`, `-ext`, `-regr`) pass with the new `subprocess` path.
- The `test-tour-terminal` TCG job (`posix-curses.script`, new on `main`, one earlier TCG pass) hung at step 55 in the first run and passed on rerun; the slow runner (about 2x the frame time of the one earlier pass) and the script's fixed pauses before typing into `vi` are the likely cause, not F4. **Follow-up for stage 10:** wait for `vi`'s screen with a `screen` step after `vi notes.txt` instead of fixed pauses.
- Known limitations (ADR-0433): a child with no cwd reads relative paths from `/`; `GRANT_WINDOW` moves, it does not share; the positive network grants are tested only over a fake netd on the host (no in-QEMU program holds a connection); a connection that holds the `narrow` right can still make connections without limit through `Narrow` (older than F4, program connections lack the right); `execve` with a Unix socket open is `EBADF`.
- Mutation: `make mutants-host CRATE=zoraya-posix --in-diff` 160 mutants, 26 missed in the first run; the holes were closed with tests, the equivalent ones (disjoint-bit `|` to `^`) remain.
- Process note: the F4 implementer's final reports were refused by the harness three times; I read its work from the branch and commits, and I made the ADR-0433/0435 decision transcription myself.

## F8 notes

- Built: `FrameAllocator::alloc_counted` and `alloc_scan_bound` (a single-frame scan reads at most one word per 256 KiB, 8192 at 2 GiB; the wrap-around no longer rereads the start region whole); `crates/zoraya-pmm/tests/two_gib.rs` (counts, a full drain, exact worst case, run search in bounded steps, 128 MiB to 4 GiB in one and two regions above 4 GiB); the test kernel's `the_worst_single_allocation_is_bounded_by_the_machines_ram`; `make run-browser` (`run-gui` at `--mem $(BROWSER_MEM)`, 2G; `make run` unchanged); `tests/shell/browser-guest.script` run by `make test-keeper` at `--mem 2G` (HVF, TCG on the full set). Proposed ADR-0436 (recommended as written; **deviation for the architect:** `top --session` is not built, `top`'s session row is the check, and `room set --mem 1800M` is refused above the session's 1452930K). Nothing in the kernel assumed a RAM size (boot tables map 4 GiB, direct map from blocks, heap on demand); the 2 GiB steps already ran.
- Measured (MacBook Air M1, 4 CPUs, debug test kernel): worst single allocation 512 words / 6.5 us at 128 MiB; 8192 words / 94 us under HVF and 1.2 ms under TCG at 2 GiB (ADR-0270's bound 2 ms). Boot to the shell script's end: 0.26 s HVF and 0.73 to 0.80 s TCG, the same at 128M and 2G. Kernel reserved: 30000 KiB at 128 MiB, 30120 KiB at 2 GiB (frame map 8 KiB against 128 KiB). Free after boot 2066388 KiB; session ceiling 1452930K.
- Run: `cargo test -p zoraya-pmm` (17 + 5 passed); `make test-qemu-one` and `make test-qemu-auto-smp4-2g` (120 passed, 9 expected failures each); `make test-qemu-tcg-smp4-gicv3` (same, before the per-word limit replaced a fixed one; not rerun); `make test-keeper` (HVF, passed, 29 steps); `make check` (passed). Not run: the TCG keeper run (full set only), `make test-changed`.
- Mutation: host `--in-diff` on `zoraya-pmm` 15 mutants: 10 caught, 3 unviable, 1 timeout, 1 missed (the wrap-around's end bound, equivalent while a frame is free); kernel list `tools/mutants/frames-f8.toml` 2 of 2 caught.
- Limitations: a guest above 4 GiB needs a second level of the frame map (the test says so); the 1 us a word limit is generous under TCG (150 ns a word).
