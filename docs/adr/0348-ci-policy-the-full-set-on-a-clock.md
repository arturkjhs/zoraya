# ADR-0348: CI policy: a work branch runs `make check`, a candidate runs what its change needs, and main's full set runs on a clock

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0213, ADR-0345
- **Evidence:** test-audit/report.md#5-time

## Context

The architect's order of 2026-11-15, recorded in "Architect's decision" below: CI is the brake on the work. On that day `main` ran two full sets for one commit, `work/10c` and `work/f2` got full `workflow_dispatch` runs, and every merge waited for `main`'s full run, which takes 66 to 70 jobs and 30 to 67 minutes plus a queue of 10 to 70 minutes on the shared free plan (docs/handoff.md, section 1; test-audit/report.md#5-time).

ADR-0345 (proposed) already tried to cut `main`'s second run by proving the merge's tree had passed the full set. The new policy removes the second run altogether: a push to `main` runs only the document checks, and the full set of `main` runs on a clock.

## Decision drivers

1. CI time and queue are the limit of how fast parts merge (the order above).
2. `main`'s green must still mean that the full set passed on `main`'s content, and a red run must be seen and fixed before more is merged on it.
3. Fail safe: any doubt, including an API error, runs the full set.

## Considered options

### Option A: the policy as decided
- **Description:** the five items of the decision below, implemented in `.github/workflows/test.yml`, `tools/ci-plan.sh` (with `tools/ci-makefile-steps.rb` and `tools/ci-last-full.rb`), `tools/plan-lib.sh` and `CLAUDE.md`.
- **Evidence for:** the architect's order; drivers 1 to 3.
- **Evidence against:** a merge that breaks something no candidate step covered is found by the next scheduled run, up to 4 hours later (plus the queue), with the commits since the last green run listed for blame; two parts merged back to back, each green against an older `main`, are first tested together by that run. A push to `main` runs 5 of the 25 check commands (the document checks): it no longer runs `fmt-check`, `clippy`, `check-workflow-syntax`, `check-ci-jobs`, `check-ci-plan` or `check-test-lanes` (a full scheduled or nightly run runs all 25; a skipping run runs none), so a merge that breaks `test.yml` shows when the next scheduled run fails to start. The merge commit itself is covered only by the rule that `make check` exited 0 on the commit pushed (CLAUDE.md, "Git").
- **Cost:** *(estimate)* two small scripts, a plan rule, a workflow edit and fixtures.
- **Precedent:** the nightly run of ADR-0213 and TASK-26.

### Option B: ADR-0345's tree proof on every push to main
- **Description:** keep `main`'s push run, and skip its full set only when a candidate with the same tree went green.
- **Evidence against:** it still runs the full set on every merge that is not tree-equal to a full-set candidate, and every merge waits for it.
- **Cost:** *(estimate)* already written.
- **Precedent:** ADR-0345.

### Option C: the full set on every push to main (the baseline before ADR-0345)
- **Description:** what ran until 2026-11-15.
- **Evidence against:** the problem of the Context.
- **Cost:** *(estimate)* one full run a merge.
- **Precedent:** the baseline.

## Recommendation

Option A, as decided. This section is not a recommendation to the architect: the decision is below and may not be reworded.

## Consequences

1. Implemented here: a push to `work/*` or any non-`main` branch runs the quick checks only (the plan job is skipped, so no build and no test); a push to `main` is planned by `PUSH_MAIN=1 tools/ci-plan.sh` (the document checks only, whatever changed); the concurrency group has one group a kind of event, cron and ref, and cancels only a branch, pull-request or tag run, never a run of `main` (a push, a scheduled or a manual run); `schedule` has the nightly cron (`17 2 * * *`, always, with the nightly's own jobs) and `23 */4 * * *` (an off minute; the full set if `tools/ci-last-full.rb` finds that `main` changed since the latest scheduled run that ran the build, else nothing); a scheduled run's summary lists the commits on `main`'s first-parent line since the last green full run (`git log --first-parent`, merges marked; the 15 latest commits if the last green run is unknown); an API error means the full set runs. A 4-hourly run that skips because the latest full run on this commit was red fails (the plan step exits 1 with the reason), so that a skipping run never reads as green over a red full run. The plan step is `tools/ci-plan-step.sh`, tested by `make check-ci-plan`; `make check-ci-jobs` compares the workflow's nightly cron with its `NIGHTLY_CRON`. `tools/ci-last-full.rb --green-for SHA` answers the stage-tag rule: the latest scheduled run whose `build` job ran, that went green and whose head has `SHA` as an ancestor; a skipped or red run never counts.
2. A candidate runs the steps `tools/ci-plan.sh` selects, and the full set only when the diff touches `kernel/`, libposix (`crates/zoraya-posix*`, which holds its ffi, and `ports/musl/`) or `tools/test-runner/`. A change of the `Makefile` runs the steps whose recipes or variables the diff changes (`tools/ci-makefile-steps.rb`: a changed recipe line belongs to its target, a changed assignment to its variable, steps are found through prerequisites, `$(MAKE)` calls and variable references, transitively); a change it cannot attribute (a `.PHONY` line, an `include`, an `export` line or an exported variable, `SHELL` and `MAKEFLAGS`, a conditional, a `$(eval` or a variable it uses, any line that is no assignment, rule or recipe line, an unchanged line that changed owner such as a deleted rule header or a removed trailing backslash) runs the full set, and so does a changed rule or variable that no step, `test-unit`, `ci-build` or listed non-step root (`check-*`, `run*`, `mutants*`, ...) reaches: "reaches no step" is not taken as "affects nothing". A recipe or prerequisite word the parser cannot resolve (`$(MAKE) $(2)`, `$(foreach ...)`) stands for every rule. `tools/test-map.txt` stays the path map; its `Makefile` line is the fallback when the parser cannot run. `make test-changed` uses the same mapping.
3. The `candidate-full/NAME` tag and a manual run with `full=true` stay available (to chase a flake, by hand); they are no longer a step of the stage's close.
4. ADR-0345 is superseded in part by this decision: its rule that a push to `main` is planned from a tree proof no longer applies, since a push to `main` runs the document checks only. Its proof tool (`tools/ci-tree-proof.rb`) and the fixtures of `make check-ci-plan` stay, as a way to show that a commit's tree has a green full run. The architect sets the status of ADR-0345.
5. A merge is no longer slowed by `main`'s run, so a merge can break `main` with no run in progress to say so; the scheduled run's list of merges and "a red scheduled run is fixed first" are what limit that.

Known limits: a `.PHONY` change always runs the full set (every new target is a `.PHONY` change; narrowing it to the words changed is a possible refinement); the parser expands simple variables and `addprefix` only, so an unresolvable word over-selects (every rule) instead of under-selecting; `$(MAKE) -f ports/Makefile T` is followed by name `T` only. A run of `main` is not cancelled while it runs; a group keeps one pending run, so a newer queued run of the same kind replaces an older queued one (harmless for pushes, whose later run checks a later tree).

## Invariants

- **INV-0348-1.** A push to `main` is planned as the document checks only (no tests, no build), whatever changed, and a scheduled run that finds nothing new on `main` since the last full scheduled run plans nothing. *Check:* `make check-ci-plan` (fixtures).
- **INV-0348-2.** A candidate's plan is the full set if and only if a changed path is under `kernel/`, `crates/zoraya-posix*`, `ports/musl/` or `tools/test-runner/`, or a change of the `Makefile` cannot be attributed to a rule or a variable; a one-line change of one recipe plans that recipe's step and the smoke set, and a variable used by two steps plans those two. *Check:* `make check-ci-plan` (fixtures of `tools/ci-makefile-steps.rb` and the plans).
- **INV-0348-3.** The workflow has the nightly and the 4-hourly schedule, a concurrency group per kind of event, cron and ref that does not cancel a running run of `main`, plans a push to `main` with `PUSH_MAIN=1`, and its scheduled run reads the last full run and lists the merges since the last green one. *Check:* `make check-ci-jobs`.

## Open questions

- Superseded by ADR-0463 (2026-11-16): the items about candidate runs (decision items 2 and 3: a candidate run before every merge, and the next part merging when its own candidate is green) and the 4-hourly schedule (now every two hours). The rest of this ADR stands.
- Whether a `.PHONY` change should map to the targets whose names were added or removed instead of the full set.
- Whether the scheduled run should open an issue or tag when it is red, instead of the list of merges in its summary only.

## Revisit triggers

- A merge that broke `main` and was found only by the scheduled run, more than once.
- A GitHub plan with more concurrency, or a faster runner, so that the full set takes minutes.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

CI is the brake on the work: today main ran two full sets for one commit, work/10c and work/f2 got full workflow_dispatch runs, and every merge waits for main's full run. New policy: (1) Pushes to work/* branches: make check only (no QEMU). No workflow_dispatch full runs, except to chase a flake. (2) Candidates: the steps tools/ci-plan.sh selects for the change, not the full set; the full set only when the change touches the kernel, libposix or tools/test-runner. A Makefile change maps to the steps whose recipes or variables the diff changes, not to every step. (3) After a merge, do not wait for main's run: the next part merges as soon as its own candidate is green. A push to main must not cancel a scheduled full run in progress. (4) main's full set runs on a schedule, every 4 hours if main changed since the last full run, plus the nightly; main's push run runs the document checks only. A red scheduled run is fixed first, before the next merge; the run lists the merges since the last green one. (5) A stage tag needs a green scheduled or nightly full run that includes the tagged commit.
