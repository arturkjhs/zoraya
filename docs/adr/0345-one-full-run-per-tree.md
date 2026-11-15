# ADR-0345: A tree that went green on the full set is not run on the full set again after its merge

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0213, ADR-0350
- **Evidence:** test-audit/report.md#5-time

## Context

CLAUDE.md, "Work discipline": a candidate runs the steps its change touches; the full set runs on `main` for every push that is not docs-only, nightly, on demand and on a `candidate-full/` tag before a stage tag. A part is therefore tested twice: as a candidate (often with the full set, for a stage's last part) and again on `main` after the merge, 66 to 73 jobs and 30 to 67 minutes, plus the queue (docs/handoff.md, section 1) (test-audit/report.md#5-time). When the merge adds nothing to what the candidate ran, the second run tests the same files.

The architect's order (2026-11-15): when the merge commit's tree equals the tree of a candidate that passed the full set, `main`'s push run may run the document checks only. The workflow must prove the equality itself (tree hashes, the candidate run's conclusion), not trust a tag name or a message. A stage tag then counts a full green run on a commit with the same tree.

A git tree is the content of every file at a commit: equal trees mean the same sources, the same `tools/ci-tests.txt`, the same `test.yml` and the same plan code, whatever the history. This is why the equality is a proof of what ran and the names of branches and tags are not needed.

## Decision drivers

1. Main's green means the full set passed on the content of `main` (CLAUDE.md, stage tags). Any shortcut must keep that true.
2. No trust: a name, a tag or a message can be written by anyone; a tree hash and the API's own conclusion and job list cannot be made up by the pusher.
3. Fail safe: any doubt, including an API error, runs the full set, as today.
4. Time and queue: a full run is about an hour of 70 jobs on a free plan shared by two machines.

## Considered options

### Option A: always run the full set on `main` (today)
- **Description:** every push to `main` that is not docs-only runs everything.
- **Evidence for:** simplest; `main` is tested after the merge itself.
- **Evidence against:** tests the same tree twice when the merge adds nothing (driver 4).
- **Cost:** *(estimate)* one full run a merge.
- **Precedent:** the baseline.

### Option B: trust a tag or branch name (`candidate-full/NAME` exists for the merged head)
- **Description:** skip when a tag names the head.
- **Evidence against:** violates driver 2: a tag is moved or pushed by hand, and says nothing of the run's result or of the tree after a later commit.
- **Cost:** *(estimate)* a few lines. **Precedent:** none worth following.

### Option C: prove the equality in the plan job (the recommendation)
- **Description:** in the `plan` job of a push to `main` that is not docs-only, `tools/ci-tree-proof.rb` (called by `tools/ci-plan.sh`) requires: (1) the pushed commit has exactly two parents and the first is the push's `before` (the previous head); (2) some run of `test.yml` (events `push`, `workflow_dispatch`, `schedule`, never `pull_request`, whose run tested the merge ref and not its `head_sha`; not this run; conclusion `success`) has a `head_sha` whose tree equals `git rev-parse HEAD^{tree}` (local `git` first, else the API's commit); the runs on the second parent are asked for by `head_sha` first, then the 100 latest successful runs are scanned; (3) that run's jobs, read through the API (all pages), include every job of the full set, computed from `git show SHA:tools/ci-tests.txt` and `git show SHA:.github/workflows/test.yml` (the tree's own files: every line of the list, `build` and every command of the checks matrix; an empty list or matrix proves nothing), each with conclusion `success` and its step `Run` (for `build`: `make ci-build`) `success`, so that a checks job whose step was skipped does not count; a name that occurs twice counts only if both are successful. A job missing or skipped (a candidate plan, a docs-only plan, a cancelled job) fails the proof. Each API call has a deadline (30 s) and the whole proof has one (120 s); a call that does not answer is an error. When proven the plan is `docs_only=1` (the document checks, no build, no tests) and the log names the run id and the tree; otherwise the full set. An API error, a deadline, a rate limit or a missing token gives the full set, never a plan job that hangs or fails.
- **Evidence for:** drivers 1 to 4. The trees equal means the proving run ran this run's workflow and plan code too.
- **Evidence against:** it needs `actions: read` on the plan job and the API; the merge has to follow the rule of Consequences 1.
- **Cost:** *(estimate)* one Ruby script of about 100 lines, a fixture test (`make check-ci-plan`), one permission.
- **Precedent:** the docs-only rule of `tools/plan-lib.sh`, which is also decided from what the commit contains.

### Option D: never run the full set on `main`, only on tags
- **Description:** the full set only before a stage tag.
- **Evidence against:** a red `main` is found late; a stage's last merge would carry the risk.
- **Cost:** *(estimate)* least CI. **Precedent:** none here.

## Recommendation

Option C. This is a recommendation, not a decision.

## Consequences

1. The merge must be tree-equal to the candidate: merge `origin/main` into the branch, push, run the full set on that head (`gh workflow run test.yml --ref BRANCH -f full=true`, or a tag `candidate-full/NAME`), then merge the branch into the same `origin/main` with `--no-ff` and nothing else (no recomputed table, no edit in the merge commit; recompute and commit such things on the branch before the full run). If `main` moved after the candidate, the merge tree differs: the full set runs. That is correct, not a failure.
2. A candidate that ran only what its change touches (the usual candidate) never proves anything: its job list is not the full set.
3. A stage tag counts a full green run on the tagged commit's nearest code ancestor, or on a commit with the same tree. The wording of CLAUDE.md, "Work discipline", must change, because "the latest ancestor commit whose run was the full set" is no longer the latest ancestor that changed code: a proven merge ran the document checks only, and a merge whose plan job failed or was cancelled has no full-set run either, so a search by run kind would skip such a merge and land on an older green commit, and a docs-only commit after it would carry the tag over code that never passed (the review of this part, finding 1). Proposed wording (the architect amends CLAUDE.md; an agent does not): replace "A stage tag still needs a full green run on the tagged commit or on its nearest code ancestor (the latest ancestor commit whose run was the full set)." with "A stage tag still needs a full green run on the code commit of the tagged commit: the tagged commit itself if its change from its first parent is not docs-only (`tools/plan-lib.sh`'s `docs_only`, which judges the whole change, so a posix-surface block or a doc a test reads makes it code although it is under `docs/`), else the nearest such commit on its first-parent chain (branch commits and a merge's second parent do not count; a merge is judged by its change from its first parent). The run counts if it is on that commit or on a commit with the same tree (`tools/ci-tree-proof.rb`; the summary of a proven `main` run names the run, and the tag check reads that run again)." And in the sentence on what a push to main runs, add: "A push to main whose merge commit has the tree of a green full-set run (two parents, the first the previous head; proved by `tools/ci-tree-proof.rb`) runs the document checks only."
4. The first push to `main` after this change shows whether job names and API fields are what the script expects: a green proof is logged with the run id; if the log says "not proven" for a merge that should have been, the full set ran (safe) and the reason is in the plan step's log.

Weak points, stated plainly:
- The runs of the second parent are found by `head_sha` however old; only a proof that has to be found by the scan (a by-hand lookup of a commit that is not a parent) needs the run to be among the 100 latest successful runs (about 17 hours on the real repository), else the lookup fails safe.
- A re-run of a single job (`run_attempt` 2) counts as the run's latest attempt; the API lists the latest jobs. A person who re-runs a failed job to green makes the run green: the same rule as a stage tag on a re-run today.
- A run of the full set whose jobs passed but whose tests did not exercise something (a flaky skip inside a step) is as good as it is for any green run.
- The token's `actions: read` reaches the runs of this repository only. A fork's token or a pull request has no proof: it runs the plan of a candidate anyway.
- Anyone who can push a tree and make the full set go green can make the proof; that is the same trust as CI itself.
- Same tree means the same code, not the same environment: the CI image under a tag can be rebuilt, the candidate may have run on the bare runner and `main` in the image, the action tags and the runner image move, caches come from other scopes (and a proven merge no longer warms `main`'s caches, so candidates may build cold more often), certificates age. The nightly is what tests `main` in today's environment.
- The proof is a snapshot: a proving run re-run later to red or deleted leaves `main`'s run green; the summary names the run, and a stage tag check reads it again.
- The two-parent and first-parent checks are policy, not soundness: with equal trees any second parent is the tested code; they only restrict when the shortcut is offered. A fast-forward of the candidate's own commit gets the full set.
- Trust in the job names: a matrix job is named by its command; a change of `name:` in `test.yml` changes the tree, and a run of the old workflow does not prove the new one (the equal tree includes the workflow file).
- Git object lookups: an old candidate's commit gone from the checkout (a deleted branch) is read through the API; a commit the API does not know ends the proof of that run.

## Invariants

- **INV-0345-1.** A push to `main` is planned as document checks only on the proof of a tree, and only if: the commit is a merge of exactly two parents whose first is the previous head, and a run of `test.yml` of the event `push`, `workflow_dispatch` or `schedule` other than itself concluded `success` on a commit with the same tree and has every job of the full set with conclusion `success` and its `Run` step (`make ci-build` for `build`) `success`. A different tree, a missing or skipped job or step, a duplicate name with a failure, a failed run, a `pull_request` or unknown event, a different first parent, a one-parent commit, the run itself, an empty list, a second page of jobs not read, and any API error or deadline give the full set. *Check:* `make check-ci-plan` (fixtures).
- **INV-0345-2.** The plan job of `test.yml` declares `actions: read`, and the proof reads the names from `tools/ci-tests.txt`, `build` and the checks matrix of the workflow at the commit asked about, not from a list of its own. *Check:* `make check-ci-plan` (the fixtures take the run's jobs from the base commit's files, so a list line or a matrix entry of the tested tree that the run lacks is not proven; it fails if the plan job lacks the permission).

## Open questions

- Whether `main`'s run should also upload the proof (run id, tree) as an artifact for the stage tag's record: the log line is enough for now.
- Whether a candidate's check job names should be compared with a reproducible list from the run's own commit through the API instead of the checkout (equal trees make them the same files; the check is redundant while the rule holds).
- A reusable `workflow_dispatch` input to run a candidate as "full" without the tag.

## Revisit triggers

- A proof that was wrong: a `main` run that skipped the full set and a later full run on the same tree failed.
- The API's job list or names change, or a GitHub plan limit makes the lookup fail routinely.
- A second workflow whose result counts toward main's green.

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
