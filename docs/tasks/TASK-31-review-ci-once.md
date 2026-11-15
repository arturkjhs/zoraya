# TASK-31: review of part `ci-once`, one full run per tree (branch `work/ci-once`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/ci-once` at `046078e`: `tools/ci-tree-proof.rb`, `tools/ci-plan.sh`,
`.github/workflows/test.yml`, `tools/check-ci-plan.sh`, `tools/test-map.txt`, `docs/design.md`, `docs/adr/invariants.md`,
`docs/adr/index.md` and the proposed ADR-0345. Read against CLAUDE.md ("Work discipline": candidates, the full set, stage
tags, the nightly), the architect's order quoted in `docs/tasks/TASK-31-part-ci-once.md` (a merge commit whose tree equals
the tree of a candidate that passed the FULL set may run the document checks only; the workflow proves the equality
itself; fail safe to the full set) and the rest of `test.yml` and `ci-image.yml`. The question asked: can `main` be
reported green without the full set having passed on its content?

Severities: **must-fix** (a false "proven" is possible, or a binding text would allow one), **should-fix** (the fail-safe
or a claimed check does not hold as stated; no false proof today), **note**.

## What was run

| Command | Result |
|---|---|
| `make check` (worktree of `work/ci-once`) | all checks pass |
| `make check-ci-plan` | "the docs-only rule and the tree-proof rule hold" |
| `MERGE=0 SHA=85b4bdad76 tools/ci-tree-proof.rb`, real API (read-only `gh`), `LIST`/`WORKFLOW` taken from that commit | proven by run 43649520010, all 71 jobs |
| the same for `23b338db0e` | proven by run 43645322051, 71 jobs |
| the same for `f3da42ce96` (a `candidate/` run) | not proven: run 43662980961 lacks only `make test-unit` (its plan had `UNIT_ARGS`); run 43662966697 (branch push) lacks 48 |
| the same with an empty `LIST` file (a slip of the reviewer's shell) | proven with **24** jobs (`build` and the checks): see finding 6 |
| `gh api .../actions/workflows/test.yml/runs?per_page=100&status=success` | 100 runs from 2026-11-14T09:13Z to 2026-11-15T02:38Z (17.5 h), all `push` |
| `CI_API` answering malformed JSON; runs without `head_sha`; no `GITHUB_REPOSITORY`; no token | each: exit 1, "not proven" |
| `CI_API` that sleeps 30 s | the script waits the whole 30 s (no timeout of its own): finding 2 |

Hand breaks of `tools/ci-tree-proof.rb` (and one of `test.yml`), each applied alone in a scratch copy, then
`sh tools/check-ci-plan.sh`, then reverted:

| Break | Result |
|---|---|
| a skipped job counts as success | killed ("same tree, a job skipped") |
| the tree comparison removed (`next unless t == tree`) | killed ("a different tree", and 3 more) |
| the first-parent check removed | killed ("first parent is not the previous head") |
| the two-parents check removed | killed ("a one-parent commit is proven") |
| the run's conclusion not checked | killed ("the run failed") |
| the run itself not excluded | killed ("a run proves itself") |
| `build` not expected | killed ("the build failed") |
| the lines of `ci-tests.txt` not expected | killed ("a job missing") |
| `MERGE` required to be `1` for the merge check | killed |
| `actions: read` removed from the plan job | killed |
| **the event filter removed (a `pull_request` run counts)** | **survives**: finding 3 |
| **the checks matrix not expected** | **survives**: finding 5 |
| **the jobs read from the first page only** | **survives**: finding 8 |
| **of two jobs of one name, the first conclusion wins** | **survives**: finding 8 |

## Findings

### 1. must-fix: the proposed stage-tag wording keeps a definition of "code ancestor" that this change makes wrong

*Where:* `docs/adr/0345-one-full-run-per-tree.md`, Consequences 3; CLAUDE.md "Work discipline" (the sentence it amends);
`docs/design.md` (the CI section's last sentence on stage tags).

*What:* CLAUDE.md defines the nearest code ancestor as "the latest ancestor commit whose run was the full set". Until
now every push to `main` that changed code ran the full set, so "whose run was the full set" and "that changed code"
were the same commit. After this change a code-changing merge can have a run that is not the full set (a proven merge,
docs only), and so can a merge whose plan job failed (finding 2) or whose run was cancelled. The proposed wording only
appends a sentence; the parenthetical still lets the search skip such a merge and land on an older commit.

*Scenario:* `main` is P (full, green). Part X is merged as M; M's plan step hangs on the API and the plan job times out,
so M's run is red with no build and no tests (not "the full set"). A docs-only commit D follows (docs-only run, green).
The stage tag goes on D: the latest ancestor "whose run was the full set" is P, green, so the rule as worded accepts the
tag. X's code has never passed the full set on `main`'s content, nor been proven by a tree.

*Fix:* redefine, in the wording handed to the architect: "its nearest code ancestor (the latest ancestor commit that
changed a path outside what `tools/plan-lib.sh`'s `docs_only` allows) must have a green run of the full set, on itself or
on a commit with the same tree (`tools/ci-tree-proof.rb`, the run id that `main`'s run summary names)". The same
sentence in `docs/design.md`. The orchestrator's tag check should follow the new definition, not the run list.

### 2. should-fix: an API call that hangs leaves the plan job to time out; the run goes red, not to the full set

*Where:* `tools/ci-tree-proof.rb:39-45` (`api`), `.github/workflows/test.yml:100` (`timeout-minutes: 10` on the plan job,
which also installs the toolchain).

*What:* `api` waits for `gh api` without limit (measured: a 30 s stand-in held the script 30 s). `gh` has no default
overall timeout. If GitHub's API stalls, the plan job is cancelled at 10 minutes; `build` and `tests` are skipped (their
`if` gets an implicit `success()`), `checks` is skipped (`needs.plan.result` is neither `success` nor `skipped`), and the
run is red. That is not a false green, but it is not the order's "fail safe to the full set" either, and it creates the
code commit with no full run that finding 1 exploits.

*Fix:* a deadline per call and for the whole proof (e.g. 30 s a call, 120 s in all) that kills the child and returns
nil (Ruby: `Process.spawn` plus `Process.wait` in a `Timeout` block, or `IO.select` on the pipe; `timeout(1)` exists on the
runner but not on a Mac). A fixture with a `CI_API` that sleeps past the deadline, expecting `docs_only=0`.

### 3. should-fix: the event filter is untested, and it is what keeps a pull request's run from proving a tree it did not test

*Where:* `tools/ci-tree-proof.rb:87`; `tools/check-ci-plan.sh` (every fixture run has `"event":"push"`).

*What:* for `pull_request`, the run's `head_sha` is the pull request's head, but the code checked out and tested is
`refs/pull/N/merge` (the head merged into the base at that moment): a different tree. The filter excludes
`pull_request`, correctly, but removing it survives every fixture. Today a second barrier exists (a pull request is
planned as a candidate, and a candidate's job names fall short: fuzz at 60 s, the smoke GUI run), but a candidate plan
can equal the full set's names (an unmapped path widens it to everything, and a change mapped to the fuzz steps keeps
600 s; run 43662980961 lacked one job only).

*Scenario:* a later edit drops the filter (or adds `pull_request` "for more proofs"). Head H contains `main` at T0;
`main` gains commit Y; a pull request of H, planned as everything, goes green on `H merged with T0+Y`. Y is reverted on
`main`, so `main`'s content is T0's again; H is merged `--no-ff`: the merge's tree is H's tree, the PR run's `head_sha` is
H, and the merge is "proven" by a run that tested H with Y, never H alone.

*Fix:* a fixture with `"event":"pull_request"` and every job, expecting `docs_only=0`; and one with an unknown event.

### 4. should-fix: a checks job's `success` does not show that its command ran

*Where:* `.github/workflows/test.yml:334-348` (`RUN_IT`), `tools/ci-tree-proof.rb:96-104`.

*What:* in a docs-only plan every checks job whose command is not in `doc_checks` concludes `success` with its steps
skipped. The proof counts it as a passed check. It is sound today only because `docs_only=1` always comes with
`tests=[]`, so such a run lacks the test jobs. That coupling is in nobody's invariant: a later plan that skips a check's
step while running the tests (a candidate that gates checks by the map, say) would make a run prove checks that never
ran.

*Fix:* read each job's `steps` (the jobs API returns them) and require the step named `Run` to be `success` for the
checks and tests jobs (and `make ci-build` for `build`); a fixture whose checks job has that step `skipped`. Or state the
coupling as an invariant of ADR-0345 and check it in `make check-ci-plan`.

### 5. should-fix: INV-0345-2's check does not check what it says

*Where:* `docs/adr/invariants.md` (INV-0345-2: "it fails if ... the proof reads names ... from a list of its own"),
`tools/check-ci-plan.sh` (`all_jobs`).

*What:* the fixtures build the run's job list from the same files and by the same pattern as the proof, so a proof that
drops the checks matrix from what it expects (break above) or uses a hard-coded copy of today's names passes. The rows'
Status says `checked: make check-ci-plan`, which is not true for this half.

*Fix:* in the scratch repository add a line to `tools/ci-tests.txt` and a `- make check-x` entry to the copied
`test.yml` that the fixture's run does not have, each expecting `docs_only=0`; then the derivation is tested.

### 6. should-fix: the by-hand lookup reads the job list from the checkout, not from the commit it is asked about

*Where:* `tools/ci-tree-proof.rb:54-55, 66-76`; `docs/design.md` ("the lookup by hand: `MERGE=0 SHA=<commit> ...`").

*What:* `LIST` and `WORKFLOW` default to the files of the working tree. In CI the checkout is `SHA`, so they agree. By
hand, for a stage tag, the checkout is usually a later `main`: the expected names are that commit's, not the tagged
tree's. A list that lost or renamed a line since then, or an unreadable/empty list (observed: an empty `LIST` gave a
"proof" with 24 jobs; the guard `expected.size < 3` does not catch it), gives a proof of a run that lacked jobs of the
tagged tree's full set.

*Fix:* read both files with `git show SHA:tools/ci-tests.txt` and `git show SHA:.github/workflows/test.yml` (the
environment variables only for the fixtures), and require at least one line from the list.

### 7. should-fix: the run window is 17.5 hours; a merge or a tag check after that always misses

*Where:* `tools/ci-tree-proof.rb:79` (`per_page=100&status=success`, one page).

*What:* measured on the real API: the 100 latest successful runs of `test.yml` span 17.5 hours, every branch push
counting. A candidate-full run on an evening, merged the next morning, is out of the window: the full set runs (safe,
but the saving is lost). The by-hand lookup of finding 6 fails for any stage tag older than that.

*Fix:* first query `runs?head_sha=<second parent>` (the API filters by `head_sha`), which is the candidate in the
documented flow; then the scan as now. For tags, look the run up by the id `main`'s summary names.

### 8. note: two paths of the jobs read have no fixture

The first-page-only and first-conclusion-wins breaks survive. A full run has 74 jobs today (71 counted plus `image` and
`plan`), so a second page is near; jobs of one name do not occur with `filter=latest`. Fixtures: a run whose jobs come in
two pages (`total_count` above 100), and one with a duplicate name, success then failure.

### 9. note: equal trees mean the same code, not the same environment

Checked: no submodules, no `.gitattributes`, no LFS; no make rule, tool or test reads `GITHUB_EVENT_NAME`, `GITHUB_REF`,
the commit message or history (only the plan does; `FULL` comes from `CI`, which every run has); the ports' archives are
checked against `ports/sources.lock`, crates against `Cargo.lock`, the toolchains are pinned. What the tree does not fix:
the image under a tag can be rebuilt and pushed again (`ci-image.yml:50`, any `workflow_dispatch`); the candidate may have
run on the bare runner and `main` in the image, or the other way (`image` job); the action tags (`@v4`) and the runner
image move; apt on the bare runner is unpinned; the candidate's caches come from its branch or tag scope, `main`'s from
`main`'s; time-dependent inputs (certificates) age. The skipped run on `main` was one more sample of the flaky tests too.
None of this makes the proof false about the code; ADR-0345's weak points should say "same tree, same code, not the same
environment; the nightly is what tests `main` in today's environment". Also: a proven merge does not save `main`'s caches
any more (only the nightly does), so candidates may build cold more often; worth measuring after a week.

### 10. note: the proof is a snapshot

A proving run re-run later to red, or deleted, leaves `main`'s run green. The summary names the run; a tag check should
re-read it at tag time (finding 1's fix does).

### 11. note: `MERGE=0` is a generic name

`MERGE` set to `0` anywhere in the plan step's environment turns off the two-parents and first-parent checks. Nothing sets
it today; a specific name (`CI_TREE_PROOF_ANY_COMMIT=1`) avoids an accident.

### 12. note: the merge checks are policy, not soundness

With equal trees an arbitrary second parent, or a merge that took the branch's tree whole (`-s ours` the other way), is
still the tested code; the two-parents and first-parent checks only restrict when the shortcut is offered. A fast-forward
push of the candidate's own commit (the safest case) gets the full set. Fine, but the ADR could say so.

## Checked and found sound

- Same tree implies the same `test.yml`, `ci-tests.txt`, `ci-plan.sh` and `plan-lib.sh`; for `push`, `workflow_dispatch`
  and `schedule` the workflow file is the one at `head_sha`, so an older workflow at the same tree is the same workflow.
- A candidate plan cannot pass by accident: it differs in names (fuzz at 60 s, the smoke GUI and POSIX lines, `test-unit`
  with `UNIT_ARGS`), confirmed on run 43662980961; when it equals the full set by names, it ran the full set.
- `full=false` manual runs, branch pushes (no plan job) and docs-only runs lack the test jobs and are rejected.
- Re-runs: the run's conclusion and the jobs (`filter=latest`, the default) are the latest attempt; the same rule as a
  stage tag on a re-run today.
- The tree of an unknown head comes from the API's commit object, not from anything the pusher writes.
- Every error path tried (no repository, no token, malformed JSON, missing fields, a failing jobs or runs call, a Ruby
  exception) exits 1 and `ci-plan.sh` emits the full set; the proof's output is printed only on success.
- The plan job's permissions are `contents: read`, `actions: read`; no write. `GH_TOKEN` reaches the plan step for every
  event, a read-only token, harmless for a fork's pull request.
- `workflow_dispatch` with `full=true`, the nightly and `candidate-full/` take the `FULL=1` path, untouched. `make check`
  (`check-workflow-syntax` among them) passes.
- The real API's job names equal the commands (two `main` runs proven with all 71 jobs).

## Recommendation

No way to a false "proven" was found on CI's own path. Merge after finding 2 (a deadline, so the fail-safe is the full
set) and the fixtures of findings 3 to 5; finding 1 must be fixed in the wording before the architect adopts it into
CLAUDE.md, and the orchestrator's tag check must follow it. Findings 6 and 7 before anyone relies on the by-hand lookup
for a stage tag.
