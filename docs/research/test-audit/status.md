# TASK-29: where the audit stands

Updated 2026-11-14, after the first fixes. The hand-over note the task asks for; the report is `report.md`.

## Done

- Part 1: `report.md` (items 1 to 6), with its tables in `data/`, the readers' notes in `notes/` and the scripts
  that fetched and summarised the run history in `scripts/`. The raw history and logs are in `build/audit/` of the
  audit's clone only (not committed).
- Part 2, committed and pushed on `work/test-audit` (`origin/main` merged in at `ce12282e`), each with what was
  shown in its commit message:
  - D1, D2, D3 with proposed ADR-0350: one run where `auto` is TCG; CI's list has 42 jobs instead of 47.
  - T3 (60 s of fuzz on a candidate), T2 (`Swatinem/rust-cache` v2.9.3), V41 (`check-mutant-lists` in CI).
  - V1 (the runner's `~` and `!` take the escapes).
  - F5 and V22 (`console.script`), V6, V7, V8, V14, V20, V21 (scripts), V34 (no skip on CI), V29 (crash tests).
  - `make check` passed locally on the branch before the merge of `origin/main` (21 of 21); after the merge
    `check-ci-jobs`, `adr-check`, `check-ci-plan` and `check-host-paths` passed.

## Not done yet

1. The candidate run: no `candidate/work-test-audit-*` tag has been pushed. When this note was written two runs of
   `main` were queued, and the task forbids a candidate then. Push `candidate/work-test-audit-a` on the branch's
   head when `main` is idle, watch it with `ci-watch`. It is the first run of every script edit and of the three
   steps V1 wakes up (`console.script:117`, `files-write.script:174`, `zosh.script:77`): a red step there is either
   a wrong correction or a real defect, to be told apart from its log.
2. The review: an `opus-review` reader was started on the branch; its findings go to `review-notes.md`, then the
   fixes and one check of the changed lines.
3. The report's "Status for the architect" still gives estimates; replace them with the candidate's measured
   figures and say the branch is ready.

## Left as proposals (report, sections 4 to 6)

V2, V3, V4 (needs the time to the first sync of a crash round), V9 to V13, V15 to V19, V23 to V28, V30 to V33, V35,
V36, V40, V42 to V45, T1, T4, T5, C1 to C9, and the `t_quota` handshake.
