# TASK-24: Stage 7 — the desktop (7a to 7f), autonomous run

## Where and how this runs

Locally on the architect's Mac mini (M4, 16 GB), **the only agent on the machine**, overnight and possibly for several days. The architect is travelling and will not answer questions during the run. **Do not ask: decide, record, continue** (see "Working without the architect"). Follow the memory limits in `CLAUDE.md` (heap caps, at most 2 mutant workers, the footprint watchdog, `CARGO_BUILD_JOBS=4`) and keep at least 20 GB of disk free.

Start each long command so that a hang cannot block you (a timeout, or in the background with a cap), and run QEMU only through `tools/test-runner` and `make`.

## Why

Stages 0 to 6 and the native shell are done. Stage 7 turns the GUI of stage 5 into the desktop the architect chose: **Clear boundaries, the Field variant, with dock B** (ADR-0160 to ADR-0172, accepted; mockups in `docs/research/desktop/mockups/clear-field/`). It is where Zoraya's model becomes visible: rooms, the grants on every window, the chooser that grants what the person picked, everything with its text.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/desktop/plan.md` (the order and each task's "done when"), `docs/research/desktop/mockups/README.md` (the four rules, Field's rules, dock B, the contrast table) and look at every PNG in `mockups/clear-field/`.

Read **in full**: ADR-0160 to ADR-0172, ADR-0090 to ADR-0097 (stage 5), ADR-0050 to ADR-0057, ADR-0153 to ADR-0158 and ADR-0200 (the shell: grants, `narrow`, descriptions, configuration, `exit` in the GUI), ADR-0056 (quotas, still proposed). Read the stage 5 compositor, terminal and GUI tests, and `zosh`'s GUI relay.

`plan.md` predates the choice of Field and ADR-0170 to ADR-0172. Where they differ, the accepted ADRs and the README win: the panel of 7d is the **top bar plus dock B** (tiles of neutral material, room-coloured running dots, room chips, tooltips, show-commands labels); rooms (ADR-0172) are built in 7d; the clipboard (ADR-0170) is built in 7f.

ADR numbers **0240 to 0259** are reserved for this task.

## The six parts, in order

Each part is done on its own branch `work/7X` from the latest `main` (not named like the tag), reviewed, merged and tagged `stage-7X` before the next part starts from the new `main`. Each has the "done when" of `plan.md` as its acceptance test, plus what is added here.

**7a — Tokens and text.** As `plan.md` says, with `zoraya-tokens` generated from `mockups/tokens.txt`'s Field set (light and dark). First, two fixes from the backlog, each its own commit with a test: (1) the TCG key-burst loss in the GUI terminal (find where keys are lost between the input device and `ttyd`/`zosh`, fix it, and make the GUI tests type whole lines again); (2) the test runner's `EAGAIN` panic (QEMU in test runs gets stdin from `/dev/null`, and writes to stdout retry on `EAGAIN`). Measure the rasteriser as `plan.md` asks; if it is too slow or not deterministic under TCG, write a proposed ADR with the numbers and take the conservative path the ADRs allow.

**7b — Frames, windows and scale.** As `plan.md` says, drawn to Field: title bars with the room tint, the top bar, the switcher overlay grouped by room, the glow only where ADR-0161 puts it, scale 1 and 2.

**7c — Keyboard layouts.** As `plan.md` says: US, UA, RU, CZ, dead keys, switching, the indicator in the top bar.

**7d — Toolkit, dock and rooms.** `zoraya-ui` and `zoraya-app` as `plan.md` says; dock B and the top bar as the README describes (Field's dock, not a panel with a window list); launch records; **rooms (ADR-0172)**: the room table in `/config/rooms`, `room new`/`room NAME` in `zosh`, fields drawn by the compositor around a room's windows only when two or more rooms have windows (Field's rules INV-0172-6 to -9), title-bar tints, the grant pills from the compositor's own records, the room chips in the dock, show-commands mode. **Untrusted rooms:** draw and test everything they show (the reserved hue, the two-pixel edge, the word `untrusted`, INV-0172-10), but `room new --untrusted` stays refused with a message naming ADR-0056 until quotas exist; tests may create one through a test-only path that never runs code from outside the system image.

**7e — Chooser and files.** As `plan.md` says: the chooser as a compositor-drawn sheet (ADR-0167), `zoraya-fsclient`, the file manager, the application data directory per launch; drops granting what was dropped, in the words the mockups show.

**7f — Editor, settings, configuration, clipboard.** As `plan.md` says (`edit`, the settings window over ADR-0157's file, `SettingsChanged`/`ApplySettings`, theme and motion in `Configure`), plus the clipboard of ADR-0170 (by gesture only). Then the stage 7 roadmap row, `docs/design.md`'s desktop sections ("Using the desktop" for a person), the backlog.

What is not in stage 7 stays out (`plan.md`'s last paragraph): image viewer, a Rust `std`, fractional scaling, POSIX programs started from the desktop, untrusted code, networking.

## Tests and evidence, for every part

- The part's "done when" test, the invariants its ADRs assign, host tests for every new crate, fuzzing where `plan.md` names it.
- `make test` passes locally before the merge.
- **Screens for the architect:** after each part, save 3 to 6 screendumps that show what the part made (light and dark where it applies), compared with the matching mockup, under `docs/research/desktop/shots/7X/` (at most 1 MB per part; PNG at scale 1 unless the part is about scale), and say in the report where they match the mockups and where not, and why.
- The measurements `plan.md` lists for that part, under HVF and TCG, in `docs/design.md`.

## Review, for every part

Independent review by a subagent that did not write the code, focused on the compositor's trust boundary (no client draws a frame, a pill or a sheet; no client reads another's pixels or input), rights narrowing in the chooser and rooms, and `unsafe`; findings and dispositions in `docs/tasks/TASK-24-review-7X.md`; one check of the changed lines; at most one more round. Mutation testing of the part's new and changed crates with `--in-diff` against `main`, 2 workers, a timeout from the baseline, capped at 90 minutes per part; the main model rechecks every survivor called equivalent.

## Working without the architect

- **Never stop to ask.** When a decision is needed: follow the accepted ADRs and the README; where they are silent, choose what is simplest, least surprising, reversible and closest to the mockups; write it down as a **proposed** ADR in 0240 to 0259 ("Recommendation" filled, "Architect's decision" empty) or, if small, in the ADR's open questions; and carry on.
- **Merging a part.** Merge into `main` when all of these hold: the part's acceptance test and `make test` pass; the review and its check found no open error; no proposed ADR of the part changes an accepted ADR's decision (proposed ADRs that only fill gaps are fine). Then: merge `origin/main` into the branch, merge it into `main` with `--no-ff` from a detached HEAD at `origin/main`, push `HEAD:main`, wait for every CI job.
  - CI green: tag `stage-7X`, push it as `refs/tags/stage-7X`, delete the local branch, go to the next part.
  - CI red because of plumbing (workflow, artifacts, caches, a timeout): fix forward, up to three attempts.
  - CI red because of a real failure: fix forward on a branch, up to two attempts; if `main` is still red, revert the merge (`git revert -m 1`), push, and continue the work on the branch.
  - **If a part cannot be merged** (a proposed ADR changes an accepted one, or an open error remains): do not merge it; start the next part on top of that branch (`work/7Y` from `work/7X`), note it in the report, and leave the decision to the architect.
- **Stop completely only** if continuing would: delete or rewrite anything outside this repository's worktree, force-push or move a published tag, exhaust memory or disk despite the limits, or run code from outside the system image as untrusted. Then commit, push the branch, write the report and stop.
- **Push your branch after each step** (only `work/7X` and, at merge time, `main` and the tag). Never force-push.
- If you see a usage-limit warning: commit, push the branch, update the report, and stop at a clean point.

## Report

Keep one running report, `docs/tasks/TASK-24-report.md`, updated and pushed after each part: what was built, the screens and how they compare with the mockups, the measurements, the invariant delta, every proposed ADR with its one-line recommendation (these are the architect's queue), what was merged and tagged, what was not and why, the time per step (writing, builds, host tests, QEMU runs, review and mutants), and the commands to try the desktop by hand (`make run-gui` and what to click or type).

## Git

Configured identity only. Branches `work/7a` … `work/7f`; tags `stage-7a` … `stage-7f`, and `stage-7` on the commit that closes 7f. Commit after each step.

## Definition of done

All six parts merged and tagged (or, for a part that could not be merged, its branch pushed with the reason in the report), `make test` and CI green on `main`, the report complete, and `docs/design.md` describing the desktop for a person.
