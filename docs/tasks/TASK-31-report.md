# TASK-31 report: stage 10 (living in the system)

## Status for the architect

Updated 2026-11-14, after 10a and 10e.

- **Merged and tagged:**
  - `stage-10e` on `3981656` (`main`'s run green, 72 jobs): the userland.
  - `stage-10a` on `e3e5774` (`main`'s run green, 72 jobs): the terminal's parser and grid.
  - `work/test-fixes-2` merged as `914cf12` (`main`'s run green); it carries no stage tag.
- **In progress:** 10b (keys, mouse, selection, paste; `work/10b`, from 10a's head); `work/py-hook` (the python3 build
  hook, the first of the three requests from outside work; fix pass after its review, then a candidate).
- **Added by the architect (2026-11-15):** part 10j, the implementation of ADR-0396, ADR-0397 and ADR-0399 (accepted
  2026-11-15), after 10g in lane 2.
- **Waiting:** 10c, 10d, 10f (lane 1); 10g, 10h (lane 2, after 10c, 10d and 10g); 10i (alone, at the end). Backlog
  requests 2 (`poll` and `ppoll`) and 3 (programs installed on the volume, design first) wait for a free slot.
- **Proposed ADRs (all `proposed`; the architect sets statuses):**
  - ADR-0343 (names toybox's commands add to `libc.a`) and ADR-0344 (what 10e decided where ADR-0336 and ADR-0342 are
    silent): recommend accepting both.
  - ADR-0380 (terminal behaviour where ADR-0332 is silent), ADR-0381 (the 16-byte cell), ADR-0382 (how conformance is
    evidenced): recommend accepting all three.
  - ADR numbers handed out: 10a 0380 to 0382; 10b 0383 to 0385; the python hook 0396 (unused).
- **Decisions for the architect (nobody was asked; these were decided to continue, each reversible):**
  - ADR-0336 item 2's review gate for `tr`, `expr` and `diff` was met by an independent `opus-review` (it did not port
    them), with a toybox patch (`ports/patches/toybox/0003`) for five defects it found. Overrule if you want a human.
  - The ADR-0212 image cache was **not built** in 10e (it is a broker, `posix_child` and `libposix` change with its own
    security argument). The userland does not depend on it.
  - 10a's stop-gap: `zosh` writes a fixed reset before a prompt that follows a command, until 10c sends `Restore`.
  - INV-0172-4: the console shell holds the volume root, so `room new X +w /t` is not refused there; the review judged
    it by design (ADR-0172 point 3). Whether entering a room should narrow `room new` is yours.
  - INV-0230-1 is no longer `checked`: the network test steps use `--net-open`. What `--net-open` may reach is yours.
  - INV-0042-1 is `not yet applicable` (the new `checked:` rule refused its old text); is an `abi` type a check?
  - The semaphore, barrier and latch groups of libc++'s tests run in no CI job (TCG too slow); a nightly step is open.
- **Hand checks for the architect:** 10a: an xterm comparison (`TERM=xterm-256color cat crates/zoraya-term/recordings/vi-edit.rec`
  in an 80x24 xterm, against the same recording in a Zoraya terminal; there is no xterm on the build machine) and
  `vttest` (needs a pin in `ports/sources.lock`). 10f's grant line, 10g's login and 10h's clone and push are not built yet.
- **Measurements:** 10e's W4 (HVF only, before the `config.sub` fix): 39.25 s wall time, 141 `exec` (mean 49 ms), 639
  `posix_spawn`, 3205 broker round trips, 709,735 system calls (`docs/tasks/TASK-31-part-10e.md`). The libc.a relink fix:
  the build job took 16 min in the candidate run (a ports-cache miss, so the saving is not yet shown).
- **Cleanup waiting for permission:** the merged branches (`work/test-fixes-2`, `work/10e`, `work/10a`) and their
  worktrees; an automated check refused the delete, so nothing was removed.

## Start (2026-11-14)

- TASK-31's file was already on `main` (`b3a4148`, with the handoff), so the first docs push carries only this report.
- `stage-cxx` and `stage-9f` are tagged; both lanes started. The handoff's first item (the run on the head of `main`,
  `0c3ada9`, run 43598033280) was still queued at the start; no push to `main` is made while it is queued or running,
  because a push cancels it.
- The "handoff at the end of `docs/tasks/TASK-27-report.md`" is `docs/handoff.md` (the report has none).
- Disk at the start: 101 GB free.

## Time and model per part

| Part | Model | Start | End | Notes |
|---|---|---|---|---|
| 10a | Sonnet (implementation), Opus (review, changed-lines check) | 2026-11-14 | 2026-11-14 (`stage-10a`) | 4 candidate runs (a, b failed; c green) |
| 10e | Sonnet (implementation), Opus (review, fix passes, gate review) | 2026-11-14 | 2026-11-14 (`stage-10e`) | 4 candidate runs (a, b failed; d green; c green before the gate fixes) |
| test-fixes-2 | Sonnet, Opus (review) | 2026-11-14 | 2026-11-14 (`914cf12`) | 2 candidate runs, both green |

## Handoff at the usage limit (2026-11-15)

Merged and tagged: `stage-10a`, `stage-10b`, `stage-10e`. Merged without a stage tag: `test-fixes-2`, `py-hook`, `poll`,
`ci-expr` (main green on each), the volume-programs design, ADR-0450 to ADR-0455 (docs).

In flight (each on its own branch; the reviews are local branches `docs/review-*` not yet merged as records):
- `work/10c`: opus-review fix pass (job control after 63 commands, Ctrl-C to the chosen group, mode 2004 only when not
  canonical, INV-0172-4 check, mutation holes); then a changed-lines check, a candidate (`candidate/work-10c-a` ran on the
  old head) and the merge.
- `work/test-fixes-3`: candidate `candidate/work-test-fixes-3-c` (head 18c10e4) running; the earlier two failed on CI's QEMU
  refusing a guestfwd on 10.0.2.2 (fixed); the `unshare -n` container options are still unverified on CI.
- `work/ci-once`: candidate `candidate/work-ci-once-a` (efdf63d) running; a changed-lines check running.
- `work/syscall-long`: candidate `candidate/work-syscall-long-a` (5f49dfe) running; opus-review running.
- `work/ports-early`: C port builds (ncurses, less, nvi2, git, OpenSSH) in progress, no result yet.
Next: 10d, 10f, 10g, 10h, 10j, 10i, `work/rust-std` (ADR-0450 to 0455), `work/test-fixes-2`'s leftovers. The cleanup of merged
branches and worktrees was refused by an automated check and is not done (disk was 45 GB free). `docs/architect-queue.md`
lists what waits for the architect.
