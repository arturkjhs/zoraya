# ADR-0349: A contiguous create helps the reaper in bounded rounds until a run appears or the reaper holds nothing

- **Status:** accepted
- **Date:** 2026-11-19
- **Depends on:** ADR-0022, ADR-0270
- **Evidence:** alternatives-postmortem.md#keykos--eros--capros-and-coyotos

## Context
`memory_create(CONTIGUOUS)` takes its run with `frames::alloc_contig_in_steps`. When the frame map has no run, the search
helps the reaper (ADR-0270 point 4): `reaper::help` frees one step (64) of the frames the reaper holds, and the scan
starts again. The number of rounds was `CONTIG_HELP_ROUNDS = 17`, enough for one run of the most a run may have
(1024 frames, 16 steps) but not for a queue that holds more than 1088 frames ahead of the frames of the run. A job that
frees its memory (an exit, a `handle_close` of a large object) puts them in the reaper's queue; about 15,000 frames sat
there when `t_bound` printed `contiguous create: NoMemory` (`docs/backlog.md`, found in F7b's merge test). The call
failed although the memory was on its way back, which is what ADR-0270 point 4 and `alloc` (which takes a frame from
the reaper) say must not be exhaustion. The failure is a race with the reaper's own turns, so it showed as a flake.

The backlog gave two ways: keep helping while the reaper holds frames, with a step point between rounds, or free the
queue in whole objects. The architect decided on 2026-11-19 to make it a part, with a test that reproduces the failure
first.

## Decision drivers
1. A create is not refused while the memory that would satisfy it is only queued (ADR-0270 point 4).
2. No unbounded kernel work: every interval holds one step (INV-0270-1), every step point can end the call for a kill
   (ADR-0270 point 3), and the number of rounds is bounded (ADR-0022: no wait without a bound).
3. The cost on a create that finds a run at once is nil, and a refusal at real exhaustion stays fast.
4. The change is small and the reaper's order and charges stay as ADR-0270 and ADR-0261 have them.

## Considered options

### Option A: Linux's baseline: fail after a fixed number of retries (today)
- **Description:** `alloc_contig_range` and CMA retry a few times and then fail with `-ENOMEM`/`-EBUSY` while pages are
  still being migrated or freed `[outside evidence: Linux's CMA, not fetched]`.
- **Evidence for:** simplest, and what the code does.
- **Evidence against:** driver 1: the refusal depends on what the reaper has not yet freed, so it is a flake.
- **Cost:** *(estimate)* none. **Precedent:** Linux, where callers retry.

### Option B: Help in rounds until a run appears, each round of at most one run's worth of steps, bounded by the machine
- **Description:** when the scan finds no run and a rescan after a change did not help, a round frees up to 1024
  frames of the reaper's queue (16 calls of `reaper::help`, a step point after each) and scans again. The search ends
  with `NoMemory` when a round frees nothing and no frame was freed since the scan began (the reaper holds nothing, and its turn at a step point freed nothing behind the scan), or after `total frames / 1024 + 2` rounds,
  enough to free every frame of the machine once, so that churn refilling the queue cannot keep the call for ever.
- **Evidence for:** drivers 1 to 4: the intervals are the steps of today, the call is killable at each step point, the
  total work is at most the work the reaper would do for the same frames, and the scan is not repeated for every step.
- **Evidence against:** a create may free many frames that belong to dead jobs on the caller's CPU time, up to the
  whole queue; the caller's job pays that CPU (it is a step of work, counted), and the queue is the work the
  reaper has not done, so the total is not more than a drain would be.
- **Cost:** *(estimate)* 20 lines in `kernel/src/memory/frames.rs`, one test.
- **Precedent:** the shape of ADR-0270 point 4's `help`, extended.

### Option C: The reaper frees a dead job's frames before the job's death is reported (and, the backlog's other form, the queue in whole objects)
- **Description:** the backlog's second way is to free the queue in whole objects, so that a create finds the run's object freed whole; the form the architect's task names is that `job_kill`, exit and `handle_close` wait until the reaper has freed what they queued, so the memory is
  free when the caller learns of it (a drain). EROS and KeyKOS did batched teardown at the point of release
  (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`).
- **Evidence for:** the failure cannot occur for the caller that waited; accounting is exact at the report.
- **Evidence against:** drivers 2 and 4: it makes the exit or the `job_kill` take as long as the teardown, which is what
  ADR-0270 moved to the reaper to avoid; it needs a wait with a wake-up on the reaper for every release and a bound for
  it; it does not help a create that races a release made by another job.
- **Cost:** *(estimate)* 100 lines and a changed contract of three calls.
- **Precedent:** the batched teardown that ADR-0270 names as KeyKOS's wart.

## Recommendation
**Recommendation (not a decision): Option B.** It changes one function, keeps every interval to a step, and fixes the
race for every caller of a contiguous run (the drivers' buffers too), not only for a job that waited for its own
teardown.

## Consequences
- **Positive.** `memory_create(CONTIGUOUS)` fails with `NoMemory` only when the frames are not free and the reaper holds
  none (or is refilled faster than rounds empty it for the whole of the machine's memory). `t_bound`'s flake goes.
- **Negative.** A create on a short machine may spend the time of freeing the queue; it is steps and killable; see the follow-up for its cost on a fragmented machine.
- **Follow-up.** This replaces ADR-0270's "the search `help()` loop makes at most 17 rounds (enough to free a run of the cap)" (Decision text, test paragraph) and "up to 17 rounds for a run" (Consequences): a round is now up to 1024 frames and the rounds are bounded by the machine's memory. ADR-0270's text is not edited (only the architect accepts a change). A create that finds no run rescans the whole frame map after each round, so a create that fails on a fragmented machine does work proportional to memory times rounds (about `total/1024` scans of `total/64` words, quadratic in memory: 128 MiB, 34 rounds of 512 words; 2 GiB, 514 rounds of 8,192 words, about 4.2 million words, all in steps with a step point that a kill ends); this is more than the reaper's own work and is the price of refusing only when nothing is left to wait for. If a measurement shows it, scan again only when the round freed a run's worth of frames. The rescan and the cap of 4 MiB stay as ADR-0270's open questions have them.

## Invariants
- **INV-0349-1.** A contiguous run that is free except for frames the reaper holds is found by `alloc_contig`, however
  many frames the queue holds ahead of it (up to the machine's memory) and when the reaper frees the queue at the step point before the first help or at a later scan's last step point, and no interval of the search holds more than a step of work. *Check:* the
  kernel tests `a_run_queued_behind_many_frames_is_still_found_by_a_create`, `a_run_the_reaper_frees_at_the_step_before_its_help_is_found` and `a_run_the_reaper_frees_during_a_later_scan_is_found` (`make test-qemu-one`).

## Open questions
- Whether the reaper's order (frames of earlier items first) should prefer the items whose frames are adjacent when a
  run is wanted. Not needed by the measurement; revisit if a create still waits for many rounds.

## Revisit triggers
- A create that returns `NoMemory` while `reaper::held()` is non-zero.
- A measured interval above INV-0270-1's bound that names the rounds of help.

## Architect's decision

Accepted 2026-11-19. Decided by the architect; recorded by an agent at the architect's request.

Accepted on the condition that the independent review is clean: the third review (`docs/tasks/TASK-31-review-reaper-race-3.md`) found no errors.
