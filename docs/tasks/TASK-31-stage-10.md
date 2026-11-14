# TASK-31: Stage 10 — living in the system (10a to 10i)

## Where, who and how

On the Mac mini (M4, 16 GB), run by a dispatcher as TASK-27 was: **do not ask; decide, record, continue.** Start it after TASK-30 (C++) is merged and tagged `stage-cxx`; until then only part 10a's reading may begin. If 9f is still open, finish it first in the same session.

- **Models:** this main session is a thin dispatcher on **Sonnet**; each part, or each large step of a part, goes to a fresh `general-purpose` subagent with model `sonnet` in its own worktree; `opus-review` (Opus) reviews every part and debugs what two attempts did not fix; `ci-watch`, `log-triage`, `test-runner`, `mutant-triage` (Haiku) wait on CI and read logs. No Fable. Keep only summaries here; write the handoff after each part.
- **Machine limits** (`CLAUDE.md`): at most two parts with builds at once, `CARGO_BUILD_JOBS=3` per part, one port build at a time, one QEMU test at a time per part, mutation runs only when the other lane is idle. Disk: check before each build; below 25 GB remove merged, unlocked worktrees; remove a part's worktree as soon as it is merged; never `git worktree remove -f -f`. Never install outside `build/`.
- **CI** is the shared bottleneck: one candidate per part at a time; batch fixes so that each part needs few candidate runs. A TASK-29 audit branch (`work/test-audit`, from the MacBook Air) may arrive: merge it like a part when its report says it is ready.

## Why

After stage 9 Zoraya runs real programs, but a person cannot yet work in it: no full-screen terminal programs, no `vi` or `less`, a partial userland, no `git`, no `ssh`. Stage 10 makes a working day inside Zoraya possible, keeping Zoraya's rule: a program gets what the line says, and the person sees it.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, **`docs/research/stage-10/plan.md`** (the parts, their order, each one's "done when", what each hands the architect), `docs/research/stage-10/survey.md` and `review-notes.md`, and **ADR-0331 to ADR-0342 in full** (accepted 2026-11-14; ADR-0337 with the grant line, System+G). Read in full the ADRs each part names before starting it, and the tensions table in `docs/research/stage-10/rows-to-add.md`: those changes to accepted ADRs are decided.

ADR numbers **0343 to 0349** and **0380 to 0399** are reserved for this task. Where accepted ADRs are silent, choose what is simplest, least surprising, reversible and safest, write it as a proposed ADR, and continue.

## The parts

As `plan.md` defines them, with its "done when" as each part's acceptance test:

- **Lane 1, the terminal:** 10a (parser and grid), 10b (keys, mouse, selection, paste), 10c (a POSIX program has a terminal from `zosh`), 10d (ncurses, `less`, `vi`), 10f (giving a file to a running program: the status row and the grant line).
- **Lane 2, programs:** 10e (the userland; ADR-0212's image cache first if it is not done), 10g (a program's own directory, and ssh; after 10c), 10h (git; after 10d and 10g).
- **10i** (living in it: the tour, `docs/design.md`, the checklist) runs alone at the end.

The two lanes run in parallel. 10f and 10g both change `zoraya-describe` and the shell's planner: not in review at the same time; the second rebases on the first. Mutation runs for 10a (`zoraya-term`), 10c (`zoraya-tty`) and 10f (`zoraya-shell`), `--in-diff`, at most 45 minutes each.

## For every part

- The "done when" test of `plan.md`, the invariants its ADRs assign, host tests for every new crate, fuzz targets the ADRs name.
- `make check` and `make test-changed` locally; the candidate CI before the merge; `ci-watch` on CI.
- Review by `opus-review`, findings and dispositions in `docs/tasks/TASK-31-review-<part>.md`; one check of the changed lines if the review found an error. Trust-boundary focus: no program output reaches the status row or the grant line; System+G reaches no client; a grant given while running is bounded by what the shell holds, goes to one program and is stored nowhere; reads and mode changes from a line not in front are refused.
- What `plan.md`'s "What each part hands the architect" lists (screens, measurements, hand-run transcripts), in the report.

## Merging

As in TASK-27: merge `origin/main` into the branch just before the merge, recompute the invariants Total from the rows, merge with `--no-ff` from a detached HEAD at `origin/main`, push `HEAD:main`, wait for `main`'s CI; green: tag `stage-10<x>` and delete the branch; red from plumbing: fix forward up to three times; red from a real failure: fix forward twice, then revert and continue on the branch. A part whose proposed ADR changes an accepted decision waits on its branch with the reason in the report; the next part goes on. Tag `stage-10` when all nine are merged and a full run on the tagged commit is green.

Stop completely only for: anything outside this repository's worktrees, force-pushes or moved tags, memory or disk exhaustion despite the limits, untrusted code (the gate stays closed). On a usage-limit warning: commit, push, update the report and handoff, stop.

## Report

`docs/tasks/TASK-31-report.md` with a "Status for the architect" block at the top, updated and pushed after each part: what is merged and tagged, what is in progress, the proposed ADRs with one-line recommendations, the hand checks the architect must do (10f's grant line, 10g's login to a real host, 10h's clone and push) with exact commands, the measurements, and the time and model per part.

## Git

Configured identity only. Branches `work/10a` … `work/10i`; tags `stage-10a` … `stage-10i` and `stage-10`. Commit and push after each step.
