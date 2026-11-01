# ADR-0071: Checking the totals in `invariants.md`

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0001
- **Evidence:** CP-13

## Context
`docs/adr/invariants.md` ends with a Counts table: invariants per ADR and a total. The sixth review of TASK-10 found the total one short (W1), and it had been wrong for some time, because `make adr-check` reads neither. A hand-kept number that nothing checks becomes false. CP-13 asks that the record stays trustworthy.

## Decision drivers
1. A false count must fail the build.
2. Small: `adr-check` uses `std` only (CLAUDE.md).
3. The file stays readable by a person.

## Considered options

### Option A: Trust review (the Linux way, `MAINTAINERS` and similar)
- **Description:** People keep the table right.
- **Evidence for:** Nothing to build.
- **Evidence against:** W1 is what that produced.
- **Cost:** None now.
- **Precedent:** Hand-kept lists in most projects.

### Option B: Check the table against the rows
- **Description:** `adr-check` counts rows per ADR and compares each row of Counts and the Total.
- **Evidence for:** Drivers 1 to 3.
- **Evidence against:** Each new invariant touches two more lines.
- **Cost:** *(estimate)* 40 lines.
- **Precedent:** ADR-0001's other checks.

### Option C: Remove the table, and generate it when wanted
- **Description:** No stored count (as a Plan 9 tool would derive it `[outside evidence]`).
- **Evidence for:** Nothing can be false.
- **Evidence against:** The table is useful when reading, and a generated file needs a step.
- **Cost:** *(estimate)* Small.
- **Precedent:** Derived data in general.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- `adr-check` finds `## Counts` in `invariants.md`, compares each row's number with the rows of that ADR, requires a row for each ADR with invariants, and compares the Total with the rows in all. Any difference is an error of a new INV-0071-1. A missing section is an error.

### Effect of the vision
None for any target. It is a tool for this repository.

### Deferred to stage 6
Nothing here depends on POSIX. ADR-0055 holds the one requirement on stage 4.

## Consequences
- **Positive.** The counts cannot drift.
- **Negative.** A small cost per invariant.
- **Follow-up.** None.

## Invariants
- **INV-0071-1.** The Counts table in `invariants.md` matches its rows, per ADR and in total. *Check:* `make adr-check`, with host tests of a wrong row, a wrong total and a missing section.

## Open questions
- None.

## Revisit triggers
- The table is dropped from the file.

## Architect's decision
