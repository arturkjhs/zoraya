# TASK-31, part reaper-race: the contiguous create refused while the reaper holds the frames

Branch `work/reaper-race`. The architect's decision of 2026-11-19 (backlog entry of 2026-11-16, removed).

## What was done
- Two kernel tests that reproduce the failure, committed before the fix (`kernel/src/testing.rs`):
  `a_run_queued_behind_many_frames_is_still_found_by_a_create` (3000 non-adjacent frames queued ahead of a run of 1024:
  the old 17 rounds of 64 free 1088) and `a_run_the_reaper_frees_at_the_step_before_its_help_is_found` (the reaper's
  turn at the step point empties the queue, `help` finds nothing).
- Fix (`kernel/src/memory/frames.rs`): a round of help frees up to 1024 frames (16 steps, a step point after each), then
  the frame map is scanned again; the search ends with `NoMemory` when a round frees nothing and no frame was freed
  since the scan, or after `total / 1024 + 2` rounds.
- ADR-0349 (proposed): Option B chosen over "the reaper frees before the death is reported" (the exit would wait for the teardown).
  INV-0349-1 checked by the two tests. Mutants added to `tools/mutants/kernel-objects.toml`.

## Evidence (ran in this session, `make test-qemu-one`, HVF)
- Unfixed code, new tests: both tests fail 3 of 3 (`NoMemory`).
- Fixed code: both pass 2 of 3 whole runs; the third run's only failure was `cpu_ceiling_from_a_process` (exit code 101),
  which also failed once on the unfixed tree in an earlier run of this session: a known flake of that test, not looked into.
- The fragmented-map test passes with the fix (a step point leaves interrupts masked; a test that calls one must mask first).

## Known limitations
- A create that fails on a fragmented machine scans the map once per round: about `total/1024` scans (ADR-0349 follow-up).
- Backlog N6 (`a_spawn_with_a_big_space_is_bounded`, intermittent `NoMemory` in the 128 MiB guest) may share the cause (the
  reaper's timing); not checked.

## Second review fix pass
- Refusal only when no frame was freed since the scan began (`generation() == began_at`), not since its last step; tests
  `a_run_the_reaper_frees_during_a_later_scan_is_found` and `a_create_under_churn_is_refused_within_the_round_cap`;
  mutant `K-contig-no-round-cap`.
- The repro tests reproduce the old failure only on one CPU (`make test-qemu-one`, lanes `auto-default`, `tcg-default`); on
  `--smp 4` the reaper may run on another CPU and free the queue first. Each asserts that `reaper::held()` grew.
