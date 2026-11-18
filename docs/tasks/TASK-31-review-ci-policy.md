# TASK-31: review of part `work/ci-policy`, the CI policy of ADR-0348 (branch `work/ci-policy` at `199d38b`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of `git diff 199d38b~1 199d38b`:
`tools/ci-makefile-steps.rb` (new), `tools/ci-last-full.rb` (new), `tools/ci-plan.sh`, `tools/plan-lib.sh`,
`tools/test-changed.sh`, `tools/check-ci-plan.sh`, `tools/check-ci-jobs.sh`, `tools/test-map.txt`,
`.github/workflows/test.yml`, `CLAUDE.md`, ADR-0348, the note on ADR-0345, `docs/adr/invariants.md`, `docs/design.md`,
`docs/handoff.md`, `docs/architect-queue.md` and the part's report (`docs/tasks/TASK-31-part-ci-policy.md`). Read against
ADR-0348's "Architect's decision" (items 1 to 5) and its drivers, ADR-0345 and ADR-0213. The architect asked for this review
of the mapping from a `Makefile` diff to steps in particular, so most of it covers that.

No workflow was dispatched and no QEMU was run.

## What was run

| Command | Result |
|---|---|
| `make check` (worktree of `work/ci-policy`) | all checks pass |
| `make check-ci-plan` | passes ("the plans, the Makefile mapping, the scheduled lookup and the tree proof hold") |
| `ruby tools/ci-makefile-steps.rb --files OLD NEW "$all"` on 26 small adversarial Makefiles (table under finding 2) and on edits of a copy of the real `Makefile` (findings 1, 2) | see the findings |
| GNU make 3.81 `make -n` on the fixtures whose make meaning was in doubt (f09, f15, f16, f22) | see finding 4 |

Hand breaks, each applied alone in a detached scratch copy of `199d38b` and reverted:

| Break | Result |
|---|---|
| B1. `.PHONY` and the other special targets no longer `:unknown` in `ci-makefile-steps.rb` | fails `check-ci-plan` (".PHONY: got 'steps:', expected 'full'") |
| B2. the closure no longer follows a variable to the variables its value references | fails `check-ci-plan` ("a variable used through another variable") |
| B3. `tools/test-runner/*` dropped from `ci-plan.sh`'s full-set case | fails `check-ci-plan` ("... tools/test-runner/src/main.rs runs the full set (got: candidate: 46 jobs)") |
| B4. `ci-last-full.rb` counts a run whose `build` was skipped as a full run | fails `check-ci-plan` (four lookup fixtures) |
| B5. the plan step's nightly test `[ "$SCHEDULE" = '17 2 * * *' ]` changed to `'17 3 * * *'`, the cron left as it is | **passes** `make check-ci-jobs` and `make check-ci-plan`: finding 6 |

## Findings

### 1. error: `$(MAKE) TARGET` is never followed, so a helper a step runs through make maps to no step and the candidate runs no machine

*Where:* `tools/ci-makefile-steps.rb:167` (`makes`); its use at `:205`; `tools/plan-lib.sh:44`.

*What:* the pattern is `(?:\$\(MAKE\)|\bmake)\b(...)`. The `\b` after `$(MAKE)` needs a word character next, and
`$(MAKE)` is always followed by a space, so `$(MAKE) --no-print-directory test-net-fetch` gives no match (checked:
`"\t$(MAKE) --no-print-directory test-net-fetch".scan(...)` is `[]`; `"$(MAKE)x"` matches). `${MAKE}` is not matched either.
Every `$(MAKE)` edge of the real `Makefile` is lost: `test-net` (lines 375 to 378) to `test-net-fetch`, `test-net-posix`,
`test-net-dns`, `test-net-lookup`; `ci-build` (1753, 1754) to `python-testlib`, `cxx-libcxx-tests`, `posix-image`; `test-posix`
(1380) to `posix-calls`; `test-libposix` (1066) to `check-libposix-widths-built`; `test-python-ext` (753) to `python`; and the
hand-run disk (935 to 942). `CONFINED` (460, 461) runs `$(MAKE) ... $(2)`, a `$(call)` argument, so `test-net-*-run` would not
be found even with the pattern fixed.

*Scenario (run on a copy of the real `Makefile`, one trailing blank added to one recipe line):*

| Rule whose recipe changed | Mapper's answer |
|---|---|
| `test-net-posix` (line 580) | `steps:` |
| `test-net-dns` (469), `test-net-lookup` (510) | `steps:` |
| `posix-calls` (1384) | `steps:` |
| `check-libposix-widths-built` (1681) | `steps:` |
| `test-net-fetch` (412) | `steps:` |
| `test-net-posix:` renamed `test-net-posix2:`, the caller at 376 left alone | `steps:` |

`steps:` with nothing after it sets `found=" check "` (`plan-lib.sh:44`). The candidate's selection is then empty, and the
smoke set is added only "if anything runs" (`ci-plan.sh`), so the candidate runs `check` and no machine at all. The
rename breaks `test-net` ("No rule to make target test-net-posix"). The candidate is green and the merge is allowed. The
push to `main` runs the document checks only. The break is first seen at the next 4-hourly run. The old map's line
(`Makefile` to every step) would have run `test-net`.

`docs/design.md` and the part's report say steps are reached "through ... `$(MAKE)` calls". That is not true of the
code, and no fixture of `check-ci-plan` has a `$(MAKE)` edge.

*Fix:* match `\$[({]MAKE[)}]` (with an optional `+`/`@`/`-` before it), and treat any make word the tool cannot resolve
(`$(2)`, a `$$` word with an empty prefix, a `$(...)` word) as "every rule" or `full`, not as nothing. Add fixtures: a
`$(MAKE) helper` edge, a `${MAKE}` edge, a `$(call F,target)` that runs `$(MAKE) $(1)`, and a rename whose caller still
names the old target. Each must plan the calling step.

### 2. error: "reaches no step" is taken as proof that nothing is affected; it only shows what the parser cannot see

*Where:* `tools/ci-makefile-steps.rb:157-162` (`refs`), `:181-185` (`rules_for` returns `[]` for a word with an empty
prefix), `:254-272` (`main`: a node no closure reaches is dropped); the fixture "a recipe no step reaches" `steps:` in
`tools/check-ci-plan.sh:119`; ADR-0348 "Known limits"; INV-0348-2.

*What:* ADR-0348's driver 3 is "any doubt ... runs the full set", and INV-0348-2 says the plan is full whenever a change
of the `Makefile` cannot be attributed. The tool does attribute every one of the changes below to a rule or a variable.
It then fails to find the steps that use that rule or variable, and answers `steps:`, which means no machine runs.
The ADR's known limit says such a variable "reaches the steps its other references reach". When there are none, it
reaches nothing, and nothing runs.

Adversarial fixtures (two steps, `test-a test-b`; every one is a real change of what `test-a` does):

| Case | Change | Answer | Should be |
|---|---|---|---|
| f01 | `helper`'s recipe; `test-a` runs `$(MAKE) --no-print-directory helper` | `steps:` | `test-a` |
| f01b | the same through `+${MAKE} helper` | `steps:` | `test-a` |
| f02 | `X`, used only as `$(X:.c=.o)` (a substitution reference) | `steps:` | `test-a` |
| f03 | `export FOO := 1` to `2`, read by the recipe's script from the environment | `steps:` | `full` (finding 3) |
| f04 | `FOO := 1` to `2` with an unchanged `export FOO` | `steps:` | `full` |
| f05 | `SHELL := /bin/bash` added | `steps:` | `full` (every recipe) |
| f05b | `MAKEFLAGS += -r` to `-R` | `steps:` | `full` |
| f06 | `helper-x`'s recipe; `test-a: $(foreach s,$(L),helper-$(s))` | `steps:` | `test-a` |
| f07 | the same through `$(patsubst %,helper-%,$(L))` | `steps:` | `test-a` |
| f24 | the same through `$(if $(CI),helper,)` | `steps:` | `test-a` |
| f08 | the body of `define TPL` that `$(eval $(call TPL,...))` turns into the steps' rules | `steps:` | `test-a test-b` |
| f12 | `a_FLAGS`, used as `$($(S)_FLAGS)` | `steps:` | `test-a` |
| f14 | `GEN`, used only in a rule's target list (`$(GEN):`), so `test-a`'s prerequisite loses its rule | `steps:` | `test-a` |
| f21 | `$(MAKE) -f ports/Makefile helper` (documented: followed by name) | `steps:` | `test-a` (finding 1) |
| f15, f16, f09, f22 | see finding 4 | | |

The constructs it gets right: a recipe edited inside a `define` used through `$(call R)` (f10: `test-a`); a continuation
line of a recipe (f11: `test-a`); a target-specific variable (f13); a static pattern rule (f17); a pattern rule (f18);
two targets on one rule (f19: both); a changed `ifeq` (f20: `full`); `.PHONY` (f23: `full`).

On the real `Makefile`, `SHELL := /bin/bash -o pipefail` added at the top answers `steps:`. Nothing in the real file
uses f02 or f06 yet (`RUN_BOOT` is used both through `$(RUN_BOOT:...)` and plainly, so it does map to `test-boot`). One
helper rule of a step being named through `$(foreach)` is enough to make them live.

*Fix:* turn the absence into a positive proof. A changed node that reaches no step, no `test-unit` and no `ci-build` is
`full` unless it is reached from a listed non-step root. That list (`check`, `check-*`, `fmt-check`, `clippy`, `adr-check`,
`mutants*`, `run*`, `debug`, `clean*`, `measure-*`, `bench-*`, `hand-*`, `print-*`) lives in the tool and is checked
against `.PHONY` by `check-ci-plan`. Then each new blind spot falls on the safe side. In addition:
- `rules_for` should give every rule for an unresolvable word with an empty prefix (`$(foreach ...)`, `$(patsubst ...)`,
  `$(if ...)`), as it already does for `$(addprefix ...)`.
- `refs` and `expand` should read `$(NAME:...)` as a reference to `NAME`.
- A change of `SHELL`, `.SHELLFLAGS`, `MAKEFLAGS`, `GNUMAKEFLAGS`, `MAKEFILES`, `VPATH`, `MAKESHELL`, of a variable named
  in any `export`/`unexport` line, or of a line with `$(eval` should give `full`.
- Replace the fixture "a recipe no step reaches" `steps:` with one whose recipe is reached only from a listed non-step
  root, and add f01, f02, f05 and f06 as fixtures that must not answer `steps:`.

### 3. warning: `export FOO := x` is attributed to `FOO`; ADR-0348 and `docs/design.md` say an `export` runs the full set

*Where:* `tools/ci-makefile-steps.rb:42` (`ASSIGN` accepts an `export`/`override` prefix) is tried at `:83`, before the
`export` test at `:87`. The fixture "an export" (`check-ci-plan.sh:126`) uses only the bare `export TIMEOUT` line.

*What:* an exported variable reaches recipes through the environment (`$$FOO` in a recipe, or any script or sub-make that
reads it). `refs` drops `$$` and never sees those uses. Fixtures f03 and f04 above answer `steps:`. ADR-0348 Consequence
2 ("an `export` ... runs the full set") and `docs/design.md` say otherwise. The real `Makefile` has no `export` today,
so the gap is latent. *Fix:* any line that begins with `export`/`unexport`, and any assignment to a variable an
`export` line names, gives `full`. Add a fixture `export FOO := 1` to `2`.

### 4. warning: only changed lines are attributed; an edit that moves unchanged lines to another owner is missed

*Where:* `tools/ci-makefile-steps.rb:229-245` (`main` looks only at the diff's lines).

*What:* make's meaning of an unchanged line can change with the line above it, and the tool never compares the owners
of unchanged lines between the two parses:
- f09: a rule header `helper:` deleted. In the new file (`make -n` checked) its recipe `rm -rf build` runs as part of
  `test-a`. The tool attributes the deleted header to the old `helper` and answers `steps: test-b`, and `test-a` is not
  run.
- f15: a trailing `\` removed from a comment. The next line `X = 1`, which make read as part of the comment, now takes
  effect (`make -n`: `echo ` before, `echo 1` after). Answer `steps:`.
- f16: a recipe line `\t# step \` continues into `\techo two`. The tool ignores the whole group as a comment, but make
  passes it to the shell and the shell runs `echo two` (`make -n` prints it). Answer `steps:`.
- f22: an assignment inserted between recipe lines. make itself fails ("commands commence before first target"), so
  `make check` catches it. Harmless.

*Fix:* map each unchanged line of the old file to its line in the new file (the hunks give the offsets), compare the two
owners, and answer `full` (or attribute to both owners) when they differ. Treat a `\t#` line that ends in a continuation
like a recipe line.

### 5. error: a scheduled run that skips is green, which hides a red full run and satisfies the stage-tag rule with nothing run

*Where:* `.github/workflows/test.yml:154-165` (the skip); `tools/ci-plan.sh` (`CI_PLAN_SKIP`: `tests=[]`, `build=false`,
`doc_checks=`); `CLAUDE.md` lines 42 to 44 and 105 to 110; `docs/architect-queue.md` (the command
`gh run list --workflow 'make test' --event schedule`); the lookup fixture `203 success aaaa skipped` in `check-ci-plan.sh`,
which models exactly such a run.

*What:* a 4-hourly run that finds `main` unchanged since the last full run plans nothing. Its `plan` job succeeds and
every other job is skipped, so the run's conclusion is `success`, on `main`'s head.

*Scenario:* the 00:00 full run on commit X is red. At 04:00 nothing has merged, the run skips, and it is green on X.
- `gh run list --workflow 'make test' --event schedule`, the command the policy gives for "a red scheduled run is fixed
  first", now shows a green run at the top. A dispatcher that reads the top line merges on a red `main`.
- CLAUDE.md's stage-tag rule ("a scheduled or nightly full run of `main` that includes the tagged commit ... is green")
  is met by the 04:00 run if one checks run conclusions, the only thing the documented command shows. X gets its stage
  tag on a red full set.

No tool implements the tag rule, and no documented command implements it either. The by-hand command in `design.md` is
ADR-0345's tree proof, which looks for any green full run on the same tree, and ADR-0348 does not ask for that. The rule
"includes the tagged commit" needs `git merge-base --is-ancestor TAG HEAD_SHA` plus a check that the run's `build` job ran.
Nothing gives both.

*Fix:*
- (a) When the lookup's latest full run is red and the run skips, make the plan job fail ("main is unchanged since the red
  full run N"), so the skip run carries the red forward. A skip after a green run stays green.
- (b) Add `tools/ci-last-full.rb --green-for SHA`. It answers the latest scheduled run whose `build` job ran, whose
  conclusion is `success`, and whose `head_sha` has SHA as an ancestor. Exit 1 if there is none.
- (c) Name `--green-for` in CLAUDE.md's tag rule and in the queue note, replacing `gh run list`.
- (d) Add fixtures for (a) and (b), among them a skip run on the tagged commit that must not count.

### 6. warning: the nightly is told from the 4-hourly by a copied string that no check compares with the cron

*Where:* `.github/workflows/test.yml:57` and `:154`; `tools/check-ci-jobs.sh` (it only greps that both cron lines exist).

*What:* break B5 changes the plan's test to `'17 3 * * *'` with the cron still `'17 2 * * *'`, and `make check-ci-jobs` and
`make check-ci-plan` both pass. The cron change the other way round is caught only because the cron line is grepped
literally. With the two strings apart, the nightly is planned as a 4-hourly: it skips when `main` did not change, and
`tools/ci-nightly-tests.txt` never runs again. Nothing reports this. *Fix:* in `check-ci-jobs.sh`, extract the first
cron string and require the plan step's comparison to use that string. Or define the nightly cron once (an `env:` of
the workflow) and compare `github.event.schedule` with it.

### 7. warning: `0 */4 * * *` is the busiest minute of GitHub's scheduler

*Where:* `.github/workflows/test.yml:61`.

*What:* GitHub's documentation for `schedule` says runs can be delayed during high load and that the start of every hour
is the heaviest time, when queued jobs may be dropped. A dropped 4-hourly tick delays the full set of `main` by another 4
hours, and the policy counts on it to find a bad merge. *Fix:* an off minute, e.g. `41 */4 * * *`, changed in the
workflow, `check-ci-jobs.sh`, `design.md` and ADR-0348's Consequence 1 together.
(Also a note: GitHub turns off the scheduled workflows of a public repository after 60 days without activity.)

### 8. warning: the blame list leaves out the commits made on `main` directly

*Where:* `tools/ci-last-full.rb:58` (`git log --merges --first-parent`).

*What:* `main` takes non-merge commits too: `8b09113` (the architect's decisions and CLAUDE.md) and `236feb1` (task
files) on this branch's own base. A direct commit that breaks something is missing from the list of a red run's
summary, so the list points only at the merges. *Fix:* list every first-parent commit
(`git log --first-parent --format='%h %s' LAST..HEAD`) and mark the merges. The 15-entry fallback should count commits,
not merges.

### 9. note: "a run of main is never cancelled" holds for running runs, not for pending ones

*Where:* `.github/workflows/test.yml:65-69`; the workflow's header comment; `CLAUDE.md`; `docs/handoff.md`.

*What:* a concurrency group keeps at most one pending run, whatever `cancel-in-progress` says. A newer queued run
cancels the older pending run. On `main`:
- Three merges in quick succession: the middle push's document-check run is cancelled. This is harmless, because the
  later run checks a later tree.
- A third `workflow_dispatch` (`full=true`) on `main` cancels the pending second one.

The architect's requirement, that a push to `main` must not cancel a scheduled run, holds, since push, schedule (per
cron) and dispatch are separate groups. Keeping the nightly and the 4-hourly in separate groups is right: in one group,
a pending 4-hourly could replace a pending nightly. The cost of separate groups is that the nightly and a 4-hourly can
run the full set on the same commit at the same time. The lookup reads only completed runs, so a slow 00:00 run and the
02:17 nightly both run. Reword the claim to "a run of `main` is never cancelled while it runs".

### 10. warning: INV-0345-1's Status column still says `checked`, but the workflow no longer applies the rule

*Where:* `docs/adr/invariants.md:635`; the note added to ADR-0345's open questions.

*What:* INV-0345-1 states when a push to `main` is planned as document checks only. Under ADR-0348 every push to `main`
is. The fixtures still check the proof tool on its own, but the invariant as written no longer holds of the workflow.
CLAUDE.md says to keep the Status column true. *Fix:* in the row's Status column, record that the push rule is not
applied since ADR-0348 and only the tool is checked, or put the row to the architect in the queue. ADR-0345 is still
`proposed`, and the architect sets its status. The note added to ADR-0345 is otherwise coherent.

### 11. note: what a push to `main` loses, beyond fmt and clippy

*Where:* `.github/workflows/test.yml:327-369` (`RUN_IT`), `tools/plan-lib.sh` (`doc_checks`).

*What:*
- A push to `main` runs 5 of the 25 check commands. It does not run `check-workflow-syntax`, `check-ci-jobs`,
  `check-ci-plan` or `check-test-lanes`. A merge that breaks `test.yml` is found when the next schedule fails to start
  ("workflow file issue"). That run has no plan summary and no list of merges.
- The merge commit itself is checked only by the local rule that `make check` exited 0 on the commit being pushed
  (CLAUDE.md, "Git"), including any conflict resolution in it.
- A skip run checks nothing at all (`docs_only=1`, `doc_checks` empty), which is right. A full scheduled run checks all
  25 commands (`docs_only=0`).
- The stated cost stands. Two parts merged back to back, each green against an older `main`, are first tested
  together by the next tick (up to 4 hours plus queue), and a red tick blames all the merges since the last green one.
None of this needs a change. It belongs in ADR-0348's "Evidence against" as one list.

### 12. note: `make test-changed` uses the same mapper, but not all of its answer

*Where:* `tools/test-changed.sh:41`, `tools/plan-lib.sh`.

*What:* the same `plan_changed` and the same mapper run in both places, so findings 1 to 4 apply locally too. A `full`
answer falls back to the map's `Makefile` line (every step), which is consistent with CI. Two differences:
- `test-changed.sh` never reads `unit_all`, so a change that reaches `test-unit`'s recipe runs no host tests locally.
- Locally, `kernel/` and libposix map through `test-map.txt`, not to the full set as on CI.
The second difference is probably intended. The first is not.

### 13. note: the workflow glue has no fixture

*Where:* `.github/workflows/test.yml:132-173`.

*What:* `check-ci-plan` tests `ci-last-full.rb` and `ci-plan.sh` on their own, and `check-ci-jobs` greps for strings.
The composition in the plan step has no test:
- the `SCHEDULE` comparison (finding 6);
- `|| last=` on an API error;
- skip only for the 4-hourly;
- `PUSH_MAIN` only for `push` on `main`.
A small shell function, taking the event, the cron, the ref and the lookup's output and giving the environment for
`ci-plan.sh`, could be called by the workflow and by fixtures. The fail-safe is sound as written: a failed lookup leaves
`last` empty, so there is no skip and the run is full.

## Checked and found sound

- The diff's lines are attributed in both versions. A removed line goes to the old parse and an added line to the new
  one, and pure additions and deletions (`,0` hunks) give no line on the other side.
- Continuation lines are grouped with their statement. A `define` block, used through `$(call ...)` or `$(NAME)`, is
  one variable. A recipe inside an `ifeq` stays with its rule, and a changed conditional line gives `full`.
- These are attributed correctly: multiple targets on one rule, pattern and static pattern rules, target-specific
  variables, and `$(addprefix P,...)` with an unexpandable list (it stands for every `P*` target, which is how
  `test-qemu` reaches every `test-qemu-*`).
- `.PHONY`, `.DEFAULT_GOAL`, `.ONESHELL`, `.RECIPEPREFIX`, `include`, `vpath` and a bare `export` give `full`.
- A mapper that fails (exit non-zero, no output, no `ruby`) falls back to the map's line, which is every step.
- A path under `kernel/`, `crates/zoraya-posix*`, `ports/musl/` or `tools/test-runner/` gives the full set (B3), and so
  does a candidate with no merge base.
- `ci-last-full.rb`:
  - Every API error or timeout exits 1, and the workflow then runs the full set.
  - A cancelled run and a run whose `build` was skipped do not count (B4). Nightly and 4-hourly full runs both count.
    `workflow_dispatch` and `candidate-full/` runs do not count, which can only add runs.
  - The plan job has `actions: read` and `fetch-depth: 0` (so the merge list works). A full run has about 76 jobs, under
    one page of 100.
- Concurrency: push, schedule (per cron) and dispatch runs of `main` are separate groups with no cancel in progress, so
  a push cannot cancel a scheduled or manual run (requirement 3 of the decision). A 4-hourly run that waits behind a
  slow one in its group starts its plan only after that run has completed, so it sees it and skips.
- The nightly runs `FULL=1 NIGHTLY=1` always. A push to a non-`main` branch skips the plan job, so it runs the checks
  only.

## Recommendation

Do not merge as it is. Findings 1, 2 and 5 break the decision's own fail-safe and its stage-tag rule:
- A `Makefile` change to a helper of `test-net`, `test-posix`, `test-libposix` or `ci-build` today plans no machine.
- A skipped scheduled run reads as green after a red one, and nothing checks the tag rule.

All three are small fixes in the two new scripts and the plan step, with fixtures. Findings 3, 4 and 6 to 8 should go in
with them, since they are a few lines each. Findings 9 to 13 can follow.

## Check of the fix pass (`git diff 199d38b 7732003`)

The same reviewer, on the fix commit `7732003` of `work/ci-policy`. Read-mostly: host checks only, no QEMU, no workflow
dispatched.

### What was run

| Command | Result |
|---|---|
| `make check` | 25 checks pass |
| `make check-ci-plan`, `make check-workflow-syntax` | pass |
| the 26 adversarial Makefiles of finding 2 against the new `tools/ci-makefile-steps.rb` | all as expected (table below) |
| one trailing ` x` on the first recipe line of a rule of a copy of the real `Makefile` | table below |
| 18 new adversarial edits, each alone and appended to the real `Makefile` | table below |
| `tools/ci-plan-step.sh` with a stub `tools/ci-plan.sh` that prints a partial line and exits 1, under `bash -eo pipefail` | finding 15 |

Hand breaks, each alone in a scratch clone of `7732003`, reverted:

| Break | Result |
|---|---|
| C1. `NIGHTLY_CRON` in `ci-plan-step.sh` set to `17 3 * * *` | fails `check-ci-jobs` |
| C2. the two crons swapped in `test.yml` | fails `check-ci-jobs` |
| C3. the 4-hourly cron back to `0 */4 * * *` | fails `check-ci-jobs` |
| S1. the red skip no longer exits 1 | fails `check-ci-plan` (plan step fixture) |
| S2. a failed lookup taken as `skip=1` | fails `check-ci-plan` |
| G1. `--green-for` counts a red run | fails `check-ci-plan` |
| G2. `--green-for` without the ancestor test | fails `check-ci-plan` |
| M1. the "reaches nothing: full" net removed | fails `check-ci-plan` |
| M2. the `$(MAKE)` pattern back to the old one | fails `check-ci-plan` |
| M3. the owner check of unchanged lines removed | fails `check-ci-plan` |

### The earlier findings

| Finding | State |
|---|---|
| 1 (`$(MAKE)` not followed) | Fixed for `$(MAKE)`, `${MAKE}`, `+`/`@`/`-` prefixes and `-f` (f01, f01b, f21). On the real file, `test-net-posix`, `test-net-dns`, `test-net-lookup` and `test-net-fetch` now plan `test-net`, `check-libposix-widths-built` plans `test-libposix`, `cxx-libcxx-tests` plans `test-cxx-libcxx`, `python` plans the python steps. A `$(MAKE)` held in a variable is still not followed: finding 14. Correction to finding 1: `@$(MAKE) -s posix-calls` (line 1380) is in `test-posix-long`, not `test-posix`. |
| 2 (no step reached taken as unaffected) | Fixed in the tool, and right on small Makefiles (f02, f06, f07, f08, f12, f14, f24). **Never fires on the real `Makefile`**: finding 14. |
| 3 (`export`) | Fixed (f03, f04 `full`). |
| 4 (unchanged lines that change owner) | Fixed (f09, f15 `full`; f16 `steps: test-a`). f22 is now `full`, which is harmless. |
| 5 (a skipping run green over a red one) | Fixed: the skip run exits 1 when the last full run is red (S1). `--green-for` counts only a green run whose `build` ran and whose head has SHA as an ancestor (G1, G2). Because a red skip run's `build` is skipped, it is never a full run, so the next tick skips again and is red again. That is the intended carry-forward. The stage-tag paragraph of CLAUDE.md can be implemented with it: see note 17 for one missing step. |
| 6 (copied cron string) | Fixed: `check-ci-jobs` compares the workflow's first cron with `NIGHTLY_CRON` (C1, C2). |
| 7 (busy minute) | Fixed: `23 */4 * * *` in the workflow, `check-ci-jobs`, `design.md` and ADR-0348 (C3). No `0 */4` left outside `docs/research/` and `docs/tasks/`. |
| 8 (blame list) | Fixed: every first-parent commit, with the merges marked. The fallback counts 15 commits. |
| 9, 10, 12, 13 | Fixed: the claim is reworded, INV-0345-1's Status records that only the tool is checked, `test-changed` honours `unit_all`, and the plan step is `tools/ci-plan-step.sh` with fixtures (S2: a failed lookup gives the full set). |

Earlier adversarial Makefiles, new tool: f01 `test-a`, f01b `test-a`, f02 `test-a`, f03 `full`, f04 `full`, f05 `full`, f05b
`full`, f06 `test-a`, f07 `test-a`, f08 `full`, f09 `full`, f10 `test-a`, f11 `test-a`, f12 `test-a`, f13 `test-a`, f14 `full`,
f15 `full`, f16 `test-a`, f17 `test-a`, f18 `test-a`, f19 `test-a test-b`, f20 `full`, f21 `test-a`, f22 `full`, f23 `full`,
f24 `test-a`.

New adversarial edits. "Alone" is the fixture with steps `test-a test-b`. "In the real file" is the fixture appended to
the real `Makefile`, with every step plus `test-a test-b`. In that column, `+3` stands for `test-posix test-posix-gui
test-tour-gui`, which every answer on the real file contains (finding 14).

| Case | Expected | Alone | In the real file |
|---|---|---|---|
| a01 a recipe line in a `define` used by two targets | `test-a test-b` | `test-a test-b` | `test-a test-b` +3 |
| a02 `?=`, a02b `+=`, a02c `::=` | `test-a` | `test-a` | `test-a` +3 |
| a03 a target-specific variable of a helper | `test-a` | `test-a` | `test-a` +3 |
| a04 a pattern rule | `test-a` | `test-a` | `test-a` +3 |
| a05 an order-only prerequisite | `test-a` | `test-a` | `test-a` +3 |
| a06 `$(MAKE) -f other.mk zz` renamed; a06b `$(MAKE) -f Makefile h`, h changed | `test-a` | `test-a` | `test-a` +3 |
| a07 `.SECONDEXPANSION`, `$$(ZD)`'s rule changed; a07b ZD changed | `test-a` | `test-a`; `full` | `test-a` +3; `full` |
| a08 `@-+$(MAKE) h` | `test-a` | `test-a` | `test-a` +3 |
| a09 a comment line | `steps:` | `steps:` | `steps:` |
| a10 a blank line inserted | `steps:` | `steps:` | `steps:` |
| a10b a trailing blank on a `:=` value | `test-a` | `test-a` | `test-a` +3 |
| a11 `$(call ZR,h)` with `ZR = $(MAKE) --no-print-directory $(1)` | `test-a` | `full` | **+3 only, no `test-a`** |
| a12 `$(ZSUB) h` with `ZSUB = $(MAKE) -s` | `test-a` | `full` | **+3 only, no `test-a`** |

### 14. error: on the real `Makefile`, two closures cover every rule, so the "reaches nothing: full" net never fires and a step reached through a variable that holds `$(MAKE)` is missed

*Where:* `tools/ci-makefile-steps.rb:88` (recipe lines are joined with `' '`); `makes` (`:190`, the pattern
`[^;&|\n]*` after `$(MAKE)`); `rules_for` (`:216`, an empty prefix means every rule); `main` (`:341-372`, `hit` and `lost`);
`Makefile:1282` (`posix-image`) and `:1752-1754` (`ci-build`).

*What:* the lines of a recipe are joined with a space, so the text has no newline. After `$(MAKE)`, the pattern takes the
rest of the recipe up to the next `;`, `&` or `|`. In `posix-image`, that means 60 words after `@$(MAKE) -s -f ports/Makefile
rootfs workloads`: `rm`, `cp`, paths, and `$(?)` from a `$$` word with an empty prefix, which stands for every rule. The same
happens in `ci-build` (`$(CARGO)` after `$(MAKE)`). `posix-image` is a prerequisite of `test-posix`, `test-posix-gui` and
`test-tour-gui`, so each of their closures is the whole file (363 of 364 nodes). So is `ci-build`'s. Three results:
- Every `Makefile` change also plans those three steps. A `test-boot` recipe edit answers `steps: test-boot test-posix
  test-posix-gui test-tour-gui`. This over-selects, which is safe but costly.
- Every node is `hit`, so `lost` is always empty. A new orphan rule appended to the real `Makefile` answers `steps: test-posix
  test-posix-gui test-tour-gui`, not `full`. The fix for finding 2 holds only on the fixtures' small Makefile. That is why
  `check-ci-plan` passes.
- A step that reaches a rule only through a variable whose value runs `$(MAKE)` is missed. `refs` reaches the variable,
  but `makes` is applied only to rule text, never to a variable's value. With the net disarmed, the answer leaves the step
  out: a11 and a12 above. The real file has this today. `CONFINED` (`Makefile:460-461`) runs `$(MAKE) --no-print-directory
  $(2)` through `$(call CONFINED,...)`. A recipe edit of `test-net-dns-run`, `test-net-lookup-run` or `test-hand-run-net-run`
  answers `steps: test-posix test-posix-gui test-tour-gui`, without `test-net` or `test-hand-run`. On CI these sub-steps print
  `SKIPPED`, so CI loses nothing today. `make test-changed` runs them locally, and loses them.

*Fix:* join recipe lines with `"\n"` (one line of `:88`). Checked on a scratch copy of the tool: the orphan rule gives
`full`, a11 and a12 give `full`, `test-net-dns-run` gives `full`, `test-net-posix` gives `steps: test-net`, `test-boot` gives
`steps: test-boot`, and `posix-calls` gives `steps:` (its only caller is the root `test-posix-long`). Then follow `$(MAKE)` in
the values of the variables a rule reaches. Apply `makes` to `@vars[v]` in `closure`, with `$(1)`-style words standing for
every rule, so that `CONFINED` plans `test-net` instead of `full`. Add two `check-ci-plan` fixtures on the real `Makefile`
(it is in the scratch repository's reach through `$here`): an appended orphan rule must answer `full`, and a one-character
edit of `test-boot`'s recipe must answer exactly `steps: test-boot`.

### 15. warning: `tools/ci-plan-step.sh` ignores a failure of `tools/ci-plan.sh`, which the old step did not

*Where:* `tools/ci-plan-step.sh:735-742` of the diff (the three `tools/ci-plan.sh ... > plan.txt` calls, under `set -u` only).

*What:* the old step ran under the workflow's `shell: bash` (`bash -eo pipefail`). A failing `ci-plan.sh` in `... | tee`
failed the plan job. The script has no `set -e` and does not test the status, so it exits 0. Reproduced with a stub
`ci-plan.sh` that prints `tests=["x"` and exits 1: the step exits 0 for a scheduled run, a push to `main` and a pull request.
*Scenario:* on a scheduled run, `ci-plan.sh` dies before it prints `build=` (an unbound variable under `set -u`, a git error).
`build` is empty, so `build` and `tests` are skipped, and `checks` runs. The run is green with no machine run, and it is the
fail-open the policy forbids. *Fix:* `|| { echo "ci-plan-step: tools/ci-plan.sh failed" >&2; exit 1; }` on each call, and a
fixture with a failing `ci-plan.sh` that expects exit 1.

### 16. note: the `::error::` line of a red skip goes into `$GITHUB_OUTPUT`

*Where:* `tools/ci-plan-step.sh` (`echo "::error::$red"` on stdout); the workflow's `cat step.txt >> "$GITHUB_OUTPUT"`;
the fixture `grep -q 'is red' ... "$aux/step.out"`.

*What:* stdout is the output file. A line with neither `=` nor `<<` is not a valid output line. The runner reports a
file-command error instead of the annotation. This is from the runner's documented format, not from a run. The run is red
either way, and the reason is in the summary. *Fix:* `echo "::error::$red" >&2`, and have the fixture look for it on stderr
only.

### 17. note: `--green-for` needs the runs' heads in the local clone

*Where:* `tools/ci-last-full.rb` (`git merge-base --is-ancestor want head_sha`); CLAUDE.md's stage-tag paragraph.

*What:* a run whose `head_sha` the local clone lacks is passed over. The tool then exits 1, or names an older green run that
also includes the SHA. Both outcomes are fail-safe. The window is 12 completed runs, skip runs included, about two days. A
tag on an older commit with no newer green full run exits 1. CLAUDE.md names `gh` and `GITHUB_REPOSITORY`, but not
`git fetch origin main` first. Add the fetch, or make the tool say "head X not in this clone" when it passes over a run.

### Recommendation after the fix pass

Errors 1, 3 and 5 of the first review and warnings 3, 4 and 6 to 10 are fixed, with fixtures that catch each break above.
Error 2 is fixed in the tool, but finding 14 cancels it on the real `Makefile`. Do not merge until findings 14 and 15 are
fixed: one line plus the variable-value edge and two fixtures for 14, a few lines and a fixture for 15. 16 and 17 can go in
with them or follow.
