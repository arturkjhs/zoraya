# ADR-NNNN: <decision title>

- **Status:** proposed | accepted | rejected | superseded by ADR-XXXX
- **Date:** YYYY-MM-DD
- **Depends on:** ADR-XXXX, ... (or none)
- **Evidence:** DM-xxx, GR-xxx, CP-xx, X-n, task-02/<file>#<section>, ...

## Context
The problem, in terms of our project. What forces are in play. What is measured vs. assumed.

## Decision drivers
Numbered list. Each driver cites evidence IDs or measurements.

## Considered options
At least three. Always include the "do what Linux/POSIX does" option as a baseline, and at least one option from a non-Unix system in `alternatives-postmortem.md`.

### Option A: <name>
- **Description:**
- **Evidence for:** (with IDs)
- **Evidence against:** (with IDs)
- **Cost:** implementation effort, performance, compatibility, complexity — estimated, labelled as estimates
- **Precedent:** which systems did this, and how it went

(repeat per option)

## Recommendation
The agent's recommended option and the reasoning, clearly separated from the neutral option analysis above. Marked as a recommendation, not a decision.

## Consequences
If the recommendation is accepted: positive, negative, and follow-up work it creates.

## Invariants
Rules that follow from the decision and can be checked mechanically (by a test, a lint, a CI script, or a type in the `abi` crate). For each: the rule, and how it would be checked. Do not implement the checks.

## Open questions
What is unknown and how it could be resolved (measurement, prototype, research).

## Revisit triggers
Concrete new evidence that should reopen this decision.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
