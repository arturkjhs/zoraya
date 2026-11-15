# TASK-31 part `work/ci-policy`: the CI policy of 2026-11-15

Branch `work/ci-policy`. The architect's decision is recorded in ADR-0348 ("Architect's decision", verbatim) and in
`CLAUDE.md` ("Work discipline", "Git").

## What changed

- `docs/adr/0348-ci-policy-the-full-set-on-a-clock.md` (accepted, the architect's text), the index row, three invariants
  (total 803). ADR-0345 got an open-question note saying it is superseded in part; its status is not set.
- `.github/workflows/test.yml`: the concurrency group is per event kind, cron and ref and cancels only branch, pull-request
  and tag runs; a second cron `0 */4 * * *`; a push to `main` is planned with `PUSH_MAIN=1` (document checks only); a
  scheduled run asks `tools/ci-last-full.rb` for the last scheduled run that ran the build and plans nothing if it was on
  this commit (the nightly never skips); a scheduled run's summary lists the merges since the last green full run. A push
  to a non-main branch already ran no plan, no build and no tests (checks only): unchanged.
- `tools/ci-plan.sh`, `tools/plan-lib.sh`: `PUSH_MAIN=1`, `CI_PLAN_SKIP`, the full set for a candidate under `kernel/`,
  `crates/zoraya-posix*`, `ports/musl/`, `tools/test-runner/`; `docs_only` and the tree proof are no longer called.
- `tools/ci-makefile-steps.rb` (new): the Makefile mapping. `tools/ci-last-full.rb` (new): the scheduled lookup.
  `tools/test-changed.sh` uses the same mapping locally.
- `tools/check-ci-plan.sh` (fixtures), `tools/check-ci-jobs.sh` (workflow rules), `tools/test-map.txt`.
- `CLAUDE.md`, `docs/design.md` (CI), `docs/handoff.md` section 1, `docs/architect-queue.md` (TASK-34 note).

## The Makefile mapping

A changed recipe line belongs to its rule's target; a changed assignment (or a `define` block, with its continuation
lines) to its variable; a blank or comment line to nothing. A step is reached from a rule through its prerequisites
(variables expanded, `$(addprefix P,...)` and an unexpandable word standing for every target that begins with the text before
it, pattern rules), `$(MAKE) TARGET` in a recipe, and variables referenced in a prerequisite list, a recipe or another
variable (`$(V)`, `${V}`, `$(call V,..)`, transitively). Removed lines are attributed in the old Makefile, added lines in the
new one. `.PHONY`, `include`, `export`, a conditional or any other line: the full set. A change that reaches the recipe of
`test-unit` runs all host tests; one that reaches only `ci-build` runs the boot step (so the build job runs).

## Known limits and open risks

- A `.PHONY` change is always the full set, as ordered, so a new target is a full set (ADR-0348 open question).
- The parser expands only simple variables and `addprefix`; other functions (`$(if ..)`, `$(foreach ..)`) are not
  evaluated, so a target name made by them is found only by prefix.
- The new workflow and the lookup against the real API have not run (no dispatch was allowed): the first scheduled runs show
  whether `status=completed` runs and the `build` job name are what `tools/ci-last-full.rb` expects. Any error runs the full set.
- Two cron strings: the nightly is told from the 4-hourly by `github.event.schedule == '17 2 * * *'`; change one and the
  other, together (`make check-ci-jobs` checks both exist).
- A push to `main` no longer runs fmt, clippy or the code checks; candidates ran `make check`'s jobs on the branch.
