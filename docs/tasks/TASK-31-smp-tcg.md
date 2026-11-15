# TASK-31 part `work/smp-tcg`: kernel tests under TCG with four CPUs

Branch `work/smp-tcg`, from `origin/main` at 514b7d42. Asked by the orchestrator, because the architect does not accept
"slow runner" for failed tests: a slow runner explains a timeout, not a failed test. This report says what the CI logs
show, what a local reproduction showed, and what is fixed.

## Summary

- **CI run 43717763197, attempt 1, `make test-qemu-tcg-smp4-gicv2`: no test failed. The suite ran out of time.** The
  runner's "9 failed" are the 9 `--expect-panic` tests (a green run prints "119 passed, 9 expected failures"; it is the
  same count). "`stream_create: 8 of 8 failed`" and "`channel_create: 8 of 8 failed`" are t_thread's *pass* lines: the
  case requires every round's call to fail, and asserts that right before the count is printed. The two "no output
  line contains `first process: ...`" lines come from what runs after the suite, which the timeout cut off. So it is one
  failure, the 420 s timeout, with test 123 of 128 (`kernel_work_is_bounded`) running.
- On that runner, user code ran at 2 times the speed of the green re-run. The longest IRQ-masked interval of each memory
  call was 20 to 100 times longer, with the same units of work. A periodic stall of 20 to 50 ms of the vCPU explains
  that. It is the host, as `t_bound`'s own comment records for an earlier run of the same job (commit 68a22cbb). No
  kernel race is shown or needed to explain it.
- The other "smp4" failures on record are both explained and fixed already. In 43437795275 (`boot-tcg-smp4-gicv3`) the
  console script expected the count in the wrong order (TASK-29 F5). In 43695351272 (candidate `work-f3`, `tcg-smp4-gicv2`
  and `tcg-el2`) `t_quota` sampled while the reaper was still returning charges (3d48c87f).
- The kernel runs on **one CPU** in the smp4 configurations (`docs/design.md`, "Single core"). The other three vCPUs stay
  powered off under PSCI, so no SMP code of the kernel runs there.
- **Locally, 2 of 20 plain `tcg-smp4-gicv2` runs failed: a real failure, and a cascade from it.** `t_cpu`'s 40-spinner
  case saw the uncapped spinner count half of its usual (685M against about 1 360M), so the room's share passed 500 per
  mille. **Cause (section 4.1): not the kernel.** In 6 of 40 rounds of t_cpu alone, the kernel had charged the spinner
  its full time (1 243 to 1 268 ms of 2 s), with idle time and interrupts as usual, but it counted at 0.25 to 0.62 of
  its usual rate. Under TCG each process executes at its own speed, and the test took counts for CPU time. **Fixed in the
  test (4.2):** the upper check compares the room's counts per charged ms with the count loop's best rate; after the
  fix, 0 of 40 rounds failed. Not a kernel change. Then t_cpu panicked and left its 41 spinners running. That made
  `t_rooms` 35 times slower, and `t_snapshot` (frames moving under its frame-exhaustion check) and
  `a_spawn_with_a_big_space_is_bounded` (1 090 frames free instead of about 17 400) fail.
- Under host load (10 CPU hogs, `-smp 1`), t_thread's `creating_calls_leave_no_handles` failed once. It assumes the
  sibling finishes its unmap within the one switch of the paused call (section 4b). **Fixed in the test:** a call that
  succeeded is a miss and the round is repeated. Wall-clock bounds of two other tests failed under the heaviest load;
  recorded, not changed.
- **Open (4.3):** a killed room at 10% with 20 parked spinners took up to 1 246 ms (12 periods) to die, in 2 of 40
  rounds. ADR-0285 item 7 says at most one period. The likely cause is that the teardown runs on the dying thread and
  is charged to the capped room. Changing that is a decision for a proposed ADR; nothing is changed.
- **CI and runner (section 6):** a red QEMU step now uploads `target/test-logs/`, and the runner prints the five
  slowest tests and, at a timeout, how long the running test had run. Time limits are not changed.
- **Fixed on this branch: the cascade.** After a failed test, the harness now ends every process left in the job tree and
  waits until none is live and the reaper is done (`end_what_a_failed_test_left`). New tests check it.

## 1. Run 43717763197, attempt 1: what the log shows

Tests: 128. Passed: 113. Finished as expected panics: the 9 listed with `--expect-panic`. Running at the timeout:
`kernel_work_is_bounded` (test 123). Not reached: `cpu_ceiling_from_a_process`, `room_keeper_from_a_process`,
`snapshots_from_a_process`, `stack_canary_intact`, `a_spawn_with_a_big_space_is_bounded`. Every `ZORAYA-TEST` line in the
printed part of the log is `ok`, and no `user panic` or `fail` line is in it. The runner printed the last 150 lines and a
summary, and skipped 433 lines (`target/test-logs/` is not uploaded; the run's only artifact is `build`). So the
beginning of the suite is known only through the runner's count.

The same figures from four jobs. Attempt 1's `gicv3` job ran on the same kind of slower runner, and t_streams there
measured 127 MB/s, as in the failing job:

| figure | failing gicv2 (att. 1) | gicv3 (att. 1) | gicv2 (att. 2, green) | tcg-default (att. 1) |
|---|---|---|---|---|
| t_bound fill 256 KiB (user code) | 163 us | 121 us | 80 us | 82 us |
| t_streams | 127 MB/s | 127 MB/s | 206 MB/s | 207 MB/s |
| memory_create, one step / large, masked | 19.3 / 27.9 ms | 0.47 / 0.45 ms | 6.3 / 11.6 ms | - |
| memory_map | 37.4 / 37.8 ms | 0.48 / 0.54 ms | 0.40 / 0.42 ms | 0.33 / 0.42 ms |
| memory_unmap | 24.0 / 24.4 ms | 0.36 / 0.35 ms | 0.23 / 0.21 ms | - |
| memory_create contiguous | 18.3 / 23.2 ms | 0.65 / 0.19 ms | 0.20 / 0.12 ms | - |
| snapshot_process | 51.2 / 2.6 ms | 5.2 / 2.6 ms | 4.4 / 1.6 ms | 4.1 / 1.6 ms |
| Run step | 449 s (timeout) | 375 s | 214 s | 217 s |

The figures are the *longest* masked interval of a measurement, and t_bound's units check passed in every case (no
assertion). In the failing job the small and the large case of a site are nearly equal (37.4 and 37.8 ms), and closing
memory, which does little work, also shows 28.7 ms. That is what a host stall of tens of ms inside some masked interval
of each measurement looks like. It is not what more work per step looks like. The guest's clock under TCG is the host's
clock, so a stalled vCPU shows as guest time.

The time budget: over the last 120 runs of `test.yml`, the green Run steps of the TCG kernel-test jobs took (seconds):

| job | n | median | p90 | max |
|---|---|---|---|---|
| tcg-default | 28 | 219 | 301 | 306 |
| tcg-smp4-gicv2 | 25 | 235 | 301 | 311 |
| tcg-el2 | 27 | 201 | 289 | 306 |
| tcg-smp4-gicv3 | 28 | 279 | 359 | 375 |

The runner's limit is 420 s (`QEMU_SLOW_TIMEOUT`). A green job already uses up to 89% of it. A runner with stalls, as
attempt 1's was, goes past it.

**What the log supports:** one failure, the timeout; a runner whose memory calls were stalled; no failed assertion. **What
it does not show:** the time of each test (the runner prints none), the first 433 lines, and anything about why that
runner stalled. It does not support "tests failed", and it does not exclude a kernel slowdown specific to that runner,
for example the cost of the broadcast TLB invalidations (`tlbi vaae1is`, `aside1is`) when QEMU runs 4 vCPU threads. It
only says that nothing needs one. The local comparison in section 3 did not show such a cost with idle secondaries.

## 2. The earlier smp4 failures

- Run 43437795275 (936db09), `make test-boot`, leg `boot-tcg-smp4-gicv3`: `timed out waiting for "bytes dropped]"`.
  Cause found by TASK-29 (F5, `docs/research/test-audit/report.md` section 3): the script's order, not the speed.
  `tests/shell/console.script` was fixed (backlog entry "The `make test-boot` leg ...").
- Run 43695351272 (candidate `work-f3`, 66ad2da7), `tcg-smp4-gicv2` and `tcg-el2`: `quotas_from_a_process` failed with
  `the maker pays: 8708712 -> 4506216`. The job's use fell by 4.2 MB between two reads, because the reaper was still
  returning the charges of an earlier child. That is a test that sampled too early, not SMP. It was fixed in 3d48c87f (it
  waits until two reads 20 ms apart agree). It is still a wall-clock heuristic: a reaper that pauses longer than 20 ms
  between two releases on a stalled host would pass it too early.

## 3. Reproduction

Host: the development Mac (10 cores: 4 performance, 6 efficiency), QEMU 11.1.2, through `make` and
`tools/test-runner` only, one QEMU at a time. The kernel was built from `origin/main` (514b7d42). From the 6th run on it
had the temporary prints of section 4 (they were removed afterwards). Logs:
`<scratchpad>/loop/*/run*.log` (not committed).

| runs | target | load | result |
|---|---|---|---|
| 12 | `make test-qemu-tcg-smp4-gicv2` | none (another session's HVF GUI test ran at times) | 10 passed (153 to 175 s); **2 failed: t_cpu share, and the cascade** |
| 1 | the same | QEMU and 6 CPU hogs on efficiency cores (`taskpolicy -b`) | timeout at test 86; `timer_interrupts_agree_with_the_clock` (13 ticks in 200 ms), t_thread `the process took 440 ms to end`, `harness_resumes_after_panic` (10 failures counted, 9 expected; a consequence) |
| 2 | the same | QEMU and 2 hogs on efficiency cores | both timed out at test 89 (`job_signal_reaches_every_process_of_its_tree`, 23 s in a plain run), no failed test |
| 8 | the same, with the counters of section 4 | none | 8 passed (158 to 174 s) |
| 5 | the same machine with `-smp 1` (the runner with `tcg-smp4-gicv2`'s command but the CPU count) | none | 5 passed (154 to 158 s) |
| 1 + 1 | `-smp 1`, then `-smp 4` | 10 normal-priority hogs | `-smp 1`: t_thread's creating calls failed (section 4b); `-smp 4`: passed, 311 s |
| 2 | `make test-qemu-tcg-smp4-gicv3` | none | 2 passed (186 s each) |
| 3 | the `boot-tcg-smp4-gicv3` leg of `make test-boot FULL=1` (the runner with its command line) | none | 3 passed, 195 steps each |

So: **20 plain `-smp 4` runs, 2 failures, both the t_cpu case of section 4.** 5 plain `-smp 1` runs, none.

Under the same 10 hogs, `-smp 4` was not slower than `-smp 1` in the memory calls (`memory_map` 330 against 548 us a
step, `memory_unmap` 278 against 365 us, `snapshot_process` 3.1 against 4.9 ms). So the 4-vCPU guest's broadcast TLB
invalidations cost nothing visible with idle secondaries on this host, and CI's 20-to-100-fold figures are not
reproduced by load alone. `job_signal_reaches_every_process_of_its_tree` took 79 s with `-smp 4` against 53 s with
`-smp 1`, in one pair.

The efficiency-core load makes the guest 3 to 8 times slower. It reproduces timeouts, and in the heaviest form it
reproduces wall-clock bounds of tests that fail (`timer_interrupts_agree_with_the_clock`, t_thread's 400 ms). Those are
assertions on wall time, which a stalled vCPU breaks. They are recorded here, not changed.

Per-test time in a plain run (temporary print): `job_suspend_from_a_process` 38 s, `job_signal_reaches_every_process_of_its_tree`
23 s, `threads_and_futexes_keep_their_rules` 20 s, `cpu_ceiling_from_a_process` 16 s, `kernel_work_is_bounded` 10 s; all
128 together 153 s.

## 4. The local failure: `t_cpu`'s uncapped spinner loses half its time

Both failing runs: in `a_room_at_a_quarter_gets_a_quarter(40)`, the room counted as usual (803M and 800M; its kernel
counter 740 to 750 ms of a 500 ms budget, as in every local run), but the uncapped spinner counted 685M and 678M,
against 1 340M to 1 395M in the 8-spinner case of the same run and in every passing run. So `check_share` saw 539 and
541 per mille, above its 500. Room and free spinner count at the same rate per CPU-second (about 1.08 G/s), so about
630 ms of the 2 s window ran neither of them.

What was measured, with temporary prints:

- The ready and parked lists at the end of the window (a test call that walks them): exactly 41 threads, the 40 of the
  room's job and the free spinner, nothing else. No leftover of an earlier test competed.
- In passing runs, over the 2 s window of the 40 case: idle ran 12 ms, 405 schedules, 202 picks that hit
  `MAX_PARKS_PER_PICK`, 202 interrupts. The 8 runs with these counters all passed, so there are no counters from a
  failing run.

Locally the room overruns its quarter by about 50% in every run, `-smp 1` included (737 ms of 504 ms). On CI's Linux
runners it overruns by 2 to 4% (512 to 519 ms). CI's shares are 248 to 256 per mille. So the local host delivers the
slice timer late. CI has not shown this failure in the logs read for this report.

First diagnosis (before the loop below): it is not another ready thread. The candidates were (a) the idle thread
running while the free spinner was ready, (b) kernel time in interrupts during the free spinner's turns, and (c) the
vCPU executing less in the free spinner's turns. The counters above separate them: (a) shows as idle time, (b) as many
more than about 200 interrupts in the window, (c) as neither.

### 4.1 The cause: the count share measures TCG's speed per process, not CPU time

To loop the case alone, a temporary test ran `t_cpu` 40 times in one boot (`-smp 4`, gicv2, TCG; a compile-time
filter ran only that test). With temporary prints it also read, for the window, the kernel's own charge to each job
(`job_get_usage(..).cpu.used_ns` of the room and of the free spinner's job), idle time, schedules and interrupts.
**6 of the 40 rounds failed**, in the 40-spinner case and in the 8-spinner case, each with the same signature:

| | room charged | room counted | free charged | free counted | free's rate | idle | irqs |
|---|---|---|---|---|---|---|---|
| a passing round | 746 ms | 802M | 1 254 ms | 1 355M | 1.08 a ns | 12 ms | 201 |
| failing, case 40 | 744 ms | 799M | 1 243 ms | 314M | 0.25 a ns | 12 ms | 203 |
| failing, case 8 | 751 ms | 811M | 1 266 ms | 789M | 0.62 a ns | 0 ms | 204 |
| failing, case 40 | 750 ms | 809M | 1 252 ms | 779M | 0.62 a ns | 12 ms | 203 |

The kernel gave the free spinner all the time the room and idle left (1 243 to 1 268 ms of a 2 s window), and the room
its usual. Idle time and interrupts were as in passing rounds. The spinner executed fewer instructions in the CPU time it
got. The kernel cannot take that time inside a turn: an exception from EL0 other than `svc` ends the process
(`exception.rs`, `dispatch_inner`), and the count loop makes no system call. So the only kernel work inside a turn is the
about 200 interrupts, the same in every round. The rate is a property of each process under the emulator. Over the 40
rounds and the 20 plain runs it was 0.25 to 1.08 counts a ns for the free spinner and 0.40 to 1.08 for a room, in
`-smp 1` too (981M in one round), and in the same window one process can be slow while the other is not (room 0.40 with
free 1.08; free 0.25 with room 1.07). It never rose above the best, 1.08.

**So: no scheduler or accounting defect, and no SMP fault.** The kernel's charges are right. The test's upper check,
the room's share of the spinners' counts at most twice the ceiling, assumed that every process counts at the same rate
per ns of CPU. TCG does not keep that.

### 4.2 The fix (`user/src/bin/t_cpu.rs`)

`check_share` keeps an independent check (review W4), in a form a slowdown cannot break. A kernel that let a room run
longer than it charged shows as the room counting more per charged millisecond than the count loop can. A slow vCPU only
lowers rates, so it cannot fail this. The upper check is now: the room's counts per millisecond of the kernel's charge are
at most 5/4 of the best rate seen. The best rate is the free spinner's rate in the same window, or `calibrate_counts`'s:
the count loop alone, uncapped, three times 200 ms, the best. The share of all counts stays as the lower check, a quarter
of the ceiling (not starved). The kernel's own checks are unchanged (`used <= upper(..)`, `used * 5 >= budget * 4`).

Why this is not weaker. Under TCG the old upper check did not measure CPU: it failed 6 of 40 rounds of a correct kernel,
and a bound that fails on a correct kernel shows nothing when it fails on a wrong one. The new check bounds the room's
true CPU by its counts and the loop's best rate: true CPU ≥ counts / best rate, and the check requires that to be at
most 5/4 of the kernel's charge, which `upper(budget)` bounds. So under TCG a room that ran more than it was charged
fails, whatever its neighbour did. Under HVF (factor 1) a lone spinner counts about 2.3 times as fast per CPU-ns as a
room's spinners (6.42M against 2.79M a ms), so there the rate check is loose. The old share bound (at most twice the
ceiling) therefore stays under HVF, where it never failed, as `upper` and `masked_limit` already differ by factor.

A hand mutant tests it: `schedule` charging half of every run (`ran / 2` and the period part `/ 2`), under HVF. In the
first two cases neither the old share bound (374 and 381 per mille, limit 500) nor the new rate check (2.79M against
6.42M a ms) caught it. `rooms_together_stay_under_their_parent`'s kernel-counter lower bound did ("764864909 ns:
starved"). So the count check was never the detector of a uniform undercharge; it watches the room against its
neighbour.

**Runs** (t_cpu 40 times in one `-smp 4` TCG boot, the temporary loop; prints removed afterwards):

| | rounds | share/rate failures | other failures |
|---|---|---|---|
| before (old check) | 40 | 6 (shares 506 to 717) | none |
| after (rate check) | 40 | 0 (80 checks; room rate at most 0.996 of the reference) | 2: `a_killed_room_with_parked_threads_dies` (section 4.3) |

### 4.3 Open: a killed parked room took up to 12 periods to die

`a_killed_room_with_parked_threads_dies` asserts that a killed room at 10% with 20 parked spinners has no process left
within (100 + 50) ms × factor. Over the 74 rounds of both loops that reached it, it took 18 to 1 246 ms, median 147
ms. It failed twice in the second loop (950 and 1 246 ms; the first loop's slowest pass took 937 ms, at a larger
factor). ADR-0285 item 7 says a killed job's parked thread ends at the next boundary, at most one period. But the dying
thread runs the process's teardown itself (`process::leave_current` → `teardown`), and `schedule` charges it to the
room, so the exits of 20 processes share the room's 10 ms a period. If they need more than one period's budget, the room
takes several periods to die. 8 to 12 periods is what about 5 ms of teardown each under a slow TCG host would give.
This is a hypothesis: no count of the room's parks after the kill was taken. If it holds, letting the threads of a killed
job run unparked is a decision: it breaks `RunQueue::pick`'s rule that no thread of a spent job ever runs, so it needs a
proposed ADR (a free number in 0380..0389 or 0396..0399). Not done here; the bound is not changed.

## 4b. Under host load: t_thread's creating calls assume the sibling finishes in one switch

The `-smp 1` run under 10 hogs failed in `threads_and_futexes_keep_their_rules`:
`channel_create: the sibling ran first`, `left: 0, right: 0` (`user/src/bin/t_thread.rs:605`). The status was `Ok`: the
call succeeded, because the sibling had **not** unmapped the buffer in time. (The message names the opposite case.) The
case relies on `PAUSE_IN_PREPARE` and `FORCE_SWITCH`. The paused call switches away once (`steps::pause`), and the sibling
must leave its `thread_yield` loop and finish `memory_unmap` before the creator runs again. With `FORCE_SWITCH` on, a
slice is 1 ms of guest time (`FORCED_SLICE_NS`), and a host that stalls the vCPU can end the sibling's slice first. This
is a wall-clock assumption of the test, not a kernel fault. It was seen once. **Fixed in the test** (`user/src/bin/t_thread.rs`): a round whose call succeeded is a miss. Its two
new handles, the table's entries that were not there before the call (`handles_in_use`, the test kernel's
`HANDLE_AT`), are closed, and the round is repeated until 8 rounds have failed as required, with at most 56 misses. Every
counted round still reaches the undo and checks the table and the charge, so the mutant that keeps the handles still
dies. The charge is checked after a miss too. The line now reads `8 of N failed (M missed)`, and the message of the
other case names it: "the sibling never unmapped in time".

The heavy efficiency-core run (section 3) also failed two other wall-clock bounds:
`timer_interrupts_agree_with_the_clock` (13 ticks in 200 ms) and t_thread's `the process took 440 ms to end` (limit
400 ms). They are bounds on wall time, which a starved vCPU breaks. They are recorded, not changed.

## 5. Fixed: a failed test no longer leaves its processes running

`kernel/src/testing.rs`: `resume` (the harness's way back after a failed test) calls `end_what_a_failed_test_left`. If
the root job's tree has a live process, it ends all of them (`Job::end_every_process`, test kernel only: as `kill`, but
it leaves the jobs open), yields until none is live (at most `PATIENCE_NS`, then it says so and goes on), and drains the
reaper (`frames::settled_stats`).

Tests: `harness_fails_with_a_process_left` is a new expected failure (in `EXPECT`, and `EXPECTED_FAILURES` is 10). It
spawns `busy` suspended, so the process would stay live for ever, and panics. `nothing_is_live_after_a_failed_test`
runs right after it and asserts that the root job's tree is empty. Without the fix it fails on every host, because a
suspended process never ends by itself.

Run on this branch: `make check` exit 0. `make test-qemu-auto-default`: 120 passed, 10 expected failures. The cleanup
also ran after two older expected failures whose processes were still live. `make test-qemu-tcg-smp4-gicv2`: 120 passed,
10 expected failures. With the call in `resume` commented out, `make test-qemu-auto-default` failed in
`nothing_is_live_after_a_failed_test` ("a failed test's process outlived it"), and `harness_resumes_after_panic` counted
11 failures. `make test-changed` was not run: the change is under `kernel/`, which maps to every QEMU step, and the
candidate run covers them.

## 6. CI and the runner

- **Done:** `.github/workflows/test.yml`, job `tests`: after "The logs of what failed", a step "Keep the test logs of a
  failed step" (`if: failure()`) uploads `target/test-logs/` as `test-logs-<job index>-attempt-<n>` and keeps it 7 days.
  The runner caps each log at 64 MiB. The name uses the job's index because a step's command has characters an artifact
  name may not have. `make check-workflow-syntax` and `make check-ci-jobs` pass.
- **Done:** `tools/test-runner/src/timing.rs`: the runner times each test by its `ZORAYA-TEST run` and `ok`/`fail` lines.
  After the verdict it prints `slowest tests: NAME 38.0 s, ...` (five). At a timeout it also prints `the running test:
  NAME for N s`, which tells a slow suite from a hung test. Three host tests.
- **Not changed:** the time limits. The numbers are: green Run steps up to 375 s (`tcg-smp4-gicv3`) of the 420 s; locally
  `job_suspend` 38 s, `job_signal` (10 000 rounds) 23 s and t_thread 20 s of a 153 s suite.
- **Proposed:** a manual workflow that runs `make test-qemu-tcg-smp4-gicv2` 20 times and keeps every failing log, to see
  whether CI's Linux hosts ever show the per-process speed of section 4 (CI's shares were 248 to 256 per mille in every
  log read).

## Checked and found sound

- The `8 of 8 failed` lines: `creating_calls_leave_no_handles` asserts `status != Ok` and `status == BadAddress` before it
  counts a round, so the line is printed only when every round failed as required.
- The `exit-with: 0: unexpected operand` lines appear inside `threads_and_futexes_keep_their_rules`, which passed. They
  are a child's own usage message.
- No test of the kernel suite runs the kernel's SMP code: the three other vCPUs are never started (`boot.rs`).
