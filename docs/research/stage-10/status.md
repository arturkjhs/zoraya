# TASK-28 status

Where the work stands, for whoever picks it up after an interruption. Updated at each commit.

- 2026-11-13: Part 1 (the survey) written and committed.
- 2026-11-14: Part 2 written and committed: proposed ADR-0331 to ADR-0342, `plan.md`, `rows-to-add.md`. A read-only
  stand-in for `make adr-check` (headings, status, citations against `docs/research`, the Evidence line, invariant
  numbering) reports 0 errors over the twelve files; `make adr-check` itself was not run (it needs `cargo`, which
  this task does not use).
- Next: the independent review, its findings and dispositions in `review-notes.md`, the fixes, one check of the
  changed lines.
- ADR numbers: TASK-28 reserves 0330 to 0349, and `main` already has an ADR-0330 (the `ping` command), so this task
  starts at 0331. 0343 to 0349 are unused.
