# TASK-29: audit of the tests and of the CI

Written 2026-11-14 on the tree of `main` at `5e9bbabf`. Part 1 (sections 1 to 6) changed nothing. Part 2 (the fixes, on
`work/test-audit`) is done: see "Status for the architect" and "What part 2 did".

How to read the evidence. A run or job number is a GitHub Actions id of the workflow `make test`. The run history
(693 runs created since 2026-10-31, 716 attempts, every job with its steps) and the logs of the 245 failed jobs were
fetched with `scripts/fetch1.sh` and `scripts/fetchlog.sh` and summarised with `scripts/analyze.py`,
`scripts/failing.py` and `scripts/why.py`; the tables are in `data/`. The raw files are not committed (61 MB of logs).
"Read, not run" means a finding comes from reading the test and the code that prints the text: this machine runs no
QEMU, so no test was broken to show a finding. Part 2 says, fix by fix, what was shown failing and what could not be.

## Status for the architect

**The branch `work/test-audit` is ready to merge**, at `5486ff7d` plus this report's commit (documents only). Its
candidate run 43579834963 (`candidate/work-test-audit-b`, on `5486ff7d`) is green: 66 jobs, all passed. Part 2 was
reviewed by an `opus-review` reader that did not write it (`review-notes.md`: 7 warnings, 6 notes, no error; 11
fixed, R5 in part, R12 left). It has not been merged: the TASK-27 dispatcher merges it like a part. `origin/main`
was merged in at `ce12282e`; `main` had not moved when the candidate was pushed.

**Time, measured.** "Before" is the median of 12 green runs of the 47-job list since 2026-11-13 18:00 UTC (worst in
brackets). "After" is the one green candidate run, which ran every job because the branch changes `Makefile`.

| | Before | After (run 43579834963) |
|---|---|---|
| Test jobs | 47 | 42 |
| Runner time, summed | 284 min (296) | 209 min |
| Runner time as billed (each job rounded up) | 318 min | 243 min |
| Wall time, first job's start to the last job's end | 44 min (57) | 31 min |
| Build job | 14 min (16) | 11.4 min |
| Longest test job | 11.5 min (the two fuzz jobs) | 12.0 min (`test-net`; then 8.9, 8.7, 7.0) |
| The two fuzz jobs | 11.5 min each | 2.5 and 2.6 min (60 s a target on a candidate) |

A full run on `main` keeps the 600 s of fuzzing, so it will cost about 18 runner-minutes more than this candidate
(estimate) and its longest jobs stay the fuzz jobs. One run is one sample: the wall time depends on the queue (the
first candidate, 43575482537, spent 46 minutes queued and took 51). The cargo cache action (T2) restored nothing in
this run ("No cache found") and saved without the old error; whether later runs restore it has not been seen.

**Flakes.** Twelve classes in two weeks (section 3); eight were already fixed on `main`.
- Fixed here: F5, the `test-boot` flood step (`tests/shell/console.script`): an order race in the script, not slow
  echo. The new step passed on all seven machines of both candidate runs. The review notes that nothing forces the
  burst to overflow while the job is in front (R5); if it ever times out there, that is the premise to look at.
- Removed with the duplicated legs: the `auto` runs that kept the 180 s limit under TCG where their twin had more
  (part of F8).
- Left: F8's other GUI timeouts (no single cause found); F2 (`harness_resumes_after_panic` counting 10 for 9, not
  seen since 2026-11-09, cause not established); N6's `NoMemory` (backlog, a defect or a wrong premise, not seen on
  CI in these two weeks).

**A defect found by a corrected test** (in `docs/backlog.md`, not fixed, outside the tests): in the POSIX session,
Ctrl-C does not end a job that was stopped, continued with `fg` and has gone on to a later child: `sleep 30` ran its
30 s and the status was 0, not 130 (run 43575482537, `posix-scenarios-tcg`; one run, under TCG, cause not looked
into). Ctrl-C half a second after `fg` does end it, and that is what `scenarios.script` had always sent, because its
waits for `first` and `second` matched the line `fg` echoes. The step is that early Ctrl-C again and says so; no
test covers the late case.

**Vacuous tests fixed** (each commit says what was shown failing; the script edits could not be run here and were
first run by the candidates):
- V1: the runner's `~` and `!` steps did not unescape their text; three checks could never fire. Unit test failed
  before, passes after; the three steps passed in both candidates.
- V6 (in part, see the defect above), V7, V8, V14, V20, V21, V22: expected texts that the echo of the typed command
  or the failure also gave, and forbidden texts nothing prints, in nine scripts. V14's correction showed that
  `keeper.script` expected the killed job's line in the wrong place (first candidate, `test-keeper`); corrected.
- V29: the filesystem crash tests assert floors for the images they check (shown failing with a floor above the
  count).
- V34: on CI a failed networking probe fails the step instead of skipping it (shown with a failing QEMU).
- V41: `make check-mutant-lists` runs in CI.
- From the review: `console.script` forbids a prompt before the child's first line; `zosh.script` counts
  `tstp: continued` once.

**Also in the branch:** D1 to D3 (one run where `auto` is TCG) with proposed **ADR-0350**, INV-0350-1 and
INV-0350-2 checked by `make check-ci-jobs`; T3 (60 s of fuzz on a candidate unless the map sends its change to a
fuzz step); T2 (the cargo cache action).

**For the architect to decide or commission** (details in sections 4 to 6):
1. ADR-0350 is proposed: INV-0213-3 says "under every accelerator". Until it is accepted the branch deviates from
   an accepted ADR, openly.
2. The defect above (Ctrl-C and a continued job).
3. The build job relinks `libc.a` in every build, so every port is relinked and Python rebuilt (T1); the fix is in
   `ports/Makefile`, which this machine cannot build.
4. `Makefile` is not in `tools/test-map.txt` and nearly every part edits it, so nearly every candidate runs
   everything (T4).
5. `docs/adr/invariants.md`: 55 rows say `not yet applicable` for code that exists, and `adr-check` accepts any
   text after `checked:` (C1, V40).
6. INV-0172-4 (a room holds only what its creator held) and INV-0230-1 (tests reach no outside network) have no
   check at all (C2, C3).
7. Not fixed, listed in section 4: V2, V3, V4, V9 to V13, V15 to V19, V23 to V28, V30 to V33, V35, V36, V42 to V45,
   and the `t_quota` handshake of section 3.

The task file `docs/tasks/TASK-29-test-and-ci-audit.md` is not in the repository: it was an untracked file of the
audit's clone and is left for the architect to commit.

## 1. What CI runs

Sources: `.github/workflows/test.yml`, `tools/ci-plan.sh`, `tools/plan-lib.sh`, `tools/ci-tests.txt`, `Makefile`.

Jobs of one run:

- `image` (finds the CI image, median 8 s) and `plan` (median 16 s; not on a branch push).
- `checks`: 19 jobs, one `make` command each (`test.yml:286-305`), on every push; median 30 s each, of which 22 s is
  "Initialize containers" (`data/steps.md`). `make check-mutant-lists` is in `make check` but not in this matrix (V41).
- `build`: `make fetch-ports` and `make ci-build`, packed into one artifact. Section 5, T1.
- `tests`: one job per line of `tools/ci-tests.txt` (47 lines), or the part of it `ci-plan.sh` picks.

Which set runs (`test.yml:114-126`, `ci-plan.sh`): a push to `main` (unless only `docs/` changed), the nightly
schedule, `candidate-full/*` and a manual run with `full=true` run all 47; `candidate/*`, a pull request and a manual
run without `full` run what `tools/test-map.txt` gives the changed paths plus a smoke set; any other push runs only
`image` and `checks`.

**The accelerator.** `tools/run-qemu.sh:150-156` resolves `auto` to `hvf` only on an Apple Silicon Mac with
`kern.hv_support`; on a GitHub runner it is `tcg`. So every QEMU leg in CI runs under TCG, whatever its label says.
`CI=true` makes `FULL=1` (`Makefile:45`), which makes `LOCAL_ACCELS` `auto tcg` (`Makefile:46-50`) and turns on every
`$(FULL_ONLY)` line.

The 47 test jobs, with their times over the runs since 2026-11-12 (the job names changed before that; `data/failures.md`
and `scripts/analyze.py` cover the whole two weeks). "Job" is the whole job, "Run" its `Run` step; the difference,
about one minute, is container start, artifact download and unpacking. Legs are the runner labels of
`data/legs.md` (run 43553496171).

| Job (`make ...`) | What it runs (all under TCG on GitHub) | Job median | Job worst | Run median |
|---|---|---|---|---|
| `test-fuzz FUZZ_SECONDS=600 FUZZ_JOBS=5` | 5 libFuzzer targets, 600 s each, host only | 11:33 | 12:38 | 10:28 |
| `test-fuzz-net FUZZ_SECONDS=600 FUZZ_JOBS=3` | 3 targets, 600 s each, host only | 11:29 | 12:53 | 10:23 |
| `test-posix` threads.script, `auto tcg` | the script twice | 11:02 | 15:00 | 9:18 |
| `test-posix-threadtest3` | SQLite threadtest3 twice (`LOCAL_ACCELS`) | 10:02 | 12:59 | 8:49 |
| `test-net` | test kernel with net twice, 2 shell scripts (one twice), fetch twice, posix twice, lookup | 9:51 | 14:15 | 8:54 |
| `test-gui` rooms, limits, spinners, files, files-manage, each `auto` and `tcg` | 10 boots | 9:47 | 11:25 | 8:37 |
| `test-gui` terminal:2 `auto` and `tcg`, windows:auto:2, wheel:1 `auto` and `tcg` | 5 boots | 8:21 | 9:42 | 6:36 |
| `test-apps` | apps and refusals scripts twice each, launches once | 7:42 | 13:33 | 6:32 |
| `test-posix` w9-term.script, `tcg` | one boot | 7:23 | 11:11 | 6:26 |
| `test-posix` w9-term.script, `auto` | the same boot | 7:11 | 9:56 | 6:15 |
| `test-gui` terminal, windows, layouts, settings, net, each `auto` and `tcg` | 10 boots | 7:04 | 9:15 | 5:51 |
| `test-gui` terminal 1920x1200 and 1600x1000, files-manage:2, wheel:2 (`auto` only) | 4 boots | 6:52 | 8:46 | 5:47 |
| `test-unit` | `cargo test` of the workspace's host crates | 5:44 | 7:06 | 4:46 |
| `test-posix` w8-j1, w8-pipe-j2, w8-fifo-j2, w8-pipe-j4, w8-fifo-j4 (5 jobs, `auto`) | one W8 build each | 5:20 to 5:44 | 8:17 | 4:04 to 4:49 |
| `test-tour-gui POSIX_ACCELS="tcg"` | tour-gui and room-gui | 5:29 | 12:32 | 4:34 |
| `test-tour-gui POSIX_ACCELS="auto"` | the same two | 5:29 | 7:38 | 4:26 |
| `test-python` | smoke scripts and TLS clients | 5:12 | 27:08 | 4:11 |
| `test-qemu-auto-smp4-2g` | kernel tests, 4 CPUs, 2 GiB, GICv2 | 5:04 | 7:54 | 4:01 |
| `test-qemu-tcg-smp4-gicv3` | kernel tests, 4 CPUs, 2 GiB, GICv3 | 5:00 | 7:53 | 4:00 |
| `test-python-regr` | regrtest's CI subset | 4:53 | 6:11 | 3:43 |
| `test-qemu-tcg-smp4-gicv2` | kernel tests, 4 CPUs, 128 MiB | 4:46 | 7:30 | 3:39 |
| `test-qemu-tcg-default` | kernel tests, GUI devices | 4:44 | 7:11 | 3:32 |
| `test-posix` w9-int.script, `auto tcg` | the script twice | 4:43 | 6:03 | 3:37 |
| `test-qemu-auto-default` | the same machine as tcg-default | 4:30 | 6:51 | 3:33 |
| `test-qemu-tcg-el2` | kernel tests, entry at EL2 | 4:28 | 6:46 | 3:22 |
| `test-qemu-auto-el2` | the same machine as tcg-el2 | 4:25 | 6:31 | 3:21 |
| `test-netsurf` | NetSurf at scale 1 and the text script at scale 2 | 4:25 | 9:11 | 3:00 |
| `test-posix` w10.script, `auto tcg` | twice | 3:43 | 5:53 | 2:39 |
| `test-posix` hello.script and w5.script, `auto tcg` | each twice | 3:21 | 4:56 | 2:28 |
| `test-posix-threads` | t_threads twice | 3:11 | 4:29 | 1:58 |
| `test-files` | 11 shell scripts, once each (`auto`) | 3:10 | 6:17 | 2:09 |
| `test-posix-gui` | gui.script twice | 2:58 | 4:40 | 2:01 |
| `test-boot` | 4 scripts once, console.script on 7 machines | 2:37 | 4:16 | 1:40 |
| `test-posix` tour.script, `auto` | one boot | 2:31 | 4:08 | 1:29 |
| `test-posix` tour.script, `tcg` | the same boot | 2:30 | 3:41 | 1:27 |
| `test-hand-run` | run, run-net, run-gui as a person starts them | 2:26 | 3:37 | 1:22 |
| `test-posix` zosh.script, `auto tcg` | twice | 2:25 | 4:56 | 1:30 |
| `test-posix` scenarios.script, `auto tcg` | twice | 2:24 | 4:49 | 1:30 |
| `test-posix` invariants.script, `auto tcg` | twice | 2:13 | 3:26 | 1:17 |
| `test-crash` | 10 crash rounds (host tool starts QEMU) | 2:09 | 3:17 | 1:09 |
| `test-libposix` | t_libposix twice | 1:54 | 3:34 | 0:55 |
| `test-keeper` | keeper.script twice | 1:48 | 3:09 | 0:48 |
| `test-posix-suites` | libc-test's thread tests twice | 1:43 | 3:35 | 0:44 |

The sum of the 47 medians is 246 min; `build` adds 8 to 16 min (T1) and the checks about 10.

## 2. Duplicates

On a GitHub runner an `auto` leg and its `tcg` leg start QEMU with the same command line. I looked for anything that
could still differ:

- **The guest.** Nothing in `kernel/`, `user/` or `crates/` reads the accelerator. The time bounds of `t_cpu` and
  `t_bound` come from a calibration the guest makes itself (`user/src/bin/t_cpu.rs:51-66`,
  `user/src/bin/t_bound.rs:54-77`), so both legs get the same factor.
- **The runner.** `runs_under_tcg` asks `run-qemu.sh --print-accel` (`tools/test-runner/src/gui.rs:1406-1417`), so an
  `auto` leg gets the TCG `latency` bounds and key pacing too.
- **Timeouts.** This is the one real difference, and it is against the `auto` leg: several `auto` legs keep
  `QEMU_TIMEOUT` (180 s, set for HVF) while the `tcg` leg has more: `test-keeper` 180 s against 420 s
  (`Makefile:1245`, `1248`), `test-net-posix` 180 s against 400 s (`Makefile:518-519`), `test-posix-gui` and the GUI runs at scale 1 180 s for both. Under TCG the shorter limit tests nothing more;
  it only fails earlier on a slow runner (F8).
- **Machine options.** Identical for every pair below; the three `test-qemu` configurations that are not pairs are
  listed as such.

| # | Duplicate | Where | Seconds in run 43553496171 | What dropping one leg loses |
|---|---|---|---|---|
| D1 | `test-qemu-auto-default` = `test-qemu-tcg-default`; `test-qemu-auto-el2` = `test-qemu-tcg-el2` | `Makefile:234-272` | two whole jobs: 4:30 and 4:25 at the median | Each side has `--expect-line`s the other lacks (`cpus: 1`, the memory and console lines on the `auto` side; the GICv2 lines on the `tcg` side). Move the `auto` side's lines to the `tcg` target and nothing is lost. |
| D2 | whole jobs: `test-tour-gui` `auto` and `tcg`; w9-term.script `auto` and `tcg`; tour.script `auto` and `tcg` | `tools/ci-tests.txt:21-22`, `36-37`, `43-44` | 5:29, 7:11 and 2:30 at the median | A second sample of the same run. |
| D3 | legs inside a job: POSIX scripts with `POSIX_ACCELS="auto tcg"` (hello, w5, w9-int, w10, invariants, scenarios, threads, zosh); the `LOCAL_ACCELS` loops (`test-libposix`, `test-posix-threads`, `test-posix-suites`, `test-posix-threadtest3`); `test-posix-gui`; the 12 `X:auto:N` and `X:tcg:N` pairs of `test-gui`; `boot-tcg` and `boot-tcg-el2` in `test-boot`; `keeper-tcg`; `net-boot-tcg`, `net-tcg`, `net-fetch-tcg`, `net-posix-tcg`; `apps-tcg` and `apps-refusals-tcg` | `tools/ci-tests.txt:25-42`; `Makefile:919`, `937`, `959`, `983`, `1159`, `854`, `856`, `1247`, `339-340`, `518-519` | 2050 s summed: threadtest3 291, threads.script 277, the GUI pairs 536, test-net 284, test-apps 220, w9-int 98, w10 63, w5 and hello 60, posix-threads 47, posix-gui 44, scenarios 37, zosh 29, invariants 24, libposix 19, test-boot 12, suites 7, keeper 2 | A second sample. Keep the `tcg` leg where the two timeouts differ. |

Not duplicates: `test-qemu-auto-smp4-2g` (4 CPUs, 2 GiB, GICv2) has no twin: `tcg-smp4-gicv2` has 128 MiB and
`tcg-smp4-gicv3` GICv3. `boot-auto-smp4-2g`, `boot-tcg-smp4-gicv2` and `boot-tcg-smp4-gicv3` differ the same way. The
GUI runs at scale 2 and at the two sizes have only an `auto` leg, except `terminal:2`.

Total: about 54 runner-minutes of a full run's 284 (24 min of whole jobs, 30 min of legs), 5 jobs fewer, and three of
the four longest QEMU jobs fall from about 10 min to about 5 to 6. The review of part 2 (R4) found that one leg of
D3 is not a duplicate: `test-net`'s `net-tcg` (237 s) boots on the disk `net-settings-auto` left, with a gateway and
a DNS server set, so it stays, and `test-net` keeps its length. The first count here was 58 minutes with it.

**What a second sample is worth.** It is not nothing: an intermittent failure shows twice as often. Of the classes in
section 3, F1 and F2 failed in one leg and passed in the other inside the same job. But a duplicate is an expensive
way to buy samples, it makes every job that has one twice as likely to fail on a flake, and the nightly run samples
every night.

**On the Mac nothing changes:** there `auto` is HVF, `make test` and `make test-changed` keep `auto`, and `FULL=1`
keeps both.

**This needs an ADR.** ADR-0110 item 6 and ADR-0213's Decision item 4 with INV-0213-3 say CI runs "every script of
`test-posix` under every accelerator", and `make check-ci-jobs` enforces it (`tools/check-ci-jobs.sh:56-70`) together
with every `test-qemu` configuration and every run of `GUI_RUNS_ALL`. Part 2 writes ADR-0350 (proposed) and changes
the check to accept a `tcg` leg for an `auto` leg where the host's `auto` is TCG.

## 3. Flakes

Method: every job that failed in one attempt or run of a commit and passed in another (`data/flakes-same-sha.md`,
21 jobs), and every failed job's log classified by its failing line (`scripts/failing.py`; 245 failed jobs, 51 of them
checks, builds and unit tests that failed for the change's own reasons). A class counts as a flake if it passed on the
same commit, or failed across unrelated branches with the same line. For each I read the test.

| # | Failing line | Jobs, first to last | Runs (examples) | Cause class | State |
|---|---|---|---|---|---|
| F1 | `[posix-scenarios-auto] FAIL: timed out waiting for "Stopped"` | 14, 11-04 to 11-10, 5 of them passed on the same commit | 41853386993, 42680764773, 42686638904 (main), 42707071806, 42725195527 | Order assumption: Ctrl-Z was sent before the job held the terminal | Fixed, `ceaf05d1` (the script waits for the job) |
| F2 | `ZORAYA-TEST fail harness_resumes_after_panic ... left: 10 right: 9` | 37, 11-04 to 11-09, 5 passed on the same commit (`f984e215`: main failed tcg-smp4-gicv2, two branches failed other configurations) | 42543187652 (main), 42623787078, 42623787456 | Not established. The count of a kernel test differed by one between runs of one commit. No occurrence since 11-09 | Gone; I did not find the commit that fixed it. If it returns it is a defect to look into, not a timing bound |
| F3 | `timeout after 180 s (test snapshots_from_a_process did not finish)`, and the same for `kernel_work_is_bounded`, `job_suspend_*`, `room_keeper_from_a_process` at 180 s and 300 s | 28, 11-11 to 11-13; three configurations of run 43129188232 passed on attempt 2 | 43129188232, 43387452404 (main) | Timeout too short for the whole suite under TCG on a slow runner | Fixed, `c1e1b344` (`QEMU_SLOW_TIMEOUT` 420 s) |
| F4 | `user panic: panicked at user/src/bin/t_cpu.rs:180` (and `t_bound.rs`) | 22 and 12, 11-11 to 11-13 | 43337731455, 43394224522 (main, passed on attempt 2) | Wall-clock assumption under TCG: the masked-time limit | Fixed, `8b2fcaa3` (units are the check; 1 s wall limit off HVF) |
| F5 | `[boot-tcg-smp4-gicv3] FAIL: timed out waiting for "bytes dropped]"` | 4, 11-09 to 11-13 | 43437795275 (main, passed on attempt 2); TASK-26 report lines 307 and 805 | Order assumption in the script (below) | **Open. Test-only fix in part 2** |
| F6 | `[posix-threads] t_threads` case `waits`, line 837 | 1, 11-13 | on `candidate/work-9b-libc-e` | Fixed sleep: a 400 ms wait expired on a slow runner | Fixed, `924b576e` (30 s waits released by the test). What it still assumes is V30 |
| F7 | `[gui-wheel-auto-2] FAIL: step 77 of 77 (Exit): timeout`, and `Region(...)` pixel mismatches at step 66 | 8, 11-14 | 43553496171 (main), 43562400937, 43558055946 | Order assumption: a key after a focusing click raced the tablet's click | Fixed, `7607c761` and `06086f91` |
| F8 | GUI and script steps that time out at 180 s in an `auto` leg: `[gui-windows-auto-1] ... Screen("restored")`, `Screen("~> _")`, `[gui-files-manage-auto-2] ... Then(...)`, `[conventions] timed out waiting` | about 10 spread over 11-09 to 11-13 | 42703006772 (passed on attempt 2), 43481729065, 42982644440 (main) | Mixed. `ea11ae6d` fixed one (windows.script waits for the tenth window). The rest I could not tie to one cause from the logs; the `auto` legs run under TCG with the HVF limit | Partly open. D1 to D3 remove the legs with the short limit; the others stay on the nightly's watch |
| F9 | `[net-fetch-tcg] FAIL: timed out waiting for "Zoraya test page"` | 1, 11-12 | 43330650492 (main, passed on attempt 2) | Probably the missed wake-up of the first lookup, a real defect fixed in `ca2c6b4b`; not confirmed from the log | Fixed if that is the cause |
| F10 | `curl: (28) Failed to connect to ftp.gnu.org` in `build` | 6, 11-13 | 43511821485 (main, passed on attempt 2) | Outside: an upstream server | Fixed (the archive cache, 9f-ci) |
| F11 | `The runner has received a shutdown signal`; `build` exit 127 on attempt 2 of 43047183567 | 2 | 42632054208 (main, test-gui); 43047183567 | Outside: the runner | Nothing to fix |
| F12 | `user panic: panicked at user/src/bin/term.rs` | 27, all 11-09 | branches `work/7b` to `7e` | A defect of that day's branches, fixed there (`fix/term-no-panic`) | Not a flake |

Not seen on CI in these two weeks: `a_spawn_with_a_big_space_is_bounded` (`NoMemory`), the N6 row of `docs/backlog.md`.
It is described there as a property of the reaper's timing, so it is a defect or a wrong test premise, not something
for a longer timeout. It stays in the backlog.

**F5 in detail** (`tests/shell/console.script:212-217`; read by a reader and checked against the log of run
43437795275). The script sends `spin\r` and then 8192 bytes at once, and waits for `bytes dropped]`. The shell prints
that notice only when it next shows a prompt (`user/src/bin/zosh.rs:5013`, `5042`), never while a job is in front. In
the passing legs the log shows `[input: 7941 bytes dropped]` *before* `/> spin`: the shell had not yet read `spin\r`
when the flood arrived, so the drop happened at the prompt and the notice came with it. That is not what the comment
says the step tests ("during a job"). In the failing leg the shell read `spin\r` first and started the job; the notice
was then queued for a prompt the script never asks for, and the wait ran out. So the step passes only when it loses a
race, and `docs/backlog.md:165`'s reading (slow echo) is not the cause. The fix is in the script: start a job and wait
for a line it prints, send the flood, end the job, then expect the notice at the prompt. It needs a CI run to confirm.

Timing risks that have not failed yet are in `notes/vacuous-kernel.md` (S1 to S6: `t_cpu`'s lower bounds and lateness
bounds, absolute bounds in `t_thread`, `t_gui`, `t_quota`, `t_storage`, `t_streams`, the `portbench` ratio),
`notes/vacuous-posix.md` (T2: `twait` in `t_posix.c:1734-1766` has the 600 ms against 400 ms pattern that failed in
F6) and `notes/vacuous-gui.md` (the `pause 3000` before `net status`, the tour's `pause 1000`). One of them has a
fixed-sleep ordering that a test-only handshake can replace: `user/src/bin/t_quota.rs:436`, `449`.

## 4. Tests that can pass without testing

Found by six readers, one per area, each told to read the runner first and then, for every candidate, the code that
prints the text; their notes with every file and line are in `notes/`. All are **read, not run**. I checked V1, V2
and V20 myself against the source; the others I list as the readers confirmed them. "Suspected" findings of the
notes are left out here.

### The runner

| # | Finding | Where | Fix |
|---|---|---|---|
| V1 | `~` (quiet) and `!` (forbid) do not unescape their text, though the header says TEXT takes the escapes. `~ 400 \r\n/>` looks for a backslash, `r`, backslash, `n`, and can never fire | `tools/test-runner/src/script.rs:190`, `214`; users: `tests/shell/console.script:117`, `tests/shell/files-write.script:174` (INV-0152-2), `tests/posix/zosh.script:77` | `text_of` in both arms; a unit test |
| V2 | No host-side check of which kernel tests ran. `begin N` is the kernel's own `TESTS.len()`; a test deleted with its function disappears with a green run | `tools/test-runner/src/protocol.rs`, `kernel/src/testing.rs` | An `--expect-count` or a name list in the runner (touches the kernel's test list: proposal) |
| V3 | `--expect-panic NAME` accepts any panic of that test. Three expected failures can panic for another reason (`fault_report_for_unexpected_interrupt` and `..._thread_stack_overflow` end in `wait_until(\|\| false)`) | `protocol.rs:271`; `Makefile:189-197` | `--expect-panic NAME=TEXT` |
| V4 | `test-crash` passes when no round saw a `crash: durable` line | `tools/test-runner/src/crash.rs` | Fail when the durable total is 0 |
| V5 | GUI runner: `serial` matches anywhere in the log since boot; `region` passes if the colour is already there; `clickc` and `mark` do not move `then`'s mark | `tools/test-runner/src/gui.rs` | Use `then` in the scripts (V10 to V13); the runner is as documented |

### Expected text that is already there (the echo of the typed command, or an earlier line)

| # | Where | What it claims |
|---|---|---|
| V6 | `tests/posix/scenarios.script:46-53` | `< first`, `< second` after `fg` match dash's echo of the job's command line; "runs its next child after fg" is not required |
| V7 | `tests/posix/scenarios.script:100-102`, `90-91`, `12-13`; `tests/posix/smoke.script:7-8`; `tests/posix/w5-jobs.script:33-34` | `< outside`, `< done` and others match the echoed command ("a write outside the limit still works") |
| V8 | `tests/shell/disk-filler.script:48` (INV-0265-3), `system-ro.script:110`, `net-fetch.script:95`, `files-write.script:132`, `console.script:109`, `116`, `149-152` | `< home`, `< fine`, `< +net nonsense:`, `< hi there`, `< tick`, `< flood` are in the typed command |
| V9 | `tests/shell/system-ro.script:34`, `52` | `echo sync` asserts nothing; `sync` was probably meant |
| V10 | `tests/gui/files.script:71-72` | The drop's two lines are identical to lines 42-43, already in the log: a drop that hands over a writable file passes |
| V11 | `tests/gui/settings.script:25`, `27` | `serial settings: Saved and applied.` is satisfied by line 20 |
| V12 | `tests/gui/netsurf.script:115` | The grey `region` after Proceed is true before the click (the server's log is what proves it, since `057f3a4`) |
| V13 | `tests/gui/windows.script:20-31`; `rooms.script:35-41` | Minimising is never observed: if the click does nothing, the next step re-activates the one window and passes |

### The failure prints the expected text too, or the check is weaker than the claim

| # | Where | What passes |
|---|---|---|
| V14 | `tests/shell/rooms.script:76`, `78`; `keeper.script:76`, `78` | `< /t/w> ` also matches the prompt inside the room, `agent-1 /t/w> `: `room --leave` is not checked |
| V15 | `tests/shell/rooms.script:80`; `hostile.script:56` | `< home` after `room remove --kill`: the `home` row is always printed (`user/src/bin/zosh.rs:3566`) |
| V16 | `tests/shell/net-settings.script:29`; `disk-filler.script:53-54`; `console.script:123`; `python.script:41` | A prefix of the right line that the wrong line shares (`stopped after 0 KiB: create refused`, any handle table starting with slot 1, `ok ssl` with either boolean) |
| V17 | `tests/posix/scenarios.script:11-14` | "killed by SIGPIPE" checks only `head`'s status |
| V18 | `tests/gui/files-manage.script:120-124` | "A terminal in ~/docs." is printed whatever `launch` returned (`user/src/bin/panel.rs:784-793`); no window is read |
| V19 | `tests/gui/netsurf.script:33-38` | `/ns/result` is yellow for any query; the server's log is not read for the query |

### Forbidden text that cannot occur, or is looked for in the wrong window

| # | Where | Why |
|---|---|---|
| V20 | `tests/posix/scenarios.script:9`, `measure.script:7` | `! t_posix: FAIL`: the program prints `t_posix: NAME: FAIL line ...` (`tests/posix/t_posix.c:124`). A failure still fails, by the timeout |
| V21 | `tests/posix/threads.script:3`, `hello.script:4`, `smoke.script:2` | `! -FAIL`: nothing these scripts run prints it; a failure costs the whole timeout (900 s for threads) |
| V22 | `tests/shell/console.script:7` | `! no such command: hello`; the form is `zosh: hello: no such command` |
| V23 | `tests/posix-core/libposix.script:16`, `36` (INV-0153-7); `tests/posix/zosh.script:42`, `49` | `x TEXT` looks between the last two matches; the text would come after them (`user/src/bin/posixrun.rs:457-470`, `zosh.rs:1242-1288`) |
| V24 | `tests/shell/net-settings.script:17-18` (INV-0229-1); `exit.script:27`; `jobs.script:90`, `114`; `console.script:28` (INV-0061-2) | `x` is evaluated before the answer arrives, or its range ends before where the text would be |

### Steps that assert nothing, C cases without a check, host tests without an assertion

| # | Where | What |
|---|---|---|
| V25 | `tests/shell/console.script:105-106`; `files-write.script:12-13`, `207-210`; `rooms.script:29-37`; `apps.script:180-181`; `net-settings.script:31-32`; INV-0158-3 in `exit.script` and `jobs.script` | The comment claims a check; the step waits only for the prompt |
| V26 | `tests/posix-core/t_threads.c:637-656` (`stdio`) | No check at all, and no pipe despite the header |
| V27 | `tests/posix-core/t_threads.c:1384-1419` (`condsig`, INV-0304-8) | Never checks that the handler ran on the waiting thread; `rc_wakes` is counted and never read |
| V28 | `tests/posix/t_posix.c:1223-1225` (`tforkthread`) | `me = pthread_self(); pthread_equal(me, pthread_self())` |
| V29 | `crates/zoraya-fs/tests/crash.rs:167-251`, `decoder_fuzz.rs:219-262`; `crates/zoraya-surface/src/tests.rs:425` | Counts printed, never asserted above zero; `close_sends_close_and_frees_the_buffer` asserts nothing |
| V30 | `tests/posix-core/t_threads.c` case `waits` | `settle()` (150 ms, no handshake) is what puts the three threads into their waits; if they are not yet blocked the rounds pass with nobody blocked |
| V31 | GUI claims checked only by a screenshot nobody reads: `rooms.script:14`, `76-80`; `files-manage.script:110-118`; `limits.script:44-48`; `netsurf.script:83-88` | |
| V32 | `tests/gui/spinners.script:95` (INV-0264-3) | The 50 ms bound runs only under HVF; CI takes `tcg +450` for both legs. Accepted for CI by the architect (ADR-0298), so it is a statement of fact: CI does not check the 50 ms |

### Tests that do nothing on some machines, and skips that are green

| # | Where | What |
|---|---|---|
| V33 | `kernel/src/testing.rs` (device-gated tests) | `t_gui` runs in 2 of the 7 `test-qemu` configurations (both pinned by `--expect-line "t_gui: all passed"`); `t_net` and `t_netd` in none (they run in `make test-net`, pinned there); `t_snapshot`'s INV-0102-5 case returns silently without a device and nothing pins it |
| V34 | `Makefile:572` (test-apps), `678` (test-python, test-python-regr), `727` (test-netsurf), `799` (test-hand-run), and `test-net-fetch`, `test-net-posix` | `--probe-net ... \|\| { echo skipped; exit 0; }`: any failure of the probe, a broken QEMU included, is a green job on CI. Only `test-net` fails on CI (`Makefile:310`) |
| V35 | `Makefile:400` | `net-dns.script` never runs on CI (no workflow sets `NET_DNS`); it prints `SKIP`, which `tools/test-summary.sh` does not count |
| V36 | `tests/posix/smoke.script`; `make test-posix-long`; `make posix-cancel-points` | In no CI list and no `make test` step. `docs/adr/invariants.md` marks INV-0110-1, INV-0110-2 and INV-0005-3 `checked: make test-posix-long` |

### Checks and mutant lists

| # | Where | What |
|---|---|---|
| V40 | `tools/adr-check/src/main.rs:373` | Any non-empty text after `checked:` passes. INV-0042-1's row reads "by type and review; no command fails if it is broken" |
| V41 | `Makefile` (`CHECK_STEPS` against `TEST_STEPS`), `test.yml:286-305` | `make check-mutant-lists` is in `make check` only: CI never runs it, and `check-ci-jobs` compares `TEST_STEPS` only |
| V42 | `tools/mutants/src/main.rs:196-211` | Any non-zero exit is a kill unless rustc's "could not compile" is in the output: a patch that does not apply, a C compile error (`tls-options.toml` mutates `.patch` files), a watchdog kill or a flaky boot count as kills; timed-out and unviable mutants are not in `report.md` |
| V43 | `tools/mutants/*.toml` | 15 mutants in four lists no longer apply (known: `MUTANT_STALE`, `docs/backlog.md:154`); the other 251 apply (checked by script) |
| V44 | `fuzz/`, `tools/run-fuzz.sh` | Five targets (`mount_walk`, `raw_image`, `describe`, `shell_parse`, `editor_keys`) are in no list and never run; run counts are printed and never checked, so a target that rejects every input passes; CI keeps no corpus and no crash artifact |
| V45 | `Makefile:1411` (`GUI_SOURCES` of check-tokens); `tools/check-charged-alloc.sh`; `tools/check-fetch-cache.sh` | Fixed lists of files: a renamed file passes silently, four applications are not in the tokens list, and check-fetch-cache passes when it finds no `make fetch-ports` line at all |

Found sound, so that the list above is read in proportion: `t_libposix.c` whole; the W8, W9, W10, tour and invariants
scripts; the `/tests/*.sh` scripts (every failure reaches a `-FAIL` line or a status); the TLS log checks of
test-net-fetch, test-apps and test-python (they fail on a missing or empty log); the test-netsurf log check after
`057f3a4`, `616fe29` and `cdabf8d`; the 9 `#[ignore]`d tests (measurements, none cited by an invariant); of 2045
`#[test]` functions 38 have no assertion of their own and all but V29 assert through a helper; no host test skips on
a missing tool or file; every command named in `invariants.md` exists.

## 5. Time

**Where a full run's 44 minutes go** (`data/waves.md`, 12 green runs on the 47-job list): the build job ends 9 to 16
min after the run starts (one outlier at 23 to 27 min with a cold ports cache); no test starts before that. Then 47
jobs with 246 min of work run at most 20 at a time (the plan's limit; the peak was 20 in 7 of 17 runs and never above),
and the last starts 20 to 46 min into the run because other runs share those 20 slots. The longest job after that is
11.5 min (fuzz).

| # | Finding | Evidence | Cost today | What changes it |
|---|---|---|---|---|
| T1 | **The build job rebuilds what its caches hold.** In 21 of 21 successful builds since 2026-11-13 18:00 `libc.a` was relinked; curl, wget and NetSurf were relinked in all 21 and Python rebuilt in 19, the host Python too in the 6 where the ports cache hit. Cause: `libc.a` depends on `libzoraya_posix.a` (`ports/Makefile:174`), which cargo builds anew in every job, so it is newer than everything restored from the ports cache. The key of that cache (the hash of `ports/**` and seven ADRs, `test.yml:247`) hit in only 6 of the 21, since most parts change `ports/` | Logs of the 21 `build` jobs; in job 132327213914 (cache hit): apps 04:16:06 to 04:19:01, Python 04:19:01 to 04:22:43, of an 11 min `make ci-build` | Build 8 to 16 min, median 13; about 7 of them are relinks when the cache hits | Proposal (not safe to do blind from this machine): write `libc.a` only when its content changed, so that an identical library keeps its time; keep the host Python's prefix in the cache. Estimate: build about 5 to 6 min on a hit |
| T2 | **The cargo cache has never worked in these builds.** `Swatinem/rust-cache@v2.7.5` logs `Failed to restore: Cache service responded with 400` and `Failed to save` in every build read: that version speaks the cache API GitHub has retired | The same 21 logs; `test.yml:183`, `314` | Dependencies compiled in every `build` and `clippy` job; the two `user` profile builds take 46 s each, `cargo test --no-run` 88 s | Move to the current release (v2.9.3, 2026-10-20). Safe; part 2. The gain is to be measured: the action caches dependencies, not the workspace's own crates |
| T3 | **Fixed-duration fuzzing in every run.** `FUZZ_SECONDS=600` twice | `tools/ci-tests.txt:51-52`; 11:33 and 11:29 at the median | 23 runner-min, and the two longest jobs of every run | 60 s on merge candidates, 600 s in the full set. ADR-0298 item 3 says "the full CI list runs 600 s", which stays true. Part 2. Note V44: on a candidate the fuzz jobs run only if `fuzz/` or `tools/run-fuzz.sh` changed or the plan fell back to everything |
| T4 | **The plan rarely narrows a candidate.** 75 of the 91 candidates since 2026-11-12 ran 35 jobs or more. In the 30 most recent of those the plan's log names the unmapped path: `Makefile` in 26, with `Cargo.lock`, `Cargo.toml`, `tools/test-runner/src/gui.rs`, `tools/run-qemu.sh`, `user/src/lib.rs`, `crates/zoraya-protocol/src/lib.rs` and `tools/posix-stats/` in the others | Plan job logs of those 30 runs | A candidate costs a full run | Proposal: `ci-plan.sh` compares `make -n STEP` between the base's `Makefile` and the branch's, and a `Makefile` change runs the steps whose commands differ. `Cargo.lock` and shared files stay "everything". It changes what a candidate checks, so it is the architect's |
| T5 | **A minute and a half of overhead per test job.** Job median minus `Run` median is 55 to 70 s (container 22 s, artifact download 16 s, unpack 12 s); inside `Run`, the first leg carries make's and cargo's up-to-date checks of the phony kernel targets, 20 to 40 s (`libposix-auto` 46 s against `libposix-tcg` 19 s; `posix-suites` 30 against 7) | `data/steps.md`, `data/legs.md` | About 47 x 1.5 = 70 of the 284 runner-min | Merging the short jobs (test-keeper, test-posix-suites, test-libposix, test-crash, the short POSIX scripts) saves runner-minutes, not wall time; ADR-0213 wants one job a step. Proposal only |
| T6 | ADR-0213's target is "CI under about 10 to 12 minutes of wall time, and no job above 10 minutes" (Decision item 3). Today: 44 min, and five jobs above 10 min at the median or the worst case | Section 1 | | D1 to D3 and T3 bring every job under 10 min at the median (estimate); the wall time needs T1 and T4 as well |
| T7 | Waves: with 47 jobs and 20 slots a run alone needs three waves; two runs at once (a candidate and `main`) wait on each other | `data/waves.md` | | Fewer and shorter jobs (D, T3); at most one candidate at a time is already the rule |

**Cost after each change** (runner-minutes of a 47-job run; estimates from the medians of section 1 and the legs of
run 43553496171; the candidate run of part 2 gives the measured figure):

| Change | Full run on `main` | Candidate that runs everything |
|---|---|---|
| Today | 284 | 284 |
| D1 to D3 (one leg where `auto` is TCG) | about 230 | about 230 |
| T3 (60 s of fuzz on candidates) | about 230 | about 212 |
| T2 (cargo cache) | to be measured | to be measured |
| T1 (proposal) | minus about 5 to 7 min of wall time before the first test | the same |
| T4 (proposal) | unchanged | most candidates: the 60 to 170 min of a narrowed plan |

## 6. Coverage holes the audit happened to see

From `notes/coverage-holes.md` (about 40 security invariants read against their tests; not a coverage study).

| # | Hole | Evidence | Proposal |
|---|---|---|---|
| C1 | 55 rows of `docs/adr/invariants.md` say `not yet applicable (stage 6/7/8/9)` for code that is built, among them INV-0172-3 and -4, INV-0220-1 and -2, INV-0224-1 to -5, INV-0228-2 and -3, INV-0230-1 to -4. Some have tests the table does not name; some have none | The table against `docs/design.md` | A pass over those rows; a tools check that a stage marked done has no such row |
| C2 | **INV-0172-4, a room holds only what its creator held, has no test.** The refusal is at `user/src/bin/zosh.rs:3646-3658`; its message is in no script and no host test. Without it a shell in a room can run `room new wide +w /` | grep of the message | A case in `tests/shell/rooms.script` (test only) |
| C3 | **INV-0230-1, tests reach no outside network, is unchecked.** Seven recipe lines pass `--net-open`, which drops `restrict=on` (`tools/run-qemu.sh:286-287`); nothing asserts the command line, and CI has no network namespace | `Makefile:360`, `401`, `447`, `495`, `565`, `697`, `725` | A check on the runner's side or in the Makefile; needs a decision on what `--net-open` may reach |
| C4 | INV-0040-2 (every call refuses a handle lacking its right) is hand-written per type: no test for `device_interrupt` without `INTERRUPT` (`kernel/src/syscall.rs:1098`) or `interrupt_ack` without `WRITE` (`:1165`) | `kernel/src/testing.rs` | Two cases in the kernel's tests (test code inside `kernel/`) |
| C5 | `tools/check-untrusted-gate.sh` checks step names only. The gate's rows `hostile-programs`, `cpu-spinners` and `disk-filler` say "untrusted room" but their scripts use trusted rooms (the shell refuses `--untrusted` today), and INV-0172-2 is `checked` although its in-QEMU half cannot exist. The gate can open with no end-to-end run in an untrusted room | `tools/untrusted-gate.txt`, the three scripts | The architect's: the gate's wording or its tests |
| C6 | INV-0269-2's third clause (no room program holds a keeper channel) has no in-QEMU test; no script runs `probe` or `slots` inside a room | `tests/shell/rooms.script` | A step in rooms.script (test only) |
| C7 | INV-0295-2 and INV-0091-2 (a driver gets only its device's transport) are positive-path only: `tests/gui/net.script` passes if `init` also hands a driver the whole page | `tests/gui/net.script` | Needs a test-build print in `init` or the drivers |
| C8 | Lower: the in-QEMU half of INV-0093-2 sends only the stage 5 ordinals; `t_open` prints "refused" on any write error; `probe` omits `SetSize`, `SetAttributes`, `MakeLink`, `SetLimit` (the host test covers them); record locks (ADR-0319) have no C-level test with two owners | `notes/coverage-holes.md` C8 to C11 | Tests only |
| C9 | 130 `checked` invariants have an id that appears nowhere outside `docs/`, and 349 of the 551 `checked` statuses name a whole command (`make test-unit` 112, `make test-qemu` 73, `make test` 27), not a test | Script over `invariants.md` | With V40: a status should name something that fails |

Well covered, for proportion: kernel rights and transfer, `grants.script`, `system-ro.script`, the TLS refusals, the
network scope in `t_netd`, the keeper's channels and quotas.

## What part 2 did

The commits of `work/test-audit` after part 1, each with its findings' numbers and what was shown in its message:
D1 to D3 with ADR-0350; T3, T2, V41; V1; F5 and V22; V6, V7, V8, V14, V20, V21; V34; V29; the review's fixes (R1 to
R13, `review-notes.md`); the two corrections the first candidate called for (`keeper.script`, `scenarios.script`).
Sections 2 to 5 above are part 1's text and keep its estimates; the measured figures are in the status block. Two
things part 1 planned were not done: V4 (`test-crash` with no durable file: a floor needs the time to a round's
first sync, or two local rounds can fail it) and the `t_quota` handshake.

Candidate runs: 43575482537 (`-a`, red: `test-keeper` and `scenarios.script`, both explained in the status block)
and 43579834963 (`-b`, green).

## Limitations of this audit

- Nothing was run on this machine except the scripts over the fetched history; every vacuous-test finding is from
  reading.
- Per-leg times are from one run (43553496171); job times are medians over 19 to 135 jobs since 2026-11-12.
- The flake table is two weeks. F2's cause is not established. F8 is a mixed class.
- Not read: `tests/posix/tour-gui.script` line by line, `tests/apps/cryptothreads.c`, `tests/posix-net/zfetch.c`,
  `tools/test-dns`'s pcap check, most fuzz target bodies, the `t_*` programs other than `t_cpu` beyond their time
  bounds and early returns, and the non-security rows of `invariants.md`.
