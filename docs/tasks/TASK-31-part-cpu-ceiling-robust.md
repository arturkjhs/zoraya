# TASK-31 part cpu-ceiling-robust: state (paused, unfinished)

Branch `work/cpu-ceiling-robust`. Paused by the architect on 2026-11-19; the work below is a WIP.

## Found

`cpu_ceiling_from_a_process` (`user/src/bin/t_cpu.rs`, driven by `kernel/src/testing.rs`) fails when the host is
shared. The failing checks are wall-clock ones, with the units of work in bound (60 of 80):

- `check_masked`: longest masked interval against 2 ms (seen 2.0, 3.0, 4.3, 11.5, 13.4 ms);
- the wake-up lateness bound (56.7 ms against 50 ms);
- (not seen failing, same kind) `upper()` charge checks and the parked room's death time.

A passing run could be luck: when the start-up calibration saw a slow host (factor > 1) the masked limit was 1 s.

## Reproduction (before the fix)

Scratch guest running only this test (local, uncommitted patch of `shard_range`), one other QEMU on the machine:
1 of 1 failed. Under 8 `yes` burners: 2 of 4 failed. Under 16 `yes` burners: 3 of 3 failed
(lateness line 717 twice, masked interval once).

## Fix (in this branch)

`t_cpu` keeps the counted units, twice the budget, a second of masked time or lateness, starvation and the counts
as assertions. Each wall-clock bound that is passed goes through `time_over`: the host is measured again
(256 KiB fill); if it is slower than the reference the overrun is printed and not counted; on a quiet host it is
counted and `t_cpu` exits 3. `cpu_ceiling_from_a_process` runs it up to three times while it exits 3 (as `t_bound`'s
`outer`).

## Verified so far

Under 16 burners after the fix: 2 of 2 passed (one after two repeated runs). Not run: the remaining 3 of the 5
planned runs (stopped), a run on a quiet host, `make check`, `make test-changed`.

## Missing

- Quiet-host run, and N of N under load with more runs.
- Mutation check: a mutant that breaks the ceiling logic (for instance no park at the ceiling, or a charge that
  stops counting) must still be killed; name it and add it to the kernel's mutant list in `tools/mutants/`
  (`rooms.toml` looks like the one).
- Independent `opus-review` into `docs/tasks/TASK-31-review-cpu-ceiling-robust.md`, one changed-lines check.
- `make check`, `make test-changed` to a file under `build/`, backlog / `docs/architect-queue.md` entries.
- Known weakness: under sustained load the "host not quiet now" test excuses a real overrun of a time bound; the
  units and the 2x/1 s assertions still hold.
