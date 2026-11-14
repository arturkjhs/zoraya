# TASK-28 status

Where the work stands. Updated at each commit.

- 2026-11-13: Part 1 (the survey) written and committed.
- 2026-11-14: Part 2 written and committed (`56b3cd6`): proposed ADR-0331 to ADR-0342, `plan.md`, `rows-to-add.md`.
- 2026-11-14: The independent review ran on `56b3cd6` (three reviewers, 86 findings; fixes in `9765226`), then the
  check of the changed lines (26 points; fixes in `eb08afb`), then the last check TASK-28 allows (10 points, none
  high; fixed in the commit after `eb08afb`, and not reviewed again). Every finding has a disposition in
  `review-notes.md`.
- **The task is complete as far as an agent can take it.** What is left is the architect's: the decisions on the
  twelve ADRs, and the rows of `rows-to-add.md` for `index.md`, `invariants.md` and the roadmap.
- Not run, because TASK-28 forbids `make` and `cargo` here: `make adr-check` and `make check`. A read-only stand-in
  for `adr-check` (template headings, status, an empty "Architect's decision", every citation resolving in
  `docs/research`, the Evidence line equal to the body's citations, invariant numbering) reports 0 errors over the
  twelve files. The first session with a toolchain should run `make adr-check`; it is expected to report twelve
  `INV-0001-5 ... is not listed` errors until the index rows are added, and nothing else.
- ADR numbers: `main` already has an ADR-0330 (the `ping` command), so this task uses 0331 to 0342, as the
  architect confirmed. 0343 to 0349 are unused.
