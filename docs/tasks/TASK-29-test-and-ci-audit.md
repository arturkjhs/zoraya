# TASK-29: Audit of the tests and the CI, with the fixes that are safe to make

## Where and how this runs

On the architect's **MacBook Air M1**, in its own clone, beside TASK-28 (stage 10 design, another clone on the same machine) and while the Mac mini runs the stage 9 dispatcher (TASK-27). The architect is travelling: **do not ask; decide, record, continue.**

- **Models:** the main session on Opus (or Fable if it is what was started); `ci-watch`, `log-triage` and similar fetching and summarising on Haiku subagents. Bulk data (run histories, job timings, logs) goes into files under `build/audit/` and is computed with scripts; never page large outputs through the conversation.
- **This machine builds nothing heavy:** no QEMU, no `make image`, no ports, no mutation runs. Allowed locally: the Rust toolchain from `rust-toolchain.toml`, `make check` and host `cargo test` of single crates, with `CARGO_BUILD_JOBS=2`. Everything that needs QEMU is verified by CI.
- **CI is the shared bottleneck** with TASK-27: at most **one** candidate run of this task at a time, and none while a `candidate/*` of TASK-27 or a `main` run is queued for more than a few minutes; batch fixes so that the whole task needs few candidate runs (aim for at most three).

## Why

CI is the slowest step of every part: a full run is about 58 jobs and 200+ runner-minutes, and runs queue behind each other on the free plan's concurrency. Several failures in stage 9 were flakes (`t_cpu`'s masked time, `test-boot` on `boot-tcg-smp4-gicv3`, `t_threads` case `waits` line 837), and a review of 9d found a test that passes without doing what it claims (the "Proceed" check). On GitHub's runners there is no HVF and no KVM (`docs/research/ci-arm.md` on `work/ci-arm`), so `auto` is TCG there: jobs and legs that run the same thing under `auto` and `tcg` may be running it twice.

## Context

Read first: `CLAUDE.md`, `.github/workflows/test.yml`, `tools/ci-plan.sh`, `tools/test-runner` and `tools/run-qemu.sh` (how `auto` resolves, `runs_under_tcg`, the timeouts), the `Makefile`'s test targets, `tests/` (the scripts and their expected and forbidden outputs), the kernel's and services' in-QEMU tests, `docs/backlog.md` (the flake rows), `docs/tasks/TASK-26-report.md` and `docs/tasks/TASK-27-report.md` (measurements, flakes, reviews), ADR-0270 (bounded kernel work, the `MAX_UNITS` check) and the `t_cpu` fix of 2026-11-13 (`work/cpu-flake`, merged).

ADR numbers **0350 to 0359** are reserved for this task (a test policy that changes what a test may assume, for example about time under TCG, is an ADR).

## Part 1: the audit (documents only)

Write `docs/research/test-audit/report.md`, every claim with its evidence (a file and line, a run id, a measured number):

1. **What CI runs.** From `test.yml` and `ci-plan.sh`: every job, what it runs, under which accelerator it really runs on a GitHub runner, and its time over the last two weeks of runs (median and worst, from `gh run list` / `gh run view --json jobs` into `build/audit/`).
2. **Duplicates.** Jobs, legs and runs that do the same work twice on a runner where `auto` is `tcg` (`POSIX_ACCELS="auto tcg"`, `GUI_RUNS` with both `:auto:` and `:tcg:`, `test-qemu-auto-*` beside `test-qemu-tcg-*`, the same script in two jobs). For each: whether anything differs (timeouts, limits keyed on the accelerator, `smp`, memory), and what dropping one leg loses. The `auto` legs keep their meaning on the Mac (HVF) for `make test` and `make test-changed`.
3. **Flakes.** Every job that failed and then passed on a re-run, or failed on one commit and passed on the next with no related change, over the last two weeks: the run ids, the failing line, and the cause class (a wall-clock assumption under TCG, a fixed sleep, an order assumption between processes or threads, a timeout too short for TCG with SMP, a host-side race in the runner, a real defect). Read the test's code for each, not only the log.
4. **Tests that can pass without testing.** Scripts whose expected output can appear before the action it is meant to prove (as the "Proceed" check), expected strings that a failure path also prints, forbidden strings that cannot occur, host tests with no assertion on the result, `#[ignore]`d or skipped tests and why, mutants listed as killed by a test that does not reach them. Say how each was found.
5. **Time.** Where the full run's time goes: the build stage before any test starts, the waves caused by the concurrency limit, the longest jobs, fixed durations (`FUZZ_SECONDS=600`), caches that miss. What a full run on `main` and a candidate run would cost after each proposed change.
6. **Coverage holes the audit happened to see** (not a coverage study): trust-boundary rules that no test exercises, invariants marked `checked` whose check does not check them.

## Part 2: the fixes

On branch `work/test-audit` from the latest `main` (merge `origin/main` in before each candidate). Each fix is its own commit with the finding's number. Make the fixes that are safe:

- **CI plumbing:** drop duplicated `auto`+`tcg` legs on GitHub runners where nothing differs (keep both on the Mac); move fixed-duration work that is not about the change (long fuzzing) to the nightly full run, with a shorter run on candidates; anything else from item 5 that changes no test's meaning. Coordinate with TASK-27's `work/ci-docs` (main's docs-only rule in `ci-plan.sh`): if it is not merged yet, build on top of it or leave `ci-plan.sh`'s main rule alone.
- **Flakes whose cause is proven:** replace wall-clock assumptions under TCG by deterministic checks where one exists (as `t_cpu` did with `MAX_UNITS`), wait for an event instead of sleeping, give TCG with SMP the time it needs. Never weaken a check on HVF, and never turn a real defect into a longer timeout: a flake whose cause is a defect goes into the report and `docs/backlog.md` for the architect.
- **Vacuous tests:** make each one fail when the behaviour is absent, and show that it does (apply the break by hand, see the failure, revert; say so in the commit).

Do not touch code under `kernel/`, `services/`, `crates/` other than their tests, and nothing on TASK-27's branches (`work/9c`, `work/9d`, …). A fix that needs a change outside tests goes into the report as a proposal.

Verification: `make check` locally; the candidate CI (tag `candidate/work-test-audit-<letter>`) for everything else, with `ci-watch`. When it is green, **do not merge**: the TASK-27 dispatcher on the mini merges it like a part. Write in the report what the branch contains and that it is ready.

## Review

An `opus-review` subagent that did not write the fixes checks part 2: that no dropped leg or changed check loses a real test, that no flake fix hides a defect, that every vacuous-test fix was shown to fail without the behaviour. Findings and dispositions in `docs/research/test-audit/review-notes.md`; fix; one check of the changed lines.

## Git

Configured identity only. Commit after each step and push `work/test-audit` and the candidate tags only; never push `main`, never merge, never force-push, never move a tag. On a usage-limit warning: commit, push, write where you stopped in `docs/research/test-audit/status.md`, stop.

## Definition of done

The report (items 1 to 6); the branch with the safe fixes, green on its candidate, reviewed; a "Status for the architect" block at the top of the report: runner-minutes and wall time of a full run and of a candidate run before and after, the flakes fixed and the ones left with their causes, the vacuous tests fixed, the proposals that need the architect or code outside tests.
