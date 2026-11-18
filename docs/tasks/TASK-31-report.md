# TASK-31 report: stage 10 (living in the system)

## Status for the architect

Updated 2026-11-15, after 10d.

- **CI policy (2026-11-16, ADR-0463, accepted):** parts merge on local `make check`, `make test-changed` and review, with no candidate run (except for workflow, `tools/ci-plan.sh` or `tools/test-runner` changes); `main`'s full set runs every 2 hours and nightly; a red scheduled run not fixed within an hour has its merge reverted.

- **Merged on `main`:**
  - `stage-10a` (`e3e5774`), `stage-10b` (`23b338d`), `stage-10e` (`3981656`): tagged.
  - `stage-10c` on `abaa43c` and `stage-10d` on `c9bab16` (with `work/ports-early`): tagged 2026-11-15 on the scheduled full run
    43749759496 (head `9b78d380`, 78 of 78 jobs green; `tools/ci-last-full.rb --green-for` names it for both).
  - Small parts, no stage tag: `test-fixes-2`, `py-hook`, `poll`, `ci-expr`, `ci-once`, `test-fixes-3`, `syscall-long`,
    `ci-policy` (ADR-0348, `47f59d2`), `alloc-flake` (`f83d1fb`), `tcg-slow` (`4640c56`).
- **In progress:**
  - `work/sigint-exec` (libposix: a Ctrl-C right after a job's first output was lost; a bug in `main` since 10c): fix,
    review and check of the changed lines done; ADR-0392 proposed; candidate `-b` running, a last doc and test pass after it.
  - `work/10g` (own directory, ssh): implemented, reviewed (3 must-fix: ssh in a room's terminal, the test sshd's
    confinement, an ADR number clash); the fix pass and the merge with `work/10d`'s ssh port are in progress.
- **Waiting:** 10f (lane 1), 10j, 10h (lane 2), `work/rust-std`, 10i (alone, at the end).
- **Proposed ADRs (the architect sets statuses; the table is in `docs/architect-queue.md`):** decided 2026-11-16 and recorded as
  accepted: ADR-0386, 0387, 0390, 0391 (with `narrow` taking a list of names, to be built), 0392 (with its amendments of
  ADR-0106); DB 1.85's licence accepted (the University of California rescinded the advertising clause in 1999). Still
  `proposed`: ADR-0388, 0389 (10c), ADR-0410, 10g's ADR (renumbered in its fix pass), and two new ones with the architect's
  decision of 2026-11-16 already given as Option A: **ADR-0460** (charge but do not park the thread of a process that is
  ending: a killed room dies at once; to be accepted after the implementation and its QEMU kernel test, which another agent
  builds) and **ADR-0461** (git's licence: `git-remote-http` links a libcurl built with mbedTLS 3.6 LTS; before 10h).
  ADR numbers handed out: 10a 0380 to 0382; 10b 0383 to 0385; the python hook 0396 (unused); `work/sigint-exec` 0392;
  **0460 to 0469 are reserved for TASK-31** (0460 and 0461 used).
- **Hand checks for the architect:** section C of `docs/architect-queue.md`: 10a's xterm comparison and `vttest` (now
  pinned by 10d; the grids are frozen from `zoraya-term`, not compared with xterm by a person), 10g's login to a real
  host (exact commands in `docs/tasks/TASK-31-part-10g.md`). 10f and 10h are not built yet.
- **Known gaps of merged parts:** 10c: mode 2004 can stay disabled for a reader that never asked for bracketed paste;
  `Clone` from a background line is allowed; `room --leave` can escape. 10d: `narrow` with a list of names is not built
  (two files in one directory are refused with a hint); `ls | less` does not page; DB 1.85 not measured on a large file.
  `w9-term` timed out once under TCG on CI and did not reproduce (`docs/backlog.md`).
- **Measurements:** 10e's W4 and the libc.a relink are in `docs/tasks/TASK-31-part-10e.md`; 10d's binary sizes are in
  `docs/tasks/TASK-31-part-10d.md` (`less` 2,975,880 and `vi` 3,340,736 bytes). SHA-256 of 64 MB for 10j: not yet measured.
- **Cleanup waiting for the architect:** merged branches and their remote refs (`work/10a`, `10b`, `10c`, `10d`, `10e`,
  `test-fixes-*`, `ports-early`, `ci-policy`, `tcg-slow`, `alloc-flake`, candidate tags).

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

## Handoff for a fresh session (2026-11-17, 01:30 UTC)

This block supersedes the "Status for the architect" block above where they differ. Read `CLAUDE.md`, `docs/architect-queue.md` and this block first.

### main and its runs
- `main` is `95ebf826` (the merge of `work/10g`) when this was written. Green: the scheduled run 43951262290 (2026-11-16 20:34 UTC, `37aefd1f`) and the manual full run 43988741570 (`759864ba`, after `work/ci-cron-fallback`). Push runs on `main` are document checks only.
- No full run includes `95ebf826` yet, so **`stage-10g` is not tagged**. Start one (rule below), then `tools/ci-last-full.rb --green-for SHA`, then tag. Stage tags still waiting for a green full run: `stage-10f` (not merged), `stage-10g`. `stage-10a`, `10b`, `10c`, `10d`, `10e` are placed.
- The red run of 2026-11-16 (scheduled, `059534cf`, `make test-net`) was my ADR-0460 kernel tests, not F7a: `a_killed_room_ends_in_the_period_after_the_kill` assumed 16 ways out fit in one 100 ms period (102 ms on CI's TCG), and `a_start_and_end_loop_keeps_its_ceiling` assumed 300 objects cost 400 us (170 us under HVF). Fixed by `30c2402b` (0 parks, plus one period per whole period run; 900 objects, a table holds 1024 handles).

### Open branches and agents
- **`work/10g`: merged** (`95ebf826`). Its fix passes, three independent reviews (`docs/review-10g-2`, `docs/review-10g-3`, with `docs/tasks/TASK-31-review-10g-3.md`) and the architect's room-network rule (ADR-0462, status stays `proposed`, the recording line is in the ADR) are in. Its `make test-changed` under HVF at `53615e70` passed (RC=0, no FAIL, log in the 10g worktree `build/tc-final.log`); the merge onto `main` was checked with `make check` only. Its earlier push `36a16bac` (third pass) was superseded by `fa822883` and `53615e70`.
- **`work/10f`: not merged.** `origin/work/10f` is still `56691b12` (2026-11-15). The agent's local merge of `origin/main` is `c242a041` in the worktree `../zoraya-10f`, **not pushed**: conflicts resolved in `gui.rs`, `zoraya-proto-file`, the ADR index, the invariants table and the queue; `NarrowNames` renumbered to file ordinal 44 (main's socket requests use 41 to 43); `TableKind::Own` added (a program's own directory is fixed, `libposix` drops a grant at an `Own` prefix); invariants Total 886. `make check` exited 2 there on `check-libposix-widths` (stale `build/sysroot` in that worktree; move it aside, do not delete) and the `make test-changed` started from it died at that check, so **no verdict exists**. Next: move the stale sysroot aside, `make check`, `make test-changed` (with the grant GUI script), push, merge, then the `zoraya-shell` mutation run (`make mutants-host --in-diff`, at most 45 minutes, when the other lane is idle).
- **10j** (installed programs, ADR-0396, 0397, 0399; measure SHA-256 for 64 MB), **10h** (git; waits for ADR-0461 accepted and the mbedTLS pin), `work/rust-std` (ADR-0450 to 0455), **10i** (last and alone): not started. `work/tcg-split` and the other branches belong to the TASK-34 dispatcher.
- Agents of this session are stopped; none is to be assumed running. A `make test-changed` in `../zoraya-10f` may still be running as a process of the stopped agent: look at `build/test-changed-merge.log` there, and do not touch processes that are not yours.
- Review records still on `docs/review-*` branches (`10f`, `10g-2`, `10g-3`, `kwb-time*`, `park-ending`, `sigint-exec*`, `smp-tcg`): merge the ones not yet on `main` as docs.

### What waits for the architect
`docs/architect-queue.md` is the list. In short: ADR-0461 (git licence), ADR-0462 (10g's room terminals, decision recorded, status), ADR-0388, 0389, 0393, 0395, 0410; hand checks (10a's xterm and `vttest`, 10g's login to a real host, exact commands in `docs/tasks/TASK-31-part-10g.md`); the libc++ timing-test exclusion under TCG; the extra line the ci-policy-2 agent put in CLAUDE.md; INV-0348-3's "4-hourly" text; cleanup of merged branches, worktrees and tags (not done by agents).

### Rules agreed
- CI policy ADR-0463: a part merges after `make check`, `make test-changed` (HVF) and its independent Opus review; **no candidate run** except for `.github/workflows`, `tools/ci-plan.sh` and what it reads, `tools/test-runner`, a TCG-only failure, and `candidate-full/` before a stage tag where no green full run covers it. A red scheduled run is fixed first and reverted if not fixed within an hour.
- **GitHub's cron is unreliable** (the runs of 10:23 to 14:23 UTC on 2026-11-16 never started). When `main` changed and no full run has started for 2.5 hours, start one: `gh workflow run test.yml --ref main -f full=true` (check `gh run list --workflow test.yml` first). Recorded in `CLAUDE.md`. Check the agents' branches on origin every half hour rather than waiting for notifications.
- **Separate git commands:** `make check` first; if it exits 0 and `git rev-parse HEAD` is the tested commit, `git push ...` as a call of its own (tags too); chains with `&&`; merge form `git fetch origin`, `git checkout --detach origin/main`, `git merge --no-ff origin/BRANCH`, `make check`, `git rev-parse HEAD`, `git push origin HEAD:main`, each its own call. If the push is refused, start again from `git fetch`.
- Never touch a process that is not yours. At most two parts with builds, `CARGO_BUILD_JOBS=3`, one QEMU at a time; disk below 25 GB means free merged worktrees (28 GB free at this writing). PATH needs `$HOME/.cargo/bin`. A worktree without the ports sources copies `build/ports/src` from another worktree. Never claim a result without the command in the session; never file a failure as "slow runner" without numbers.
