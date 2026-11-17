# TASK-31: review of `work/smp-tcg` (kernel tests under TCG with four CPUs)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 514b7d42...origin/work/smp-tcg` at `aa98215c`: `user/src/bin/t_cpu.rs`, `user/src/bin/t_thread.rs`,
`kernel/src/testing.rs`, `kernel/src/job.rs`, `Makefile`, `tools/mutants/kernel-objects.toml`,
`tools/test-runner/src/{main,timing}.rs`, `.github/workflows/test.yml`, `docs/backlog.md` and the report
`docs/tasks/TASK-31-smp-tcg.md`. Read against ADR-0264, ADR-0285 (items 4 to 7) and ADR-0270. The report's claims
were re-run where it mattered, not taken.

## What was run

In a detached scratch worktree of `aa98215c` (removed afterwards), one QEMU at a time, through `make` only:

| Run | Kernel / test change | Result |
|---|---|---|
| E1, `make test-qemu-auto-default` (HVF, factor 1) | t_cpu made to fail on purpose in `wake_up_latency_next_to_capped_rooms(20 ms)` (two capped rooms, 64 processes live); a temporary print of what is live when the harness gives up | t_cpu's rooms all ended; **the harness waited its whole 10 s three times and named `koverflow` each time**: finding 1 |
| M1, the report's mutant: `schedule` charges `ran / 2` and the period part `/ 2` (`kernel/src/sched.rs:516`) | HVF, a loaded host (t_cpu's factor 2) | a-room cases **pass**: shares 521 and 701 per mille (ceiling 250), rate check passes (1.12M against 1.22M a ms); `rooms_together` passes (842 of 1 029 ms, its lower bound is 823); caught only by the wake-up latency (`t_cpu.rs:665`, 151 ms against 70 ms) |
| M1 | HVF, factor 1 | caught by `assert!(u1.cpu.parked > u0.cpu.parked)` (`t_cpu.rs:353`; the room never parked) |
| M1 | `make test-qemu-tcg-default` (factor 6) | a-room cases **pass**: shares 533 and 534 per mille, which the removed bound (500) would have failed; rate 2.085M and 2.089M against 5/4 of 2.134M; `rooms_together` passes (976 of 1 058 ms; share 922); caught only by the wake-up latency (721 ms against 210 ms) |
| M3: the leaf of a charge with a limit of its own is charged half (`zoraya_cpu::charge`), the free spinner in full | HVF, factor 1 | caught by `parked > 0` (`t_cpu.rs:353`) |
| M3 | TCG, factor 6 | caught by the new rate check (`t_cpu.rs:303`: 2.145M a ms against 5/4 of 1.090M); the share was 530 |
| `cargo test --manifest-path tools/test-runner/Cargo.toml timing` | | 3 passed |
| `make check` (on this review branch) | | exit 0 |

So the report's statement that the halved-charges mutant "was caught only by `rooms_together`'s kernel-counter lower
bound" was not reproduced: in three runs it was caught by `parked > 0` once and by the wake-up latency twice, never by
a check of the ceiling's own case. Which check catches it depends on the host's load.

## Findings

### 1. error (must-fix): the cleanup waits its full 10 s twice in every green run, for a process that can never end

*Where:* `kernel/src/testing.rs:639-655` (`end_what_a_failed_test_left`), with `PATIENCE_NS` (`testing.rs:990`, 10 s).

*What:* `fault_report_for_stack_overflow_from_el0` (`run_program("koverflow")`) panics on the kernel stack of
`koverflow`'s thread; `on_panic` abandons that thread (`adopt_boot_thread` forgets it), so the process `koverflow` stays live in the
root job for the rest of the boot with its exit requested, and nothing can end it. From that test on, `root.is_empty()`
is never true again. The cleanup then runs after every later failure, expected ones included, sends a second exit
request to `koverflow`, yields for the whole `PATIENCE_NS` and prints "processes still live".

*Scenario (observed, run E1, `target/test-logs/auto-default.log`):*

```
ZORAYA-TEST fail fault_report_for_stack_overflow_from_el0 panic at ... kernel stack overflow from EL1 ...
harness: ending the processes the failed test left
harness: processes still live after the failed test
REVIEW live: koverflow exit_requested=true
ZORAYA-TEST run fault_report_for_stack_overflow
ZORAYA-TEST fail fault_report_for_stack_overflow panic at ...
harness: ending the processes the failed test left
harness: processes still live after the failed test
REVIEW live: koverflow exit_requested=true
```

(the `REVIEW` line was the temporary print). That is 20 s added to every kernel-test run, a green one too, on every
configuration. Under TCG the guest's clock is the host's, so it is 20 s of wall time in jobs whose green runs already
take up to 375 s of the 420 s limit (the report's own table) — the timeout this branch set out to explain. Every real
failure after that test costs another 10 s. A green log now carries "processes still live after the failed test" twice,
so the message means nothing when it matters, and `nothing_is_live_after_a_failed_test` holds only because it runs
before `koverflow`. The runner's new "slowest tests" cannot show the cost: the wait lies between a `fail` line and the
next `run` line, so it belongs to no test (`timing.rs` `line`), which is likely how it went unnoticed. The report says
"the cleanup also ran after two older expected failures whose processes were still live" (section 5); it does not say
that it timed out both times.

*Fix:* clean up relative to a baseline, not to an empty root: record the set (or count) of live processes before each
test in `run`, and after a failure end only what the failed test added and wait until the live count is back at the
baseline. Or have the tests that abandon a process on purpose account for it (a count of known-lost processes the
cleanup subtracts). A wait that runs out should fail the run (count a failure, as `harness_resumes_after_panic` checks
counts), not print and go on. Add a check that a green run has no "still live" line, and print the cleanup's duration.

### 2. error (must-fix): under TCG the room's upper count check no longer depends on anything but the kernel's charges

*Where:* `user/src/bin/t_cpu.rs:294-315` (`check_share`), the reference at `:296`, the gate at `:307`;
`calibrate_counts` at `:88-109`.

*What:* review W4 asked for an independent measure of the ceiling, one that needs no kernel counter. The old upper
bound (the room's share of all counts at most twice the ceiling) was that: counts against counts. The new bound divides
the room's counts by the room's *charged* time and compares it with a reference that is also counts per *charged*
time (the free spinner's in the window, or `calibrate_counts`'s, both from `job_get_usage().cpu.used_ns`). It is a
consistency check between two charges made by the same code. A room-specific under-charge shows in it (M3 under TCG:
caught); a uniform one scales numerator and reference alike and can never show, at any factor. Off HVF (factor > 1) the
share bound is gone, so the only upper checks left there are the kernel's own counter (`used <= upper(..)`, which is
`2 * budget` off HVF) and this one. CI runs every kernel-test job under TCG (Linux runners), so on CI the ceiling now
has no upper check independent of the accounting it is meant to verify.

*Scenario (observed, M1 under TCG):* every charge halved; the 25% room took 533 and 534 per mille of the counts,
more than twice its ceiling; the removed bound fails at 500, the new check passed (2.085M a ms against 5/4 of 2.134M,
both halved), `used` read 540 and 533 ms of 504 and 500 ms, and `rooms_together`'s lower bound passed. Only the
wake-up latency test failed, which is a different invariant (INV-0264-3) and caught it by side effect. A uniform
under-charge between about 1.25 and 4 times (above that `used * 5 >= budget * 4` fails, since a room with 8 spinners
next to one free spinner cannot get more than 8/9 of the CPU) passes both a-room cases on CI.

The report's argument (section 4.2) that "true CPU ≥ counts / best rate, and the check requires that to be at most
5/4 of the kernel's charge, which `upper(budget)` bounds" holds only if the best rate is a property of the loop. It is
not: it is measured per charged ms, so it moves with the charges.

*Fix:* give the upper check a reference that is not a charge. The cleanest: let the room's spinners measure their own
run time — loop on `clock_monotonic`, add each step below a small gap (below the 50 us floor slice) to a per-spinner
total in the shared page — and assert that the room's self-measured time is at most `upper(budget, periods)` (and,
independently, close to `used`). It is wall time during the room's turns, which is what the kernel charges, so a host
stall raises both alike, and a stall the gap filter drops only lowers the measured value (safe for an upper bound);
TCG's speed per process does not enter, since nothing counts instructions. Short of that: a reference in counts per
wall ms of `clock_monotonic` for a lone spinner, with the bound `5/4 × best_wall × upper(budget, periods)`; it catches a
uniform under-charge above about 2.5 times only, but it is independent. Keep the counts-per-charge check as well: it
is what caught M3.

### 3. warning (should-fix): the share bound is kept by host speed, not by accelerator

*Where:* `user/src/bin/t_cpu.rs:307` (`if factor() == 1`).

*What:* the factor is `calibrate`'s fill time over 10 us. HVF on a loaded host gives 2 (run M1-HVF: "fill of 256 KiB
takes 14292 ns: time bounds scaled by 2"); then the share bound, which the comment says is kept "under HVF", is off.
Under HVF the rate check is loose (the report: a lone spinner counts 2.3 times as fast per charged ns as a room's), so
nothing independent is left.

*Scenario (observed, M1 under HVF at factor 2):* shares 521 and 701 per mille at a 250 ceiling passed both a-room cases.

*Fix:* decide by the accelerator, not by speed: `tools/run-qemu.sh` knows it and can pass it (`/chosen/bootargs`, read by
a test call), or keep the share bound whenever the measured lone-spinner rate per charged ms differs from the room's by
more than TCG's spread. With finding 2's self-measured time the share bound can go everywhere.

### 4. warning (should-fix): "a slow vCPU only lowers a rate, so it cannot fail this" is not true of the reference

*Where:* `user/src/bin/t_cpu.rs:284-285` (comment) and `:296`; `docs/tasks/TASK-31-smp-tcg.md` 4.2.

*What:* the room's rate can only fall with a stall, but so can the reference. `reference = max(free's rate in the
window, BEST_RATE)`, both measured on the same host in the same boot. If the three 200 ms calibrations and the window's
free spinner are all slow and the room is not, a correct kernel fails. The report's own data has that pairing in one
window (free 0.25 counts a ns with room 1.07), so only the max over three calibrations stands between it and a false
failure. The other direction is the one that hides a defect: a slow room hides an under-charge of up to
`5/4 × best / room_rate` (0.40 against 1.08 counts a ns gives 3.4 times). So the check fails for the right reason only
in windows where the room ran near the best speed; under TCG it is a probabilistic detector, and its false-failure path
exists. Also, `calibrate_counts` reads the charge before the counter at both ends (`:94-101`); a preemption of the
parent between the two reads at the end adds a slice of counts that is not in the charge, raising `BEST_RATE` by up to
a slice over 200 ms (5% at a 10 ms slice), which loosens the bound.

*Fix:* state the false-failure path in the comment; read the counter and the charge in the same order at both ends
(or take the charge, the counter, the charge again and drop a sample whose two charges differ by more than a step);
with finding 2's fix the point goes away.

### 5. warning (should-fix): `a_killed_room_with_parked_threads_dies` is a real departure from ADR-0285 item 7, not a test artefact

*Where:* `user/src/bin/t_cpu.rs:512-549`; `kernel/src/process.rs:977` (`leave_current`) and `:1040` (`teardown`);
`kernel/src/sched.rs:500-525` (the charge in `schedule`); ADR-0285 item 7.

*What:* the code supports the report's hypothesis. The dying thread runs the whole teardown itself (handles, space,
frames), with step points that yield (`steps::flush`, ADR-0270); each switch charges the room (`zoraya_cpu::charge`)
and `RunQueue::pick` parks a thread of a spent job, dying or not. So a killed room ends after
`ceil(teardown work / budget)` periods, not "at the next boundary (at most one period)". That the run got 950 and
1 246 ms at a 100 ms period on a slow host is consistent; that it never failed under HVF is too (20 small exits fit in
10 ms there). The test's bound is the ADR's; the test is right to fail. The report did not prove the cause (no count of
the room's parks after the kill). The consequence is wider than the test: a killed room at a low ceiling keeps its
processes, and their frames and charges, for many periods (a room at 1% with processes of 24 MiB, as `t_bound` makes,
would take seconds), which delays the reuse that killing a room is for.

*What a proper test needs:* a deterministic, clock-light form that also fails under HVF if the cause holds: a room at
the lowest ceiling with a few processes with large spaces (as `t_bound`'s 24 MiB one), parked, then killed; read the
room's `cpu.parked` right after the kill and after the first boundary, and assert that it does not grow after that
boundary, and that the room is empty within two periods (count periods, not milliseconds times the fill factor).
*What a fix needs:* a proposed ADR (the report names the free numbers): either a thread whose process is ending is
charged but not parked (a bounded overrun of one teardown, which ADR-0270 already bounds per step), or the teardown
moves to the reaper and is charged to no one. Until then, record it in ADR-0285's open questions (ADR-0001 rule 6, which
agents may write); `docs/backlog.md` alone is not where the ADR's reader will look.

### 6. warning (should-fix): t_thread's miss path accepts every success, including the defect the case exists for

*Where:* `user/src/bin/t_thread.rs:624-648`.

*What:* the case guards against a creating call that writes its handles after a sibling unmapped the buffer. A call
that returns `Ok` is now a miss whatever the sibling did. A kernel that sometimes ignores the unmap (a stale
translation after the pause, the TOCTOU the test is for) passes unless it does so 57 times in a row before 8 genuine
failures, and when it always does, the message blames the host ("the sibling never unmapped in time"). Also the miss
branch (closing two handles, `made == 2`, the charge check after a miss) is reached only under a stalled host, so no
run exercises it.

*Scenario:* a regression in which the creating call re-validates the buffer only on a cold heap: half the rounds succeed
with the buffer unmapped; the case prints `8 of 16 failed (8 missed)` and passes.

*Fix:* read `ENDS_GO` immediately after the call returns, before `FORCE_SWITCH` is cleared. A miss is legitimate only if
the sibling had not finished (`ENDS_GO != 2`); `Ok` with `ENDS_GO == 2` is a failure. Under HVF (no stall) assert
`missed == 0`. Force the miss branch once (a round with `FORCE_SWITCH` off, where the call must succeed) so that its
handle closing and charge check run in every run.

### 7. note: the cleanup is a single pass over a tree whose jobs stay open

*Where:* `kernel/src/job.rs:530-541`, `kernel/src/testing.rs:645-653`.

*What:* `end_every_process` asks each process of the tree to exit once and leaves the jobs open. A process that spawns
between the walk and its own exit (a shell or a fan-out program that failed mid-way) leaves a child that never gets a
request, and the loop waits its full 10 s. *Fix:* repeat the walk in the wait loop (every 100 ms, say), or close the
jobs as `kill` does and reopen them after.

### 8. note: the workflow step

*Where:* `.github/workflows/test.yml:410-419`.

Sound: the name `test-logs-<job-index>-attempt-<run_attempt>` is unique per matrix entry and attempt (a re-run of failed
jobs raises `run_attempt`); no secret or token is used or written; `if-no-files-found: ignore`; 7 days; the logs are the
runner's, of a guest that has no secret. Notes: `if: failure()` does not run when the job is cancelled by its own
`timeout-minutes`, which is the case where the logs would help most (use `if: failure() || cancelled()`, bounded by the
grace period); the index does not name the command, so the job's log is needed to map one to the other (put `matrix.cmd`
in a file in the uploaded directory); `build/ports/logs/*.log`, printed by the step before, is not uploaded.
`make check-workflow-syntax` and `make check-ci-jobs` pass (in `make check`).

## Checked and found sound

- `Job::end_every_process` is `#[cfg(feature = "qemu-test")]` (`job.rs:529`), and `testing.rs` is built only with that
  feature (`main.rs:36`); the test kernel alone has the cleanup. No kernel behaviour of an ordinary build changes.
  `resume` runs on a fresh boot stack with IRQs unmasked (`recover_to`'s SPSR), so the yield loop lets the period's
  deadline release parked threads (seen in E1: t_cpu's 64 capped processes all ended).
- `EXPECTED_FAILURES` 10 and the new `--expect-panic` agree; the hand mutant
  `K-harness-keeps-a-failed-tests-processes` matches one line.
- The counts-per-charge check does catch a room-specific under-charge under TCG (M3), which the old share bound only
  just did (530 against 500).
- `tools/test-runner/src/timing.rs`: parses `run`/`ok`/`fail` by the second word, keeps five in order, prints the
  running test at a timeout; its tests pass.
- t_thread's handle comparison by raw value, the reserved vectors (no heap growth inside the charge window) and the
  bound on misses are as described.
