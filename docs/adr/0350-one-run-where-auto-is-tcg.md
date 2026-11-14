# ADR-0350: Where `auto` is TCG, a run and its `tcg` twin are run once

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0110, ADR-0212, ADR-0213
- **Evidence:** test-audit/report.md#2-duplicates, test-audit/report.md#5-time

## Context

ADR-0110 item 6 puts the POSIX scripts into `make test` "under `auto` and `tcg`", so that on a Mac each runs under
HVF and under TCG. ADR-0213's Decision made every step a CI job and added INV-0213-3: CI runs "every script of
`test-posix` under every accelerator", every configuration of `test-qemu`, and (since stage 7) every run of
`GUI_RUNS_ALL`; `make check-ci-jobs` enforces it. ADR-0212 already noted that "on CI (Linux) both are TCG".

Measured by TASK-29 (test-audit/report.md#2-duplicates): on GitHub's runners `tools/run-qemu.sh` resolves `auto` to
`tcg`, there is no HVF and no KVM, and nothing else tells the two runs apart. The guest does not read the
accelerator (`t_cpu` and `t_bound` calibrate themselves), the runner asks `run-qemu.sh --print-accel`, and the
machine options of each pair are the same. So CI runs the same QEMU command line twice for: two of the seven
`test-qemu` configurations, the tour in the window, `w9-term.script`, `tour.script`, eight more POSIX scripts, four
`libposix` steps, `test-posix-gui`, twelve GUI runs and the second runs inside `test-net` and `test-apps`. In run
43553496171 that was about 58 of 284 runner-minutes (test-audit/report.md#5-time). The one difference found is
against the `auto` run: several keep the 180 s limit set for HVF while their `tcg` twin has 400 s or more.

Assumed, not measured: the wall time this saves (it depends on the queue).

## Decision drivers

1. CI is the slowest step of every part: a full run is 44 minutes at the median and 284 runner-minutes
   (test-audit/report.md#5-time), against ADR-0213's target of 10 to 12 minutes.
2. No test may be lost: every script, configuration and GUI run must still run under TCG in CI, and under HVF and
   TCG on a Mac.
3. A second run of the same thing is a second sample of an intermittent failure
   (test-audit/report.md#2-duplicates): that has a value, and a cost in jobs that fail on a flake.
4. `make check-ci-jobs` must still fail when CI leaves a run out.

## Considered options

### Option A: keep both runs everywhere (ADR-0213 as it is)

- **Description:** CI runs every pair twice, as now.
- **Evidence for:** Two samples a run: of the flake classes of the last two weeks, two failed in one leg and passed
  in the other inside one job (test-audit/report.md#2-duplicates). No change to a check.
- **Evidence against:** Driver 1: 58 runner-minutes a run for no new coverage. The `auto` runs carry the HVF time
  limit under TCG, which only makes them fail earlier on a slow runner.
- **Cost:** none to build; the CI time above on every run *(measured)*.
- **Precedent:** the state since ADR-0213.

### Option B: one run where `auto` is TCG, chosen by the host

- **Description:** `Makefile` asks `tools/run-qemu.sh --print-accel` what `auto` is. With `FULL=1` on a host where
  it is `tcg`: the per-accelerator loops run `tcg` only; the `auto` configurations of `test-qemu` that have a `tcg`
  twin (`auto-default`, `auto-el2`) are left out and the twin expects their lines; a GUI run `S:auto:N` is left out
  when `S:tcg:N` is listed; a recipe's own second run under TCG is not made, and its `auto` run gets the twin's
  longer limit. CI's list (`tools/ci-tests.txt`) holds the `tcg` run of each pair. On a host where `auto` is HVF
  nothing changes: `make test` and `make test-changed` run `auto`, `FULL=1` runs both.
- **Evidence for:** Drivers 1, 2 and 4: every script, configuration and GUI run still runs once under TCG in CI and
  under both on a Mac; the check now also refuses the same run twice.
- **Evidence against:** Driver 3: one sample a run instead of two; the nightly run still samples every night.
  `auto-smp4-2g` has no twin (4 CPUs, 2 GiB, GICv2) and stays, as do W8's builds and the GUI runs that exist only
  under `auto`.
- **Cost:** *(estimate)* a few lines of `Makefile`, the list and the check; about 58 runner-minutes less a run.
- **Precedent:** ADR-0212's Decision, which runs W8 under one accelerator in `make test` for CI's budget.

### Option C: one run on merge candidates, both in the full set

- **Description:** As B for candidates; pushes to `main`, the nightly run and `candidate-full/` keep both runs.
- **Evidence for:** Keeps the second sample where the full set runs.
- **Evidence against:** The full set is where the time is (driver 1): every push to `main` is a full run. Two lists
  to keep in step, and a candidate's green run would no longer be the part of the full set its change needs.
- **Cost:** *(estimate)* as B plus a second form of each line in the plan.
- **Precedent:** none here.

## Recommendation

**Recommendation (not a decision): Option B.**

1. Where the host's `auto` is TCG, `FULL=1` runs each pair once, as Option B describes. `FULL=0` and a host with HVF
   are unchanged.
2. `tools/ci-tests.txt` is the list for GitHub's runners: it holds the `tcg` run of each pair and never both.
3. `make check-ci-jobs` takes a script under `tcg` for the script under `auto`, a GUI run `S:tcg:N` for `S:auto:N`,
   and the twin of a configuration in `QEMU_AUTO_TWINS` for that configuration; it fails when the list holds both of
   a pair, and when `FULL=1` would not run both on a host with HVF.
4. If CI ever gets a hardware accelerator (KVM on an arm64 runner), `auto` there is no longer TCG and the pairs are
   two runs again: the list then gets its `auto` runs back.

This changes INV-0213-3's "under every accelerator" for CI to "under TCG", which is what CI has always run.

## Consequences

- **Positive.** About a fifth of a full run's runner time; 42 jobs instead of 47; the four longest QEMU jobs about
  half as long. The `auto` runs that remain inside recipes get the limit their `tcg` twin had.
- **Negative.** One sample of each run a CI run, not two. The labels in CI's logs change (`posix-zosh-tcg` only).
- **Follow-up work.** None for the tests. ADR-0213's INV-0213-3 is read with this ADR.

## Invariants

- **INV-0350-1.** CI's list runs every script of `test-posix`, every GUI run of `GUI_RUNS_ALL` and every
  configuration of `test-qemu` once under TCG: a pair's `tcg` run stands for its `auto` run, and the list never
  holds both. *Check:* `make check-ci-jobs`.
- **INV-0350-2.** With `FULL=1` on a host where `auto` is HVF, both accelerators, every configuration and every GUI
  run are run; where `auto` is TCG, one of each pair and no second run inside a recipe. *Check:* `make
  check-ci-jobs` (`make print-legs` for each kind of host).

## Open questions

- The wall time saved is not measured yet; the first candidate run of TASK-29 gives it.
- `test-boot` and `test-keeper` keep their `tcg` twins (14 s together); not worth a rule.

## Revisit triggers

- A CI runner with KVM or HVF.
- An intermittent failure that the second sample would have caught and the nightly run did not.

## Architect's decision

(Left empty for the architect.)
