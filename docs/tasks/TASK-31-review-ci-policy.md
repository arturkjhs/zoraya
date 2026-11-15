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
