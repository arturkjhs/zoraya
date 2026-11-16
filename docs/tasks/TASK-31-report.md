# TASK-31 report: stage 10 (living in the system)

## Status for the architect

Updated 2026-11-15, after 10d.

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
