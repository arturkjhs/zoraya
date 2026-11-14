# TASK-29 audit: POSIX-session tests that can pass without testing what they claim

Read-only audit; nothing was built or run. Every statement below comes from reading the script, the
program that prints the string, and `tools/test-runner/src/script.rs`. Line numbers are of `main` at
`5e9bbabf`.

Runner semantics that matter here (`script.rs`):

- `< TEXT` matches the first TEXT after the end of the last match. The tty echoes typed keys, so the
  echo of a `>` line is part of the searched text.
- `x TEXT` looks only at `text[before..from]`: from the end of the match before the last one to the
  end of the last match. Text printed after the last match is not seen.
- `!` covers the whole output; `o` counts in the whole output.
- A failed C check ends with `exit 1`; scripts notice it only as a missing "passed" line (a timeout)
  unless a `!` line matches the FAIL text.

## Confirmed findings

### P1. scenarios.script:46-53 — "runs its next child after fg" is matched by fg's echo

- Claim (lines 30-37): "the group stopped whole runs its next child after fg (review R1); then Ctrl-C
  of the continued job".
- Script: `> fg\r`, `< first`, `< second`, `~ 500 PSURF>`, `> \x03`, `< PSURF> `, `after=130`.
- Why it passes without the behaviour: dash's `fg` prints the job's command text, which is
  `sh -c '...; sleep 5; echo first; sleep 1; echo second; sleep 30'`. Both words are in it. The script's
  own comment at lines 55-56 says so for the later rounds ("the word printed, end1, is not in the line
  fg echoes") and those rounds use `end$((0+1))` to avoid it; this round does not. So `< first` and
  `< second` match at once, Ctrl-C goes about 0.5 s after `fg`, while `sh` is still in `sleep 5`.
  A system where the continued `sh` never starts its next child (R1's bug) still gives 130.
- Confidence: CONFIRMED from the script and its own comment; dash's `fgcmd` (`outstr(jp->ps->cmd)`)
  from memory, the dash source is not on this machine.
- Fix: print words the command line does not hold, as lines 57-72 do:
  `echo fir$((0+1))`/`echo sec$((0+2))` and expect `fir1`, `sec2`; or expect `\nfirst` and `\nsecond`
  (output lines start a line, the echoed command does not).

### P2. scenarios.script:100-102 — "a write outside the limit still works" is matched by the echo

- Claim (lines 92-95): "a write outside the limit still works".
- Script: `> echo outside > /home/outside && cat /home/outside && ls /limited; echo done\r`,
  `< outside`, `< done`, `< PSURF>`.
- Both words are in the echoed command line (`echo outside`, `echo done`). If the redirect fails,
  `cat` never runs and the step still passes. `t_posix diskfill` (t_posix.c:873-928) writes nothing
  into OUT (only the `rename` that must fail), so nothing else covers it.
- Same pattern, lower value: line 90-91 `> echo still-here\r` / `< still-here` (and no prompt wait
  before the next command); line 12-13 `< y` matches the `y` of the echoed `yes`; `smoke.script:7-8`
  `> echo hello\r` / `< hello`.
- Confidence: CONFIRMED (the echo is documented in `hello.script:2-3` and `w5.script:4-5`).
- Fix: `< \noutside` and `< \ndone` (and `\nstill-here`, `\ny\r`, `\nhello`), the convention of
  `w5.script`.

### P3. scenarios.script:11-14 — SIGPIPE is not what is checked

- Claim: "A writer killed by SIGPIPE on PeerClosed."
- Script: `yes | head -n 1; echo "statuses ${?}"`, `< y`, `< statuses 0`. `$?` is `head`'s status.
  The prompt proves only that `yes` ended. A libposix that returns EPIPE without raising SIGPIPE
  passes (`yes` exits on the write error). No case of `t_posix` checks `WTERMSIG == SIGPIPE`
  (`statuses` covers SIGINT, SIGSEGV, SIGKILL).
- Confidence: CONFIRMED by reading.
- Fix: a `t_posix sigpipe` case: fork a child that writes to a pipe whose read end is closed in
  both, `CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGPIPE)`; and with SIGPIPE ignored,
  `write == -1 && errno == EPIPE`.

### P4. libposix.script:16 — `x posixrun: refused` looks where a refusal is never printed

- Claim: the `files` run has no refused path.
- Window of the `x`: between `POSIX-STATS` (line 14) and `posixrun: broker messages 0` (line 15).
  `user/src/bin/posixrun.rs:457-470` prints `broker messages` first and the `refused` lines after it.
  A refusal report would lie after the window.
- Confidence: CONFIRMED.
- Fix: move the `x` to after `< posixrun: exit 0` (line 17): the window is then from
  `broker messages 0` to `exit 0`, where the reports are printed.

### P5. libposix.script:36 — `x missing.h` cannot see a report of the missing path (INV-0153-7)

- Claim (lines 27-28): "a missing path inside never" (reported).
- Window: between `posixrun: refused write /elsewhere/new` (line 33) and `posixrun: exit 0` (line 34).
  `t_libposix refused` (t_libposix.c:345-348) opens `/etc/passwd` twice, then `/work/missing.h`, then
  `/elsewhere/new`; posixrun prints reports in channel order, so a report of `/work/missing.h` would be
  printed between the passwd line and the elsewhere line: before the window. A library that reports
  missing paths inside an entry passes.
- Confidence: CONFIRMED.
- Fix: `! refused read /work/missing.h` at the top (the text is nowhere else in the run), or put the
  `x missing.h` directly after line 33.

### P6. zosh.script:42 and :49 — the same window error in the shell's half of INV-0153-7

- Line 42 `x tried to read ~/missing.txt`: window is from the prompt before to the end of the hint for
  `/hello.txt` (line 41). `zosh.rs:1242-1288` (`hints`) prints all hints after the job, in report
  order; `hello.txt` is opened first, so a hint for `~/missing.txt` would follow the matched hint:
  outside the window.
- Line 49 `x hint`: window ends at `dash says hi`; hints are printed after the job's output, so no
  hint can lie in it.
- Confidence: CONFIRMED.
- Fix: move each `x` to after the following `< \r\n~>` (window: last output to the prompt). For line
  42 also `! tried to read ~/missing.txt`.

### P7. t_threads.c:637-656 `stdio` — no check at all

- Claim (header line 15): "printf and a pipe from several threads".
- Four threads print 20 lines each; the case then prints "stdio passed". There is no pipe, and
  neither the program nor `threads.script:37-38` looks at the lines. Lost, torn or interleaved
  lines pass; only a hang or a fault fails.
- Confidence: CONFIRMED.
- Fix: the threads `fprintf` to a `FILE *` over a pipe (or a file in `/work`); the main thread reads
  it back and checks 80 whole lines, 20 per thread.

### P8. t_threads.c:1384-1419 `condsig` (INV-0304-8) — does not show the handler ran on the waiter

- Claim: "a handler runs on a thread blocked in pthread_cond_wait or pthread_mutex_lock; both go on
  waiting".
- The loop checks `hl_count` and `!rc_done` / `!rc_got`. `hl_on` is never compared with `t` here
  (`handlerlock` does it, line 1349), and `rc_wakes` is counted (line 1366) and never read. If
  `pthread_kill` ran the handler on the main thread, the waiter would never be disturbed and the
  case passes.
- Confidence: CONFIRMED by reading.
- Fix: `CHECK(pthread_equal(hl_on, t))` inside both loops (reset `hl_on` each round).

### P9. Forbidden strings that cannot occur

- `! t_posix: FAIL` (`scenarios.script:9`, `measure.script:7`): `CHECK` prints
  `t_posix: NAME: FAIL line ...` (t_posix.c:124, `test = argv[1]` at 1845). The text never occurs.
- `! -FAIL` in `tests/posix/threads.script:3`, `hello.script:4`, `smoke.script:2`: only the
  `/tests/*.sh` scripts print `-FAIL`; these three run none of them.
- Effect: not a false pass (the "passed" line is missing), but a failed check is seen only at the
  timeout (900 s for `threads`), and the first-failure line in the log is the timeout, not the FAIL.
- `invariants.script:7` (`! FAIL`) and `threads.script:7` of posix-core (`! (errno`) do match. In
  `t_threads.c` three FAIL lines have no `(errno` (723, 1144, 1268, 1491): timeout only.
- Fix: `! : FAIL` in the t_posix scripts; `! FAIL` in `posix-core/threads.script`.

### P10. t_posix.c:1223-1225 `tforkthread` — a tautology

- Claim (header 65-67): "the child's pthread_self and join".
- `pthread_t me = pthread_self(); if (!pthread_equal(me, pthread_self())) _exit(30);` cannot fail.
- Fix: take `pthread_self()` before `fork()` in `fork_in_thread` and compare in the child.

### P11. Scripts and targets that nothing runs (Makefile, CI)

- `tests/posix/smoke.script`: named by no Makefile variable, `tools/ci-tests.txt`, `tools/ci-plan.sh`
  or workflow.
- `make test-posix-long` (w1, w2, w2j, w3, w3j and `posix-calls`): not in `tools/ci-tests.txt`, not in
  `.github/`. `docs/adr/invariants.md` rows INV-0110-1 and INV-0110-2 say
  `checked: make test-posix-long`; INV-0005-3 and INV-0308-3 rest on the same call-log union. They are
  checked only when someone runs the target by hand.
- `make posix-cancel-points` (Makefile:989-1004): by hand only; `threads-excluded.txt:10` says the six
  scenarios "pass (exit 0)".
- `w4.script`, `w5-jobs.script`, `measure.script`: `make measure-w4` only (stated in the files).
- Fix: a nightly or `candidate-full/` job for `test-posix-long` and `posix-cancel-points`, or change
  the Status cells to say "by hand"; delete `smoke.script` or add it to the smoke set.

### P12. Comments that claim more than the code does (small)

- t_threads.c:976-978 "(and a thread of the same process is not affected)": only the main thread is
  alive there.
- t_threads.c:1207 "from a second thread's point of view too": no second thread.
- t_posix.c header 68-72 `texec`: "is waited for once": no second `waitpid` (execwait has it).
- w5-jobs.script:33-34 `> fg\r` / `< fg-finished`: matched by fg's echo, as P1 (not in make test; the
  prompt and `after-fg=0` still need the job to end).

## Timing and ordering (class 5)

### T1. t_threads `waits` (t_threads.c:818-873, INV-0302-2)

- The flake "line 837" was in the version before `924b576e` (2026-11-13): line 837 was
  `CHECK(!w_sleep_done && !w_select_done)`, and the sleeper and selector waited 400 ms, which expired
  on the slow TCG runner while the main thread's 20 rounds ran (the bound then was `< 0.3` s).
  `924b576e` made both waits 30 s, released by the test (SIGUSR1, a byte). In the present file line
  837 is `pthread_create(&e, ...)`.
- What the present case still assumes:
  1. `settle()` (150 ms, line 838) is enough for all three threads to be blocked. There is no
     handshake. If they are not yet blocked, the main thread's calls run with nobody blocked: the
     case passes without testing the invariant for those rounds (weak, not a false failure).
  2. Line 854: 20 rounds of create, write, stat, close, unlink in `/work` and a console line take
     less than 10 s of wall time. On a loaded TCG runner this is the remaining false-failure risk.
  3. Line 858: SIGUSR1 reaches the sleeper inside `nanosleep` (or the interrupt is sticky, as the
     comment at lines 67-74 says). If the handler ran before the sleep began and were not sticky, the
     sleeper would sleep 30 s and line 867 fails.
- It is not vacuous: a reader holding the library lock deadlocks the main thread (timeout), a sleeper
  or selector holding it makes the loop take 30 s (line 854 fails).
- Fix for 1: each thread sets a flag before its call and the main thread `wait_flag`s all three before
  `settle()`; for 2: drop the wall-clock bound and rely on `!w_sleep_done && !w_select_done`.

### T2. t_posix `twait` (t_posix.c:1734-1766) — the pattern that flaked in `waits`

- A child lives 600 ms; after a 100 ms pause the main thread does 20 `stat`+`open` rounds and checks
  `ms < 400` and `wait_result == 0`. On a slow TCG runner 20 rounds over 400 ms, or the child ending
  first, fails falsely. `tests/posix/threads.script` runs under `auto tcg` in CI.
- Fix: the child waits for a byte on a pipe instead of 600 ms; the parent writes it after the loop;
  no wall-clock bound.

### T3. Others, lower risk

- t_posix.c:1469 `tsig`: `ms >= 50 && ms < 2000` for a 60 ms `sigtimedwait`.
- t_posix.c:414-420 `groups`: 100 ms pauses order four signals; the counts are exact, so a late
  delivery fails falsely, never passes falsely.
- t_threads.c `blocked` (754-764), `blockedexit` (1236-1247): `settle()` only; if the sibling is not
  yet in `read`, the case passes without a blocked sibling.
- t_threads.c `mutex`, `locks`: totals are right if the threads ran one after another; the yields
  inside the critical sections make an interleaving likely, not certain.
- w5-jobs.script:23-24: `bg-finished` must come after the prompt of `jobs`; on a slow host it can come
  before (false failure). Not in make test.

## Suspected (not verified; the sources are not on this machine)

- S1. `threads-excluded.txt:21` `regression/pthread_exit-dtor`: "the destructor case is covered by
  t_threads tls". From memory the musl test is about `atexit` handlers and stdio flush when the last
  thread calls `pthread_exit`; `tls` checks TSD key destructors and `mainexit` prints through a
  line-buffered stdout. If so, nothing covers it.
- S2. `threads-excluded.txt:15` `functional/tls_align`: from memory `tls_align.mk` links
  `tls_align_dso.o` statically into `tls_align.exe`; it could run if `build.sh` compiled both files.
- S3. `tests/libc-test/build.sh:33`: a second test with the same base name is skipped silently, and
  the number of tests built is printed but pinned nowhere. A changed glob result shrinks the suite
  without a failure. Fix: a pinned count next to `threads-excluded.txt`.
- S4. `rootfs/tests/w2.sh:26-27`: nothing shows which make built `w2-p` and `w2-g` (pdpmake or GNU
  make). It rests on dash 0.5.13.5 keeping `PATH=... build` local to the function call. Fix: echo
  `make --version | head -n 1` per build and expect it.
- S5. `posix-core/threads.script:105-106` `overflow` (INV-0302-6): any fault of the process passes;
  `detail` is not looked at.
- S6. `zosh.script:66-70`: Ctrl-C after `fg` ends the job whether or not it was continued.
- S7. t_posix.c `texec`: no check that any 'O' was read before the first 'N' (the race may not be
  exercised in a round).
- S8. t_threads.c:1284-1286 `hl_handler`: `if (fd >= 0) close(fd)` accepts an `open` that always fails
  in the handler (INV-0304-3 "calls the library").
- S9. `w5.script:21-22`: only `up.txt` is expected from `ls *.txt`; `touch a.txt b.txt` is unchecked.

## Read fully and found sound

- `tests/posix-core/t_libposix.c` (every case checks results and effects; `CHECK` exits 1; the script
  forbids its FAIL text).
- `tests/posix/t_posix.c` apart from P3, P10, T2, S7: `forkpos`, `cloexec`, `execwait`, `groups`,
  `tcsetpgrp`, `statuses`, `blocked`, `raise`, `execmask`, `eintr`, `files`, `products`, `time`,
  `intcount`, `forkbomb` and `diskfill` (the script checks the errno text), `live` (EPERM would fail
  loudly, broker `targets`), `tfork`, `tspawn`, `tsig`, `tsigdfl`, `treroute`.
- `tests/posix-core/t_threads.c` apart from P7, P8, P12, T1, T3: `create`, `cond`, `once`, `tls`,
  `cycles`, `limit`, `exit`, `mainexit`, `signals`, `selfkill`, `cancel`, `handlerlock`,
  `cancelrounds`, `maskedcancel`, `attr`.
- Scripts: `invariants.script`, `hello.script`, `w1`, `w2`, `w2j`, `w3`, `w3j`, `w5`, `w8`, `w8-*`,
  `w9-int`, `w9-term`, `w10`, `tests/posix/threads.script` (apart from P9), `tour.script` (strict `^`
  lines with `\r\n`; the generator `tools/tour` was not read), `threadtest3.script`,
  the generated libc-test script (a failing test stops the run at its `exit 0`).
- `rootfs/tests/w1.sh`, `w2.sh`, `w3.sh`, `w8.sh`, `w9-check.sh`, `w10.sh`: every failure prints
  `-FAIL` or changes `W*-STATUS`; `w4.sh` is a measurement by design.
- Makefile recipes 905-1120: each loop is `mkfs && runner && fsck || exit 1`; no failure is
  swallowed. `LOCAL_ACCELS` is never empty. `POSIX_TIMEOUTS` only sets how long a failure takes.

## Not read

- `tools/tour` (the generator), `tools/posix-surface/workloads/w8` (Makefiles, `mktab.c`),
  `tests/posix/trees.sh`, `tests/libc-test/strerror_r.c`, `ports/Makefile` (`suites`, `check-libc`).
- The libc-test and SQLite `threadtest3.c` sources (not unpacked here): S1, S2 and the exit status of
  `threadtest3` for an unknown scenario name are from memory.
- `gui.script`, `room-gui.script`, `tour-gui.script` (test-posix-gui, out of scope).
- `tools/posix-stats` (`posix-calls`), `measure-w4`'s recipe.
