# TASK-31: second independent review of part reaper-race

Reviewed: `git diff ab45a207...a7e05f7c` (the merge base with `origin/main` to the branch head), with the code around
it: `kernel/src/memory/frames.rs`, `kernel/src/reaper.rs`, `kernel/src/steps.rs`, `crates/zoraya-pmm/src/lib.rs`
(`alloc_contig_step`, `free_contig`, `generation`), the run tests in `kernel/src/testing.rs`, ADR-0270, ADR-0022,
ADR-0349 and the first review (`TASK-31-review-reaper-race.md`). Line numbers are those of the head. `make check` was
run on the head and exited 0. No QEMU run was made (another session's run shared the machine); the claims about test
outcomes below are from reading the code, not from runs.

**Verdict: not clean.** Finding 1 must be fixed.

## Findings

### 1. error: a run the reaper frees during a scan is still refused once the one rescan is spent

- **Where:** `kernel/src/memory/frames.rs:174-177` (the `changed && !rescanned` arm) and `:192` (`if !helped &&
  generation() == scanned_at`).
- **What:** `scanned_at` is the generation read at the scan's last step, so it already includes every free made
  during the scan, also frees behind the scan's cursor, which the scan did not see. Those are caught only by the
  `changed && !rescanned` arm, and that arm is taken once per call. After it is spent, a scan that ends with
  `changed` true goes to the help arm; `help` finds the queue empty; `generation() == scanned_at` holds; the call
  returns `NoMemory` with the run free.
- **Scenario:** a 1024-frame create on a machine with at least 1024 free but fragmented frames (so the scan has step
  points). Round 0: some other frame is freed during the scan (any exit or close on another CPU, or a thread that ran
  at a step point): `changed`, `rescanned = true`. Rounds 1 to k: help frees steps of the queue. Round k+1: at one of
  the scan's step points (`steps::between` -> `preempt_point`, or simply the reaper on another CPU) the reaper's turn
  frees the rest of the queue, the dead object's run among it, at an address the cursor has passed. The scan ends
  with `OutOfFrames`, `changed` true, `rescanned` true; the help arm's `help()` is false, the generation has not moved
  since the scan's last step, and the create is refused. This is the race with the reaper's own turns that ADR-0349's
  context names as the flake; ADR-0349's consequence ("fails with `NoMemory` only when the frames are not free and the
  reaper holds none") and INV-0349-1 are not true of it.
- **Fix:** refuse only when no frame was freed since the scan *began*: keep `began` out of the loop and test
  `!helped && generation() == began` (or drop `!rescanned`, since `rounds` now bounds the rescans). Test: as
  `a_run_the_reaper_frees_at_the_step_before_its_help_is_found`, but with a fragmented map (so the scan has step
  points), a step point that frees one hoarded frame during the first scan (spends the rescan) and runs `while
  crate::reaper::help() {}` at the last step point of a later scan.

### 2. warning: the round cap, the only bound under churn, has no test and no mutant

- **Where:** `frames.rs:105-107` (`contig_help_rounds`), `:178` (`round >= rounds`); `tools/mutants/kernel-objects.toml`.
- **What:** both new tests end long before the cap. A mutant that makes `round >= rounds` false (no cap) or
  `contig_help_rounds` return `u64::MAX` survives every test, so ADR-0349 driver 2 (ADR-0022: no wait without a bound)
  is asserted only by reading. With finding 1's fix the cap matters more: a create then keeps going for as long as
  anything frees a frame during its scans.
- **Fix:** a kernel test whose step point queues a fresh scattered batch with the reaper at every call (churn): the
  create must return `NoMemory` after at most `contig_help_rounds()` scans (count them in the step point). List the
  no-cap mutant with it.

### 3. warning: ADR-0349's cost figures are wrong by four times

- **Where:** ADR-0349, Follow-up: "2 GiB, 2,050 rounds of 8,192 words, about 17 million words".
- **What:** at 2 GiB `total / 1024 + 2` is 514 rounds (524,288 frames), so about 4.2 million words, as the first
  review computed; 2,050 rounds is 8 GiB. At 128 MiB it is 34 rounds, not 33. The architect will weigh Option B on
  these numbers.
- **Fix:** 514 rounds, about 4.2 million words, at 2 GiB; 34 at 128 MiB.

### 4. note: the end condition is stated loosely

- `docs/design.md` ("refused only when a round frees nothing or after as many rounds...") and ADR-0349 Option B ("ends
  with `NoMemory` when a round frees nothing") leave out the generation test; the part report states it right.
- The first review's note 7 is not addressed: `kernel/src/testing.rs:3531-3532` still says "no interval holds more
  than one round (64 frames)"; a round is now up to 1024 frames, a step 64.

### 5. note: the reproductions hold only on one CPU, and do not check that the frames were queued

- On the `--smp 4` lanes the reaper runs on another CPU while the test thread is masked, so it may free the queue
  before or during the scan and both tests pass whatever the code; they reproduce the old failure only on the
  one-CPU lanes (`auto-default`, `tcg-default`), which `make test-qemu-one` uses. Neither test asserts that
  `reaper::held()` grew by the frames released (the first review's suggestion), so a `defer` that fell back to
  freeing on the spot would make test 1 pass vacuously. Add the `held()` assertion.
- `K-contig-help-one-step` is killed at 128 MiB (34 rounds of 64 frames, 2,176 < 4,024 queued) but would survive at
  2 GiB (514 x 64 > 4,024); mutants run at the default size, so this holds today.

## Checked and found sound

- First review: finding 1 fixed (the queued frames are every other node of the hoard, which is in descending address
  order, so two queued frames are at least two frames apart and only the run makes 1024); finding 2 fixed for the step
  point before the help (the test's dry run has one step point, the empty-queue scan fails at once since `free <
  count`, so `calls == points` is that step point); finding 3 consistent with the tests named; warnings 5 and 6 done;
  warning 4 stated (figures wrong, finding 3).
- Boundedness: at most `rounds + 1` scans and `16 * rounds` helps; each `help` (at most 64 frames, counted by
  `frames::free`/`free_contig`) is followed by a step point, the first preceded by one; an empty `help` walks nothing
  (the cursor is past emptied items) and shares an interval with one scan step only; the scan's words stay within
  `STEP + 1` with the final `count(1)`. Every new step point is `step_point()?`, so a kill ends the call with nothing
  taken.
- Races on the queue: `help`, the reaper and `take` all work under `QUEUE`'s lock; a frame is popped before it is
  freed, so no double free and no lost frame; `held` is decremented by what was freed; the charge stays on the item
  until the reaper drops it (ADR-0261), so the accounting is as before. `generation` counts every `free_contig` call
  under the frame map's lock, and wraps harmlessly.
- No spinlock across a step point: `generation()` and `stats()` take and drop the lock; `help` drops both locks.
- `contig_help_rounds` before the allocator: 2 rounds, and the scan fails first on `FRAMES` being `None`.
- INV-0349-1's row, index row and total (892) agree with ADR-0349; `make check` (adr-check included) exits 0.
- No `unsafe` added by the diff in `frames.rs`; the tests' `unsafe` is in the existing helpers, with SAFETY comments.
