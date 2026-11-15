# TASK-34: Foundations for big programs (F2 to F8), second dispatcher on the MacBook Air

## Where, who and how

On the architect's **MacBook Air M1**, as a **second dispatcher** beside TASK-31 (stage 10) on the Mac mini. Both merge into `main`. **Do not ask; decide, record, continue.**

- **Clone:** a new worktree of `~/workspace/zoraya` at `~/workspace/zoraya-f` (`git -C ~/workspace/zoraya worktree add --detach ~/workspace/zoraya-f origin/main`); each part in its own worktree beside it. Do **not** use `~/workspace/zoraya` (its own branch) or `~/workspace/zoraya-main` (another session's toolchain).
- **Models:** this main session on **Sonnet**, a thin dispatcher; each part, or each large step, to a fresh `general-purpose` subagent with model `sonnet` in its own worktree; `opus-review` (Opus) reviews every part and debugs what two attempts did not fix; `ci-watch`, `log-triage`, `test-runner` (Haiku) for CI and logs. No Fable.
- **This machine:** **one** part with builds at a time. Check `sysctl -n hw.memsize`: 16 GB or more, `CARGO_BUILD_JOBS=3`; 8 GB, `CARGO_BUILD_JOBS=2`. One QEMU guest at a time. Disk: below 20 GB free, remove this task's merged worktrees; never touch other folders. Keep the machine awake (`caffeinate`). Never install outside `build/` (Homebrew packages the build needs may be installed; list them in the report).
- **CI:** one candidate of this task at a time.

## Why

The browser-engine design (TASK-33, merged; ADR-0411 to ADR-0421 accepted, ADR-0410 open) and outside work need the same POSIX foundations: Unix sockets with descriptor passing, shared memory, `posix_spawn` with grants, more mappings per process, shared code, socket files and listening. F1 (`poll`, `ppoll`) is merged. Each part is useful on its own (CPython's `asyncio` self-pipe and `http.server`, `tmux` later, any program that uses `socketpair`).

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, **`docs/research/browser-engine/plan.md`** (the parts, their order, each "done when"), ADR-0411 to ADR-0421 in full (accepted 2026-11-14; each records which foundation part changes which older ADR, with an Opus review), and the older ADRs each part changes (ADR-0103, ADR-0108, ADR-0225, ADR-0287, ADR-0310, ADR-0315, ADR-0341, ADR-0342). Also the backlog's generic requests: `socketpair`, `pread`/`pwrite`, `usleep`, `mkdtemp` (do the small ones inside the first part that touches libposix, if TASK-31 has not).

ADR numbers **0430 to 0449** for decisions where accepted ADRs are silent (proposed; simplest, least surprising, reversible, safest).

## The parts, in this order

F2 (Unix sockets, `socketpair`, descriptor passing), F3 (shared memory), F4 (`posix_spawn` with what each child is given), F8 (the 2 GiB guest for the browser's steps), F5 (4096 mappings), F6 (read-only code shared between a program and the copies it spawns), F7 (socket files and TCP listen). Each as `plan.md` defines it, with its "done when" as the acceptance test.

## Sharing `main` with TASK-31

- TASK-31's 10c and 10d change `libposix` too. Before a merge: merge `origin/main` into the branch, re-run `make check` and the part's tests, recompute the invariants Total from the rows; if the push to `main` is refused because the other dispatcher merged first, repeat.
- Never push `main` unless `make check` exited 0 in the same command chain (CLAUDE.md).
- Do not change files only stage 10 owns (the terminal crates, `zoraya-tty`, the shell's planner) except where a part needs it, and then say so in the report.

## For every part

The "done when" test; the invariants its ADRs assign; host tests for new code; the Opus review with findings in `docs/tasks/TASK-34-review-<part>.md`; one check of the changed lines if it found an error; the candidate CI green; merged with `--no-ff` from a detached HEAD at `origin/main`; tagged `stage-f<n>`; worktree removed. Mutation runs `--in-diff`, at most 30 minutes, for the parts that change the kernel (F3, F5, F6).

## Report

`docs/tasks/TASK-34-report.md`, a "Status for the architect" block at the top, pushed after each part: merged and tagged, in progress, proposed ADRs with one-line recommendations, measurements, time and model per part. On a usage-limit warning: commit, push, update the report, stop.

## Git

Configured identity only (the same as on the Mac mini). Branches `work/f2` … `work/f8`; tags `stage-f2` … Commit and push after each step.
