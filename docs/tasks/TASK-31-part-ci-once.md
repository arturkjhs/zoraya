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

## Rule for merging (what the workflow now rewards)

Merge `origin/main` into the branch, commit everything (the invariants Total too), run the full set on that head
(`gh workflow run test.yml --ref BRANCH -f full=true` or `candidate-full/NAME`), then merge `--no-ff` at the same
`origin/main` with nothing else, push `HEAD:main`. If `main` moved, the trees differ and the full set runs.

## Not verified here

The real API's answers were checked against run 43649520010's job names (the names equal the commands) but the
proof has not run on a real push to `main`: the first merge after this shows it (the plan step's log).
