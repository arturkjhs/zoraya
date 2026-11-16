# TASK-34 review: the TCG kernel-suite split (`work/tcg-split`)

Independent review by the `opus-review` subagent of `git diff origin/main...work/tcg-split` at 39b502a7, 2026-11-16,
read against the architect's decision of 2026-11-16 (split each TCG kernel suite into two jobs, keep the 420 s limit),
ADR-0213's Decision, ADR-0348, ADR-0350 (INV-0350-1, -2), ADR-0463, and `docs/backlog.md`'s section on the
contiguous create refused while the reaper holds the frames.

Run by the reviewer (`CARGO_BUILD_JOBS=2`, output to files; another session was building and testing at the time, so no
QEMU run was made):

- `make check`: exit 0, 27 of 27 (`check-test-shards`, `check-ci-jobs`, `check-ci-plan`, `check-test-lanes` among them).
- `make check-test-shards`: exit 0, `141 tests, shard 1 = 0..131, shard 2 = 131..141`.
- `make check-ci-jobs CI_TESTS=<copy>` on two altered copies of `tools/ci-tests.txt`: `tcg-el2-a` replaced by
  `auto-el2` fails (`does not run make test-qemu-tcg-el2-a`); `tcg-el2-b` removed fails (53 of 54 jobs). Both refused.
- CI logs read: F5's candidate run 43770158088 (the three TCG jobs, green), F6's run 43789776708 attempt 1 (its
  `tcg-smp4-gicv3` timed out) and attempt 2, and attempt 1's artifact `test-logs-5-attempt-1`.

Not run: any TCG run of a shard. The implementer's logs in `target/test-logs/tcg-smp4-gicv3-{a,b,full}.log` carry the
`tcg-` label but ran under HVF (`t_bound: ... HVF (by the CPU model)` in each). So the shards have not yet run under
TCG anywhere; the candidate run is their first.

Severities: **error** (wrong; fix before merge), **warning** (fix in this part or record it), **note**.

## Findings

1. **warning**: the six sharded jobs no longer check the boot stack's canary after the tests that overflow it. Where:
   `kernel/src/testing.rs:622` (`stack_canary_intact`, index 139, shard 2) against `:409`
   (`fault_report_for_stack_overflow`, index 86, shard 1, which sets `REARM_CANARY`) and `:728` (the re-arm in
   `resume`). Before the split each of these configurations checked, at the end of one boot, that the canary was
   re-armed after the deliberate overflow and that no later test overran the boot stack. Now shard 1 runs the overflow
   and 44 more tests and never checks; shard 2 checks a boot in which nothing overflowed. *Scenario:* a change breaks
   the re-arm, or a test in 87..130 overruns the boot stack only on 4 CPUs with GICv3 or at EL2: `tcg-smp4-gicv3-a` and
   `tcg-el2-a` stay green. (`tcg-default` and `auto-smp4-2g` still run the whole list in CI, so GICv2 at one and four
   CPUs keeps the check.) *Fix:* check the canary after the loop in `run()` (`kernel/src/testing.rs:695`), outside any
   test, so that every run, sharded or not, ends with it; a failure there panics outside a test and fails the run. Or
   register a second canary check as shard 1's last test.

2. **note**: shard 2's expected lines are a hand copy of four of `EXPECT`'s lines and leave out two that shard 2
   prints. Where: `Makefile:307-310` (`EXPECT_B`) against `Makefile:219-243`. `uart interrupt 33` (the banner,
   `kernel/src/main.rs:230`) and `hello from EL0` (the final `hello`) are printed in a shard-2 boot
   (`target/test-logs/tcg-smp4-gicv3-b.log` lines 11 and 187) but not expected there. A banner line changed later in
   `EXPECT` is not changed in `EXPECT_B`. *Fix:* split `EXPECT` into the boot lines (`EXPECT_BOOT`) and the panics and
   fault lines, with `EXPECT := $(EXPECT_FAULTS) $(EXPECT_BOOT)`, and give shard 2 `$(EXPECT_BOOT)`.

3. **note**: `--shard` is not checked by `tools/run-qemu.sh`. Where: `tools/run-qemu.sh:144`, `:245`. It is taken
   without `--test` and with any value, and with `--dumpdtb` (no `-kernel`) it gives QEMU an `-append` without a
   kernel, which QEMU refuses. The kernel side is safe (see below), and the Makefile's expected `shard:`
   line catches a value the kernel ignores. *Fix:* `case $2 in [0-9]*/[0-9]*) ;; *) die ...` and refuse `--shard`
   without `--test` or with `--dumpdtb`.

4. **note**: a malformed shard word runs the whole list without a word. Where: `kernel/src/testing.rs:664-672`.
   `zoraya.shard=1/2x` or `zoraya.shard=1/2/3` makes `shard_argument` return `None` and the run covers all 141 tests
   with no `shard:` line; only the Makefile's `--expect-line "shard: N/2 ..."` turns that into a failure, after a full
   run. `3/2` and `0/2` panic before the begin line, which fails the run at once (good). *Fix (optional):* when a
   `zoraya.shard=` word is present but does not parse, panic as `3/2` does.

5. **note**: `tools/check-test-shards.sh:15` finds the registered names only in rustfmt's multi-line form
   (`name: "...",` on its own line). A struct literal short enough for rustfmt to keep on one line (`Test { name: "x",
   run: x }`, body at most 18 characters) is invisible to it, so a duplicate in that form passes the uniqueness check,
   and the kernel's `position()` would cut at its first copy. `fmt-check` keeps every current entry multi-line, so
   this is theoretical. `:27` counts every `--expect-panic` at the start of a Makefile line, in any target; a second
   kernel's expected panic would make the `EXPECTED_FAILURES` comparison fail (fails safe). *Fix (optional):* also
   match `name: "([^"]+)"` anywhere in the block.

6. **note**: `docs/design.md:2841` still names the configuration `tcg-el2` ("covered only by TCG (`tcg-el2`, ...)");
   it is now `tcg-el2-a`/`-b`.

7. **note**: shard 2 holds the most variable test of the list. `kernel_work_is_bounded` took 28.8 to 36.7 s in the
   GICv2 and EL2 jobs, 69.5 to 74.0 s in the GICv3 job of a fast run and 116.9 s in F6's slow GICv3 run, and it repeats
   a measurement up to three times when a masked interval is over its bound. The cut is still right (section below);
   the point is that a shard-2 job near its limit will be this test, and the job's time is the thing to watch in the
   first nightly runs.

## TCG balance (question 4)

The runner prints a run's five slowest tests; CI's logs drop the middle of the output, so the other per-test times are
not in them. What the logs give (seconds; S2 = in shard 2):

| run | total (first guest line to verdict) | kernel_work_is_bounded (S2) | job_signal_... | threads_and_futexes_... | job_suspend_... (S2) | cpu_ceiling_... (S2) |
|---|---|---|---|---|---|---|
| F6 gicv2, 139 tests | 225 | 30.7 | 38.0 | 30.0 | 28.9 | 18.8 |
| F6 gicv3 (attempt 2) | 314 | 74.0 | 42.3 | 32.5 | 28.9 | 18.7 |
| F6 el2 | 246 | 36.7 | 40.8 | 29.7 | 30.1 | < 20.6 |
| F6 gicv3 attempt 1 (slow) | timeout 420, in `snapshots_from_a_process` after 6.2 s | 116.9 | 58.8 | 43.2 | 32.1 | < 24.9 |

In the slow run the timeout came in shard 2's eighth test. The shard-2 tests that had run by then took at least
116.9 + 32.1 + about 19 (`t_cpu` runs fixed 2 s windows, 18.7 to 18.8 s in every fast run) + 6.2, so about 175 s, plus
`quotas`, the two room tests and `room_keeper`. Shard 1 therefore took at most about 420 - 175 = 245 s on that slow
runner, and shard 2 would take about 175 s plus those four tests, the rest of `snapshots_from_a_process` and the big
spawn: roughly 200 to 250 s. On the fast GICv3 run the same reckoning gives about 150 to 165 s each. So by TCG time the
cut at index 131 is close to even; the 131-to-10 split by count is misleading, not lopsided. Both shards keep about
40% margin under 420 s on the slowest runner seen.

Other cuts: a cut must stay after `memory_objects_from_a_process` (index 110), and any cut in 111..130 moves
`channel_measurements` (up to 24.9 s), storage, GUI, network and image tests into shard 2, the side that already holds
the variable test. A cut at 133 (`kernel_work_is_bounded` first) moves about 35 s (`job_suspend`, `quotas`) into shard 1,
giving roughly 280 against 165 s on the slow runner. Keep `SHARD_2_FIRST = "job_suspend_from_a_process"`.

## The contiguous-create flake (question 5)

The order around `t_bound` is unchanged: in both the whole list and shard 2 it follows `job_suspend_from_a_process` and
`quotas_from_a_process` directly, and the frames the reaper holds when its contiguous create runs come from `t_bound`'s
own earlier steps (the 24 MiB process, the 400 objects, the 4000 mappings). What shard 2 removes is the 131 earlier
tests' residue: a heap grown and kept in scattered frames, and fragmentation. That can only make a contiguous run easier
to find, so the refusal is as likely or less likely in shard 2, not more. The flake is not fixed by this (backlog,
ADR-0270). The implementer's whole-list HVF run (`tcg-smp4-gicv3-full.log`) shows the other `t_bound` failure, `run 3
went over the time limit` (masked time on a busy host), which the order does not affect either.

## Checked and found sound

- **Normal boot unchanged.** `testing` is `#[cfg(feature = "qemu-test")]` (`kernel/src/main.rs:36`); the normal kernel
  never reads `/chosen/bootargs`. Without `--shard`, QEMU writes no `bootargs` (it does only for a non-empty command
  line), `shard_argument` returns `None`, and the test kernel runs `0..TESTS.len()` with the same begin line as before.
- **Decoder total and bounded.** `Property::as_str` refuses a value without its final NUL or with an inner one and
  non-UTF-8 (`crates/zoraya-dtb/src/lib.rs:666`); `split_whitespace`, `strip_prefix`, `split_once` and `parse` are
  bounded by the property's length, and every failure is `None`. `n`, `m` other than 1 or 2 of 2 panic before the begin
  line, outside a test. No loop depends on the guest-supplied value.
- **`-append` and the run-qemu rule.** The option is added inside `tools/run-qemu.sh`, the one script that invokes
  QEMU, as one argument through `set --` (no shell reparse).
- **Harness.** `END` is set once on the first call and used by every resume after a panic (`run(i + 1)`), so an
  unexpected panic in shard 2 resumes inside shard 2. The begin line announces `hi - lo`; the end line's `pass`/`fail`
  count only that shard's tests, which the runner checks against the lines. Tests of the other shard are not run and
  not reported; that is consistent with `begin`. Every `--expect-panic` test and `harness_resumes_after_panic` (which
  asserts `FAILED == EXPECTED_FAILURES`) are in shard 1, and shard 2's runner has no `--expect-panic`, so a panic there
  fails the job; the runner also fails a `--expect-panic` test that did not run (`protocol.rs:288`).
- **Shard 2's order dependence.** The ten tests were read: `job_suspend`, `quotas`, `kernel_work`, `cpu_ceiling` and
  `room_keeper` each run one program and check its exit; the two room tests build their own jobs and reset the test
  period (`run` sets it to 0 before each test); `snapshots_from_a_process` compares its second run with its first
  (`twice_with_nothing_left`), not with what earlier tests left; `a_spawn_with_a_big_space_is_bounded` checks the free
  frames before each spawn and is still last, with more free memory than before. The state the earlier tests leave
  (heap growth, the volume, the signal table) is deterministic, so the HVF run of shard 2 (`pass=10 fail=0`) shows no
  hidden dependence on it; a speed dependence would come from leftover background work, and shard 2 has less of it.
  Each CI job makes a fresh test disk (`test-qemu-disk`), and `zoraya-fsck` runs after each shard.
- **`check-test-shards`.** The partition is by construction (one list, one cut, `position()` of a name the script has
  proved unique and registered); the script checks uniqueness, the cut's existence, that every expected panic is before
  the cut, and `EXPECTED_FAILURES` against the `--expect-panic` count. A renamed or removed cut fails both the script and
  the kernel's `expect`. It is in `CHECK_STEPS`, `TEST_STEPS`, the workflow's check matrix and `tools/test-map.txt`.
- **CI lists.** Six steps replace three, each with `RUN_TEST_SLOW`/`RUN_TEST_SLOW_B` (420 s), the old machine options
  and expected lines, and its shard line. `QEMU_AUTO_TWINS := ... auto-el2:tcg-el2-a` lets `auto-el2` be left out where
  `auto` is TCG while both `tcg-el2-a` and `-b` stay required as configurations of their own (both altered lists above
  were refused); `print-legs` with `HOST_ACCEL=tcg` leaves out only `auto-default` and `auto-el2`. `auto-smp4-2g` has no
  twin and none of the `tcg-smp4-*` steps is one. `tools/ci-plan.sh` takes every `test-qemu-*` line for the test-qemu
  step; `tools/ci-last-full.rb` looks only for the job `build`; the full run's jobs stay under the API's 100 a page
  (54 test lines and 27 checks). `test-par` runs test-qemu as one lane, so the split adds no parallel guest. No other
  tool or workflow names the old steps.
- **CLAUDE.md.** No host paths, user names or e-mail addresses in the diff; `docs/design.md` describes the shards.

## Verdict

No error. Merge after a green candidate run of the six new jobs (their first TCG run), with finding 1 fixed or recorded
as a known limitation in `docs/design.md`; findings 2 to 6 can follow.
