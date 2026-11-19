# ADR-0463: CI: parts merge on local tests and review; main's full set runs every two hours

- **Status:** accepted
- **Date:** 2026-11-16
- **Depends on:** ADR-0348
- **Evidence:** test-audit/report.md#5-time

## Context

ADR-0348 (2026-11-15) made a merge candidate run the steps its change needs, and made every merge wait for that run to be green. (Full-run times: test-audit/report.md#5-time.) On 2026-11-16 the scheduled run of `main` was red on a timing assertion that no candidate had shown, and the candidate runs still cost CI time and queue on the shared free plan for parts that `make check`, `make test-changed` under HVF and an independent review had already covered. The architect's decision of 2026-11-16 is recorded below.

## Decision drivers

1. CI time and queue limit how fast parts merge (ADR-0348 driver 1).
2. `main`'s green must still mean that the full set passed on `main`'s content, and a red run must be fixed or reverted quickly (ADR-0348 driver 2).
3. A change to CI itself cannot be tested by the local tiers, so it keeps a candidate run.

## Considered options

### Option A: the policy as decided
- **Description:** the four items of the decision below, in `CLAUDE.md`, `.github/workflows/test.yml` (the schedule), `tools/check-ci-jobs.sh` and `docs/architect-queue.md`.
- **Evidence for:** the architect's decision; drivers 1 to 3.
- **Evidence against:** a merge that breaks something the local tiers do not cover (TCG, the full GUI set, the nightly's own jobs) is found by the next scheduled run, up to two hours later plus the queue; the revert rule limits how long `main` stays red.
- **Cost:** *(estimate)* one cron line, a check, and text.
- **Precedent:** ADR-0348.

### Option B: ADR-0348 as it stands
- **Description:** every part waits for a green candidate run before its merge.
- **Evidence against:** the problem of the Context.
- **Cost:** *(estimate)* none.
- **Precedent:** ADR-0348.

### Option C: the full set on every push to main (the baseline before ADR-0345)
- **Evidence against:** ADR-0348's Context (two full sets for one commit, every merge waiting).
- **Cost:** *(estimate)* one full run a merge.
- **Precedent:** the baseline.

## Recommendation

Option A, as decided. This section is not a recommendation to the architect: the decision is below and may not be reworded.

## Consequences

1. This ADR supersedes the items of ADR-0348 about candidate runs: item (2) of its decision (a candidate runs the steps `tools/ci-plan.sh` selects) as a rule for every part, and the rule of item (3) that the next part merges as soon as its own candidate is green, together with the rule in `CLAUDE.md` that a candidate run must be green before a merge. It keeps the rest of ADR-0348: a push to `work/*` runs `make check` only, a push to `main` runs the document checks only, `main`'s full set runs on a schedule and nightly, a red scheduled run is fixed first, and the summary of a scheduled run lists the merges since the last green one. The mapping of a change to steps (`tools/ci-plan.sh`) stays, for the candidate runs of item 3 and for `make test-changed`.
2. The schedule of the full set of `main` is `23 */2 * * *` (every two hours, an off minute) in place of `23 */4 * * *`; the nightly (`17 2 * * *`) stays. The plan step still skips a scheduled run when `main` has not changed since the last full run, and fails it if that last full run was red (`tools/ci-plan-step.sh`, `tools/ci-last-full.rb`).
3. A part merges when `make check`, `make test-changed` under HVF and its independent review pass locally. No candidate run is dispatched for it.
4. A red scheduled run is fixed first, before any merge. If it is not fixed within an hour, the merge that broke it is reverted and fixed on its branch.
5. A candidate run (the tag `candidate/BRANCH`, a pull request or `gh workflow run test.yml --ref BRANCH`) is made only when a part changes `.github/workflows`, the ci-plan scripts (`tools/ci-plan.sh`, `tools/plan-lib.sh`, `tools/ci-plan-step.sh`, `tools/ci-makefile-steps.rb`, `tools/ci-last-full.rb`) or `tools/test-runner/`; when a failure shows only under TCG; and as `candidate-full/NAME` before a stage tag, where no green scheduled or nightly run already includes the commit. Adding or changing a test's own lines in `tools/ci-tests.txt` or `tools/test-map.txt` needs no candidate run; the agent tags a candidate itself (amended 2026-11-19, see the Architect's decision).
6. A stage tag needs a green scheduled, nightly or `candidate-full/` run that includes the tagged commit (`tools/ci-last-full.rb --green-for SHA`).

Known limits: the checks of this ADR cover the cron lines and the plan step's skip rule; the merge rules and the revert rule are conduct, not checked.

## Invariants

- **INV-0463-1.** The workflow has the nightly cron (`17 2 * * *`) and the cron every two hours (`23 */2 * * *`), and no cron every four hours. *Check:* `make check-ci-jobs`.
- **INV-0463-2.** A scheduled run that finds nothing new on `main` since the last full run plans nothing and is green if that run was green, and is red if it was red; if the lookup fails, it plans the full set. *Check:* `make check-ci-plan` (fixtures).
- **INV-0463-3.** A part is merged on its local `make check`, `make test-changed` and independent review; a red scheduled run not fixed within an hour has its breaking merge reverted. *Check:* none (conduct of the sessions).

## Open questions

- Whether the one-hour rule should be checked by a script that lists red scheduled runs older than an hour.

## Revisit triggers

- A merge that broke `main` and was reverted, more than once a week.
- A faster runner, so that the full set takes minutes.

## Architect's decision

Accepted 2026-11-16. Decided by the architect; recorded by an agent at the architect's request.

CI decision of 2026-11-16: (1) A part merges after make check, make test-changed under HVF and its independent Opus review pass locally; no candidate run in CI. (2) main's full set runs on a schedule every 2 hours when main changed since the last full run, plus the nightly. A red scheduled run is fixed first, before any merge; if it is not fixed within an hour, revert the merge that broke it and fix it on its branch. (3) A candidate run in CI only when a part changes the CI workflow (.github/workflows), tools/ci-plan.sh (and what it reads: tools/ci-tests.txt, tools/test-map.txt, tools/plan-lib.sh, tools/ci-plan-step.sh, tools/ci-makefile-steps.rb, tools/ci-last-full.rb), or tools/test-runner; when a failure shows only under TCG; and candidate-full/ before a stage tag where no green scheduled or nightly run already includes the commit. (4) A stage tag needs a green scheduled, nightly or candidate-full run that includes it (tools/ci-last-full.rb --green-for SHA).

Accepted 2026-11-19. Decided by the architect; recorded by an agent at the architect's request.

Two corrections to item 5 and to who tags: (1) adding or changing a test's own lines in `tools/ci-tests.txt` or `tools/test-map.txt` (a new step, a map entry) does not need a candidate run; only changes to `.github/workflows`, the ci-plan scripts (`tools/ci-plan.sh`, `tools/plan-lib.sh`, `tools/ci-plan-step.sh`, `tools/ci-makefile-steps.rb`, `tools/ci-last-full.rb`) and `tools/test-runner/` do. (2) The agent tags candidate runs itself (`git push` of `refs/tags/candidate/...`, as a command of its own); the architect does not.
