# TASK-31: independent review of part reaper-race

Reviewed: commit 728458c3 (the test `a_run_queued_behind_many_frames_is_still_found_by_a_create`), the fix as a patch
on `kernel/src/memory/frames.rs` (base blob 4ebb2226), the proposed ADR-0349 and the uncommitted document changes
(`docs/adr/index.md`, `docs/adr/invariants.md`, `docs/architect-queue.md`, `docs/backlog.md`, `docs/design.md`).
Context read: `kernel/src/reaper.rs`, `kernel/src/steps.rs`, `kernel/src/sched.rs` (`preempt_point`),
`crates/zoraya-pmm/src/lib.rs` (`alloc_counted`, `alloc_contig_step`, `free_contig`), the other run tests in
`kernel/src/testing.rs`, ADR-0270, the removed backlog entry. Line numbers of `frames.rs` are those of the patched
file. No build and no QEMU run was made for this review; the one run cited below is a log another session's script
wrote in the worktree.

**Verdict: not ready to merge.** Errors 1 to 3 must be fixed first.

## Findings

### 1. error: the test does not reproduce the old failure; it passes on the unpatched kernel

- **Where:** `kernel/src/testing.rs:3557-3586` (`a_run_queued_behind_many_frames_is_still_found_by_a_create`), its
  loop over `unhoard_one`.
- **What:** the 3000 "scattered" frames are not scattered. `hoard_frames` takes the free frames with `frames::alloc`,
  which is next-fit (`zoraya-pmm` `alloc_counted`: the search goes on after the last frame returned), so the list is in
  address order, each node the frame just below the one before (the comment of
  `the_worst_single_allocation_is_bounded_by_the_machines_ram` relies on exactly this). The 3000 frames taken from the
  head are, apart from the holes of frames in use, one run. `reaper::free_some` pops them from the end of the vector,
  so after 16 calls of `help` 1024 adjacent frames are free and the scan finds a run among them. The old code does 16
  helps in rounds 0 to 15 and scans again in round 16 (< 17): it succeeds. The queued run of 1024 is never reached, by
  the old code or the new.
- **Evidence:** `target/test-logs/auto-default.log` in the worktree, kernel banner `git 728458c3dac6-dirty`, built
  `build/zoraya-kernel-test.elf` at 13:32:32 while `kernel/src/memory/frames.rs` was blob 4ebb2226 (the patch's base,
  `CONTIG_HELP_ROUNDS` present; the file last changed at 13:30:28, the dirt is documents only), lines 61-62:
  `ZORAYA-TEST run a_run_queued_behind_many_frames_is_still_found_by_a_create` /
  `ZORAYA-TEST ok a_run_queued_behind_many_frames_is_still_found_by_a_create`, at 128 MiB. The commit message's
  "fails before the fix" is not true.
- **Consequence:** the fix has no test; INV-0349-1's check column (finding 3) checks nothing; a mutant that puts the
  17 rounds back, or sets `HELP_STEPS_PER_ROUND` to 1, survives.
- **Fix:** queue frames no two of which are adjacent, as `a_run_search_over_a_fragmented_map_goes_in_steps` does:
  walk the hoard, put the frames with an even frame number in `scattered` until it has 3000 (more than 1088 + one
  step), keep the odd ones in a second list (link them through the direct map as that test does) and give them back
  with `unhoard` after the call. Assert `crate::reaper::held()` grew by `SCATTERED + RUN` after the two releases, so a
  `defer` that fell back to freeing on the spot is not taken for a pass. Run it once on the unpatched kernel and quote
  the failing line (`the run the reaper held (ADR-0349): NoMemory`) in the commit, then on the patched one.

### 2. error: a create that races the reaper's own turn is still refused (the race the part is for)

- **Where:** `kernel/src/memory/frames.rs:179-191` (the help arm), `frames.rs:189-190` (`if !helped { return
  Err(Status::NoMemory) }`).
- **What:** `steps::between` is `pause`, whose step point calls `sched::preempt_point` and may switch threads
  (`sched.rs:658-670`). The reaper is ready as soon as `defer` woke it, and one of its turns frees up to `REAPER_STEP`
  = 1024 frames. The `changed` test covers only frees made while a scan ran (`generation != began`, compared at the
  scan's last step); frees made after the scan ended are not looked at before the refusal.
- **Scenario:** a job closes a 4 MiB contiguous object (queued, `held` = 1024 plus less than a turn of other
  frames) and asks for a new one. The scan finds no run. The step point at `frames.rs:180` switches: the timer slice
  is over and the reaper is ready. The reaper's turn frees everything queued, the run included, and yields. The create
  resumes; the first `reaper::help()` returns false, `helped` is false, and the call returns `NoMemory` with the run
  free. The same holds at the start of any later round (the step point at `frames.rs:187` after the last help of a
  round, then the rescan, then a preempting step point at `frames.rs:180`). This is "a race with the reaper's own
  turns", which ADR-0349's context names as the flake; the old code had the same hole and the patch keeps it, so
  ADR-0349's consequence "fails with `NoMemory` only when the frames are not free and the reaper holds none" is not
  what the code does.
- **Fix:** before refusing on `!helped`, compare the frame map's generation with the one the scan last read (keep it
  from the loop: the `generation` of the last step); if frames were freed since, scan again as a round (still bounded
  by `rounds`). Test it with a step point that, on the call just before the first help (count the step points of a
  failing scan first, as `a_run_freed_behind_the_scan_is_found_by_a_second_pass` does), runs `while
  crate::reaper::help() {}`, which is what the reaper's turn does; the create must return the run.

### 3. error: INV-0349-1 is recorded as `checked` by a test that does not check it

- **Where:** `docs/adr/invariants.md` (new row INV-0349-1, `checked: make test-qemu-one`);
  `docs/adr/0349-a-contiguous-create-helps-the-reaper-until-it-holds-nothing.md`, Invariants.
- **What:** by finding 1 the named test passes with the old 17 rounds, so the Status column is not true (CLAUDE.md:
  keep it true). The rule itself also overstates the code: "however many frames the queue holds ahead of it" holds
  only without churn (the cap `total / 1024 + 2` rounds) and not across a preemption (finding 2).
- **Fix:** after findings 1 and 2, state the rule with its bound (a queue that is not refilled during the call), name
  both tests, and only then mark it checked.

### 4. warning: the work of a failing create is quadratic in the machine's memory, and the ADR says it is not more than the reaper's

- **Where:** `frames.rs:105-107` (`contig_help_rounds`) with the full rescan after each round (`frames.rs:151-170`);
  ADR-0349 Option B, "Evidence for" and Consequences.
- **What:** every round rescans the whole frame map once free frames reach `count` (`alloc_contig_step` returns at
  once only while `count > free`). On a fragmented machine (as many free frames as a run, no run) with a large queue
  of non-adjacent frames, every round reads every word: at 2 GiB (ADR-0436), 514 rounds of 8192 words, about 4.2
  million words, some 88,000 step points, charged to the caller, before `NoMemory`. Each interval stays within a step
  (INV-0270-1) and each step point ends the call on a kill (ADR-0270 point 3), so it is bounded and killable; but
  "the total work is at most the work the reaper would do for the same frames" is false (the scans are extra), and
  driver 3's "a refusal at real exhaustion stays fast" holds only when free frames are fewer than `count`.
- **Fix:** either state the cost in the ADR (rounds times map words, with the 2 GiB figure) and drop the claim, or make
  it linear: rounds that grow (1024, 2048, 4096 frames of help before the next scan), or a rescan only when the round
  made `free >= count` or freed a queued run (`Garbage::run`).

### 5. warning: no mutant for the new bounds in a live mutant list

- **Where:** `tools/mutants/kernel-objects.toml:397-401` (`K-contig-no-rescan`, `find` names `CONTIG_HELP_ROUNDS`).
- **What:** the patch removes the text `K-contig-no-rescan` finds. `make check-mutant-lists` will not see it, because
  `kernel-objects` is in `MUTANT_STALE` (`Makefile:1875`), so the rescan mutant silently stops applying. Nothing
  lists the new decisions (`HELP_STEPS_PER_ROUND`, `contig_help_rounds`, `if !helped`).
- **Fix:** update the `find`/`replace` of `K-contig-no-rescan` to the new arm (`round < rounds`), and add mutants
  (`contig_help_rounds` returning 17; `HELP_STEPS_PER_ROUND = 1`) to a list that is checked, with the fixed test of
  finding 1 as the one that kills them.

### 6. warning: ADR-0349 does not say which text of accepted ADR-0270 it replaces, and Option C is not the backlog's second way

- **Where:** ADR-0349 header and Considered options; `docs/adr/0270-bounded-kernel-work.md:182` ("the search `help()`
  loop makes at most 17 rounds") and `:204` ("up to 17 rounds for a run").
- **What:** the code now deviates from those sentences of an accepted ADR; ADR-0349 lists ADR-0270 only under
  "Depends on" and does not quote what it changes, so the architect accepting it cannot see that ADR-0270's notes
  become false. The backlog named two ways, "keep helping" and "free the queue in whole objects"; Option C is a third
  one (waiting for the teardown at the report) and the backlog's second is not analysed, so the comparison argues
  against an option nobody proposed. Option A's only evidence is `[outside evidence: Linux's CMA, not fetched]`.
- **Fix:** add the two ADR-0270 sentences to the Context as what this ADR would replace; add (or replace C with) the
  option "the create frees the queued items whole, a run item first", with its cost (a 1024-frame run item in steps
  is what Option B does anyway).

### 7. note: stale wording

- `kernel/src/testing.rs:3526-3528`: `a_run_short_of_frames_waits_for_the_reaper_in_steps` says "no interval holds
  more than one round (64 frames)"; a round is now up to 1024 frames, a step is 64.
- The new test's comment says the queued run "is the only one that makes a run of 1024", which is false as written
  (finding 1).

## Checked and found sound

- Loop logic: `round` counts every scan; the `changed && !rescanned` arm is taken at most once and only below
  `rounds`; `round >= rounds` ends the search after that many scans, so the loop ends (at most `rounds + 1` scans and
  `16 * rounds` helps). With no churn `rounds - 1` rounds of 1024 frames free at least `total` frames, more than the
  queue can hold, so the cap does not cut a create that the queue could satisfy. `unreachable!()` stays unreachable.
- A round that ends early because the queue emptied (`helped` true) rescans before the next round's `help` returns
  false, so frames freed by a partial round are seen.
- INV-0270-1: every `help` (64 units under the queue's lock and the frame map's) is followed by a step point; the
  first is preceded by one; the scan steps are as before. No spinlock is held at a step point: `help` takes and drops
  its locks, `stats()` is read once before the loop, `FRAMES` is released at the end of each scan step. The new test's
  units assertion (`<= STEP + 1`) is the right bound for the 1024-frame candidate (at most 47 + 17 words + 1).
- Killability: every new step point is `step_point()?`, so a `Cancelled` from `steps::between` ends the search with
  nothing taken (ADR-0270 point 3).
- `contig_help_rounds` before the allocator is up: `stats()` gives zeros, 2 rounds, and the scan fails first on
  `FRAMES` being `None`.
- The test's accounting: the vector is reserved before the hoard; the queue has `QUEUE_ROOM` room, so `defer` does not
  allocate with IRQ masked; `release` and `release_run` with IRQ masked are what the existing tests do; the hoard's
  rest is unhoarded and the run freed; `frames_accounted` drains the reaper, which drops the vector.
- Other users of the old constant: only `tools/mutants/kernel-objects.toml` (finding 5) and historical reports
  (`docs/tasks/TASK-26-*`, `TASK-34-report.md`), which are not to be edited. The other run tests
  (`a_run_search_over_a_fragmented_map_goes_in_steps`, `a_run_freed_behind_the_scan_is_found_by_a_second_pass`)
  queue nothing, so the first `help` returns false and their outcome is unchanged.
- ADR-0349's number is in TASK-31's reserved range (0343 to 0349) and not used elsewhere; status `proposed`, the
  Architect's decision section empty; the template's sections are present; the index, invariants and queue rows are
  consistent with it.

## Resolution (the implementation session)

The review read the first version of the test. Findings 1 and 3: the test now queues every other frame (kept ones stay taken) and fails 3 of 3 on the unfixed code. Finding 2: fixed (`frames::generation` compared with the scan's last one before refusing), with the test `a_run_the_reaper_frees_at_the_step_before_its_help_is_found` (fails 3 of 3 before). Warning 4: stated in ADR-0349's follow-up. Warning 5: `K-contig-no-rescan` updated, two mutants added to `tools/mutants/kernel-objects.toml` (a stale list, `MUTANT_STALE`). Warning 6: ADR-0349 names the ADR-0270 sentences it replaces and the backlog's second form in Option C.
