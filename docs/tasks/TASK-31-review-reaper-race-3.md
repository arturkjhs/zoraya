# TASK-31: third review of part reaper-race (changed lines of the fix pass)

Reviewed: `git diff 38213249..HEAD` (the fix pass after `TASK-31-review-reaper-race-2.md`), with the code around it:
`alloc_contig_in_steps` and `contig_help_rounds` in `kernel/src/memory/frames.rs`, `alloc_contig_step`, `first_used`
and `generation` in `crates/zoraya-pmm/src/lib.rs`, `reaper::help`, `release`, `release_run` and `held`, the helpers
`hoard_frames`, `unhoard_one`, `unhoard`, `test_step_point` and `frames_accounted` in `kernel/src/testing.rs`, ADR-0349
and its row in `docs/adr/invariants.md`. Line numbers are those of the head. Host reading only: no build, no QEMU run
and no `make` target was run for this review (another session's `make test-changed` was using the worktree). What is
said about test outcomes below comes from reading the code, apart from the fixer's runs quoted in the task.

**Verdict: clean.** No error. Finding 1 should be fixed before the merge, since without it the new test can pass on the
broken code without anyone noticing. Findings 2 and 3 are notes.

## Checked and sound

- **Fix of error 1** (`frames.rs:154-166`, `:194`). `began` is reset for each round (`:153`) and set at the first step
  of that round's scan, from the generation read under the same lock as that step (`:155-161`). So `began_at` is never
  the value of an earlier scan. A free before the first step is seen by the scan. A free after it, ahead of or behind
  the cursor, at a step point of the scan or at the step point before the help (`:180`), makes
  `generation() != began_at` and so another round. The refusal is now exactly "this round freed nothing, and nothing was
  freed since this scan began".
- **Termination and bound.** Every path that does not return goes to the next value of `round`: the one rescan
  (`:174`), the round with help, and the round with no help but a changed generation. At `round >= rounds` a failed
  scan returns `NoMemory` (`:178`) before any help. So there are at most `rounds + 1` scans, one rescan point, and
  `rounds - 1` help phases of at most 1 + 16 step points each. The bound has the same shape as before the fix. Only
  the reason a round can be added without help is wider now.
- **Cost, already recorded.** Any free anywhere during a scan (on another CPU, say) now keeps a failing create going
  for one more round. A create that fails on a busy, fragmented machine therefore tends to run to the cap. The ADR-0349
  follow-up already prices this (memory times rounds), so it is not a new finding.
- **Figures.** 2 GiB is 524,288 frames: /1024 + 2 = 514 rounds. Each scan reads 524,288/64 = 8,192 words, and
  514 x 8,192 is about 4.21 million words. 128 MiB is 32,768 frames: 34 rounds of 512 words. The ADR-0349 follow-up,
  `design.md:1786` and the part report agree with the code (`contig_help_rounds`, `frames.rs:105`).
- **`a_run_the_reaper_frees_during_a_later_scan_is_found`, call counts** (`testing.rs:3676`). The dry run has the
  same map as the measured call: the run is allocated and the reaper is idle after `frames_accounted`. The dry run
  makes `S - 1` step points in its scan and 1 before its help, and it refuses (`helped` false, generation unchanged).
  So `scan = points - 1` is the number of step points inside one scan. In the measured call:
  - calls 1..`scan` are scan 0. Call 1 comes after the first step and frees an odd frame, so `changed` is true.
  - call `scan + 1` is the rescan's step point.
  - calls `scan + 2`..`2*scan + 1` are scan 1, so `2*scan + 1` is its last step point, provided scan 1 has as many
    steps as the dry run (see finding 1).

  The drain there frees the run behind the cursor. The old code then compared the generation with its value after the
  drain and refused. The new code compares it with the value at scan 1's first step, before the drain, and goes on to
  round 2, which finds the run.
- **`a_create_under_churn_is_refused_within_the_round_cap`** (`testing.rs:3722`).
  - **Frames only move.** `frames::alloc` takes only free frames, and those are the even ones. The odd frames stay on
    `kept` until the end. Heap growth for the batch `Vec` also takes even frames. So no two adjacent frames are ever
    free, and no run can form.
  - **Step point.** Heap use inside the step point is fine: the frame map's lock is released there (`frames.rs:155-160`)
    and the caller holds no spinlock.
  - **Accounting.** `settled_stats` waits for the reaper, so queued frames are counted back, and the heap's growth is
    counted in `frames_accounted`.
  - **The 2x bound is a true upper bound.** A busy even frame costs at most one more word per pair of frames (two
    candidates instead of one), so a scan under churn reads at most twice the words of the dry run. That gives
    `(rounds + 1)(2*scan + 1 + 18) <= 2(rounds + 1)(scan + 18)`.
  - **Cancel guard.** It returns `Cancelled` from the step point at twice the bound. The create returns it through `?`
    without having taken a run, and the `assert_eq` on `NoMemory` then fails. So the round-cap mutant fails the test
    and does not hang.
- **Mutant entries** (`tools/mutants/kernel-objects.toml`). `if !helped && generation() == began_at {` and
  `Err(_) if round >= rounds => return Err(Status::NoMemory),` each occur exactly once in `frames.rs`. Under churn
  `K-contig-no-round-cap` is killed as described above.
- The `held()` assertions added to the two older tests check what they say.

## Findings

### 1. warning: the later-scan test does not check that its drain came at a scan step point, so on some memory sizes it passes on the broken code

Where: `kernel/src/testing.rs:3696-3707` (the frame freed at call 1, and `if calls == 2 * scan + 1`).

What is wrong: the test assumes scan 1 has exactly as many steps as the dry run's scan. It does not always. The kept
frame freed at call 1 is an odd frame `o` between two free even frames. Before the free, the candidates `o-1` and
`o+1` each cost a word (busy at `o`, then busy at `o+2`). After it, `o-1` reads free up to `o+2` in one word and the
next candidate is `o+3`. So every scan that sees the frame reads one word fewer than the dry run. (Scan 1 always sees
it, and scan 0 sees it if it is ahead of the cursor.)

When the dry run's word total sits just past a multiple of `SCAN_WORDS` (48), that one word removes a step. Whether it
does depends only on the map, so on a given memory size it happens every time or never. When it happens, call
`2*scan + 1` is no longer scan 1's last step point. It is the step point after the first `help()` of round 1. By then
`help()` has freed 64 of the run's frames and set `helped`, the drain frees the other 64, and round 2 finds the run. The
unfixed code finds it the same way, so the test passes on the code it was written to catch. The same vacuous pass
happens on `--smp 4` when the reaper runs on another CPU, and the part report records that. Nothing in the test reports
either case.

Scenario: a test guest whose memory size, or whose frames reserved before the test, put the dry run's total at
`48k + 1` words. On the unfixed code the test passes, and so does the mutant `K-contig-no-rescan-after-empty-help`,
which it is listed as killing.

Fix: make the test check its own premise. At the drain call, assert that `crate::reaper::held()` is still `RUN`, so
that no help has run yet, and record that the drain ran. After the create, assert that the drain ran and that `got` is
`run`. The test then fails loudly instead of passing vacuously. The 3 of 3 failures on the unfixed code show that the
counts hold for the default test memory size.

### 2. note: the churn test does not check that the cap is what ended the create

Where: `kernel/src/testing.rs:3779-3785`.

What is wrong: the test checks only an upper bound (`calls <= bound`) and the status. A create ended early by the
no-help refusal, for example because the churn could not refill the queue, also passes. So would a mutant that
lowers the cap, such as `/ MAX_CONTIG_FRAMES + 2` changed to `+ 1`. The upper bound alone does not show that driver
2's cap is the cap ADR-0349 states.

Fix (optional): count the scans, that is the step points of a scan that follows a help phase, or assert
`calls >= rounds * scan / 2`. Either shows that about `rounds` rounds ran.

### 3. note: two texts were not updated for the new case

Where: `kernel/src/memory/frames.rs:100-102`, `docs/adr/invariants.md:672` and ADR-0349 INV-0349-1.

What is wrong:
- The doc comment of `contig_help_rounds` still says that "a round that frees nothing ends the search, so a create is
  refused only when the reaper holds nothing". The search now also goes on when a frame was freed since the scan
  began.
- INV-0349-1's text and its Check name the step point before the first help and the two older tests. They do not name
  a free during a later scan, or `a_run_the_reaper_frees_during_a_later_scan_is_found`, although the mutant list
  credits that test for INV-0349-1's behaviour.

Fix: add "or a frame was freed since the scan began" to the doc comment. Add the case and the test to INV-0349-1 and
to its `invariants.md` row. The ADR is proposed, so its invariant may be edited. Then run `make adr-check`.
