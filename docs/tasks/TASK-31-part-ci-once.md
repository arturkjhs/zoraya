# TASK-31 part `ci-once`: a tested tree is not tested again on `main`

Branch `work/ci-once` (from `origin/main` 3292aa2). Order of the architect (2026-11-15): a part is tested twice, as a
candidate and again on `main` after the merge. When the merge commit's tree equals the tree of a candidate that passed
the FULL set (merge `origin/main` into the branch, candidate, then `--no-ff` merge with nothing else), `main`'s push run
may run the document checks only; the workflow proves the equality itself; a stage tag counts a full green run on a
commit with the same tree.

## What was done

1. `tools/ci-tree-proof.rb` (Ruby, no gems, 2.6 and 3.2): the commit is a merge of exactly two parents whose first is
   `BEFORE`; a run of `test.yml` (events push, workflow_dispatch, schedule; conclusion success; not the run itself)
   whose `head_sha` has the same tree (git first, else `git/commits/SHA` through the API); that run's jobs (API,
   paged) hold every line of `tools/ci-tests.txt`, `build` and every command of the checks matrix, each `success`.
   Any error or doubt: exit 1, the full set.
2. `tools/ci-plan.sh`: in the push-to-main branch, when `docs_only` fails the proof is tried; proven gives
   `docs_only=1`, `tests=[]`, `build=false`, the document checks and a summary naming the run and the tree.
3. `test.yml`: the plan job has `permissions: actions: read`, and `GH_TOKEN`, `RUN_ID` in the plan step.
4. `tools/check-ci-plan.sh` (`make check-ci-plan`): fixtures for same tree and all jobs (docs only), a job missing, a
   job skipped, a failed build job, a failed run, a different tree, an API error (jobs, runs), an unknown head whose tree
   comes from the API (same, different, error), the run itself, a one-parent commit, a different first parent, main
   moved; and the plan job's permission.
5. `docs/design.md` (CI section), `docs/adr/0345-one-full-run-per-tree.md` (proposed; index and invariant rows;
   proposed CLAUDE.md wording for the architect; weak points). CLAUDE.md is not edited.

## Review fixes (docs/tasks/TASK-31-review-ci-once.md)

1. The proposed CLAUDE.md wording defines the nearest code ancestor by the paths it changes (ADR-0345 Consequences 3, design.md).
2. A deadline per API call (30 s) and for the proof (120 s); an unanswered call is an error: the full set. Fixture with a slow API.
3. Fixtures for `pull_request`, an unknown event, `workflow_dispatch` and `schedule`.
4. The jobs' `Run` step (`make ci-build` for build) must be `success`; fixtures with each skipped.
5. The fixtures take the run's jobs from the base commit's files; a tree with a list line or a matrix entry the run lacks is not proven; an empty list is not proven.
6. The expected names come from `git show SHA:tools/ci-tests.txt` and `SHA:.github/workflows/test.yml`; the by-hand lookup is `CI_TREE_PROOF_ANY_COMMIT=1` (renamed from `MERGE=0`).
7. Runs on the second parent and on the commit are asked for by `head_sha` first, then the scan of the 100 latest. Fixtures: found by `head_sha` with the scan failing, two pages of jobs, duplicate job names.
8. origin/main merged (invariants Total 793). Mutation sanity: seven breaks (event filter, checks names, one page, first conclusion wins, deadline, step check, head_sha query) each fail `make check-ci-plan`.

## Second fix pass (the check of the fixes)

13. The fake API and the fixtures live outside the scratch repository (`$tmp/aux`; the repository is `$tmp/repo`), so no commit carries them and no checkout removes them. Every negative fixture names the reason the proof must print (`first: make check-extra`, `the full list is empty`, ...) and fails if it was not proven for another reason.
14. `by_hand` fixtures (`CI_TREE_PROOF_ANY_COMMIT=1`, the checkout at the base commit): the names come from `git show SHA:` for the list and for the workflow.
15. `capture` also bounds the wait for the exit after EOF (`WNOHANG` polling, then the group is killed); fixture `SLOW=close`.
16. `SLOW=hold` sleeps 60 s, the run must end in 10 s, and a grandchild of the call's group that would touch a marker after 3 s must have been killed.
17. Stage-tag wording: the tagged commit itself, else the nearest code commit on its first-parent chain; `docs_only` judges the whole change (ADR-0345, design.md).
Breaks re-run, each failing `make check-ci-plan`: names read from the checkout (list and workflow), the matrix not expected, the empty-list guard removed, the per-call deadline removed, the group kill removed, the wait after EOF unbounded.

## Rule for merging (what the workflow now rewards)

Merge `origin/main` into the branch, commit everything (the invariants Total too), run the full set on that head
(`gh workflow run test.yml --ref BRANCH -f full=true` or `candidate-full/NAME`), then merge `--no-ff` at the same
`origin/main` with nothing else, push `HEAD:main`. If `main` moved, the trees differ and the full set runs.

## Not verified here

The real API's answers were checked against run 43649520010's job names (the names equal the commands) but the
proof has not run on a real push to `main`: the first merge after this shows it (the plan step's log).
