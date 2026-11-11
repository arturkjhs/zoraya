# TASK-26: Stage 8 — quotas and the network (Q1–Q7, N1–N6), autonomous run

## Where, who and how

Locally on the architect's Mac mini (M4, 16 GB), **the only build-and-test agent on the machine**, for several days. The architect is travelling and will not answer during the run: **do not ask; decide, record, continue** (as in TASK-24).

**Models, to save the usage budget** (`CLAUDE.md` and `.claude/agents/`):
- This main session runs on **Sonnet**: implementation of accepted ADRs, tests, documentation, reports.
- **`opus-review`** (Opus; Fable if offered): every independent review; every change to the kernel, `unsafe`, the charge and scheduler code, a service's accounts or a trust boundary gets its review there; and any bug you have not found in two attempts.
- **`ci-watch`, `log-triage`, `test-runner`, `mutant-triage`** (Haiku): waiting on CI, reading logs, test runs, mutant triage. Never poll CI or read long logs in this session.
- After each merged part, write the handoff into the report and start the next part in a **fresh session** (long sessions cost the most).

Follow the memory limits (heap caps, at most 2 mutant workers, the footprint watchdog, `CARGO_BUILD_JOBS=4`), keep 20 GB of disk free, start long commands with a timeout or in the background, run QEMU only through `tools/test-runner` and `make`.

**Testing:** during work, `make check` and `make test-changed`; before a merge, the full CI on the branch (the merge-candidate trigger); merge only when it is green. Mutation testing with `--in-diff`, at most 45 minutes per part, only for the kernel, the charge and scheduler code, the services' accounts, `netd`'s scopes and the resolver, and the rooms keeper.

## Why

Two things wait on this stage: untrusted code (AI agents in untrusted rooms, the isolation direction of the vision) and the network. The designs are accepted: quotas in ADR-0260 to ADR-0270 with the architect's answers recorded there, the network in ADR-0220 to ADR-0230. Their plans are `docs/research/quotas/plan.md` and `docs/research/network/plan.md`; each task's "done when" there is its acceptance test.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, both plans, `docs/tasks/TASK-24-report.md`'s handoff. Read in full the ADRs each part names before starting it (ADR-0260 to ADR-0270 and ADR-0220 to ADR-0230 above all; ADR-0002, ADR-0046, ADR-0050, ADR-0051, ADR-0052, ADR-0054, ADR-0055, ADR-0091, ADR-0104, ADR-0153, ADR-0172 where a part touches them).

ADR numbers **0280 to 0299** are reserved for this task.

Decisions already made that the plans leave open:
- ADR-0222's `connect` takes the client's stream ends (the quotas answer 3): N3 builds that shape directly, so Q3's `netd` part is the per-scope account only.
- ADR-0228's crypto provider: try `ring` first with the toolchain's clang; BearSSL is the fallback, for native programs too.
- ADR-0221 item 8 (virtio pages when the GUI and the network are both on): take its recommended option, register access by kernel call, in N6.

## The parts, in this order

One part at a time, each on its own branch `work/<part>` from the latest `main` (not named like the tag), merged and tagged `stage-8-<part>` before the next starts:

1. **Q1** frame and kernel budgets on jobs (the most risk; everything depends on it).
2. **Q2** processes, the fork bomb, bounded kernel work.
3. **N1** the driver answers ARP.
4. **N2** `netd` answers ping and DHCP.
5. **Q5** storage limits.
6. **Q3 (fsd part)** accounts in the services.
7. **Q4** the CPU ceiling.
8. **N3** a native program fetches a page over HTTP (with the client's stream ends).
9. **N4** a POSIX program fetches the same page, together with **Q3's `netd` account**.
10. **N5** HTTPS with a real clock and real randomness.
11. **Q6a** the rooms keeper and the room line.
12. **Q6b** what the person sees (`top`, pills at 90%, the settings page).
13. **N6** failure, rooms and the desktop (with ADR-0221 item 8 as decided above).
14. **Q7** the gate's tests, both stages (without and with `+net`).

## The gate stays closed

ADR-0268 makes the architect's go-ahead one of the gate's conditions. Build and pass everything Q7 asks, write the `opus-review` report of the system call surface, and then **stop with `room new --untrusted` still refused**: the report says which conditions hold, and the architect opens it. `make check-untrusted-gate` keeps the refusal in place until then. Never run code from outside the system image as untrusted.

## For every part

- The plan's "done when" test, the invariants its ADRs assign, host tests for every new crate, the fuzz targets the ADRs name.
- The measurements the plans list for that part, under HVF and TCG, in `docs/design.md`.
- Review by `opus-review`, findings and dispositions in `docs/tasks/TASK-26-review-<part>.md`; one check of the changed lines only if the review found an error.
- For parts a person can see (N3 to N6, Q6b): a few screens or terminal transcripts in the report.

## Working without the architect

As in TASK-24:
- Where accepted ADRs are silent, choose what is simplest, least surprising, reversible and safest, write it as a **proposed** ADR (0280 to 0299) or in the ADR's open questions, and continue.
- **Merging a part:** when its acceptance test and the full CI on its branch pass, the review found no open error, and no proposed ADR of the part changes an accepted decision: merge `origin/main` into the branch, merge it into `main` with `--no-ff` from a detached HEAD at `origin/main`, push `HEAD:main`, let `ci-watch` wait for main's CI; green: tag `stage-8-<part>` (push as `refs/tags/...`), delete the branch; red from plumbing: fix forward, up to three attempts; red from a real failure: fix forward twice, then revert the merge and continue on the branch.
- **A part that cannot be merged:** start the next part on top of its branch and leave the decision to the architect in the report.
- **Stop completely only** for: anything outside this repository's worktree, a force-push or a moved published tag, memory or disk exhaustion despite the limits, or untrusted code. Then commit, push the branch, write the report, stop.
- On a usage-limit warning: commit, push, update the report and the handoff, stop at a clean point.

## Report

One running report, `docs/tasks/TASK-26-report.md`, updated and pushed after each part: what was built, the measurements, the invariant delta, the proposed ADRs with one-line recommendations (the architect's queue), what was merged and tagged, what not and why, the time per step and **which model did what** (to see what the Sonnet/Opus split costs and saves), the gate's state, and the commands to try each visible result by hand (`make run --net`, `fetch http://...`, `top`, `room new ...`).

## Git

Configured identity only. Branches `work/q1`, `work/n1`, …; tags `stage-8-q1`, …, and `stage-8` on the commit that closes Q7's first stage (the gate ready, still closed). Commit after each step; push the branch after each step.

## Definition of done

All fourteen parts merged and tagged (or a branch pushed with the reason), `make test` and the full CI green on `main`, the gate ready and closed with its conditions listed, the report complete, `docs/design.md` describing quotas and the network for a person.
