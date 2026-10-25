# TASK-03: First architecture decision records (ADRs)

## Context

We are designing a new operating system: Rust (`no_std`) kernel, aarch64, QEMU `virt`. Native interfaces are our own design; POSIX exists only as a compatibility personality for hosting existing software such as a C toolchain.

Read first, completely:
- `docs/research/design-mistakes.md`, `held-up-well.md`, `alternatives-postmortem.md`, `candidate-principles.md`, `research-log.md` (TASK-01)
- everything in `docs/research/task-02/` (TASK-02)
- everything in `docs/research/task-02b/` (TASK-02b)

Key measured facts from TASK-02 and TASK-02b that these ADRs must engage with (verify against the files, do not take this summary as the source):
- a self-hosting C toolchain (tier T1) uses roughly 134–248 libc functions; parallel builds add no new libc imports;
- only the shell forks; make, tcc and core utilities only create children that immediately exec, which a spawn call can replace; dash also forks children that keep running shell code (subshells, command substitution, builtin pipelines);
- in non-interactive T1–T3 workloads the only signal delivered is SIGCHLD and no syscall is interrupted; Ctrl-C and job control appear only in the interactive shell session;
- parallel GNU make adds exactly one readiness call: `pselect6` on the jobserver fd with an empty signal mask; most of these waits end because a child exited, not because a token arrived; no `poll`, `epoll` or `select` appeared anywhere;
- `pdpmake` has no real `-j`; parallel builds in the measured set require GNU make;
- on Ctrl-C the terminal signals the whole foreground process group at once; each make deletes its partial target, waits for its children and re-raises SIGINT on itself; dash then retakes the terminal; on SIGTERM to the top make, make forwards it one level at a time, with a timing-dependent race;
- two interfaces to other programs could not be designed away inside the programs: the jobserver address passed in `MAKEFLAGS`, and "died by signal N" as an exit status the parent acts on.

The human architect makes all final decisions. Your job is to prepare decisions so well that the architect can accept, change or reject each one in minutes, with full visibility of the evidence and the costs.

## ADR template

Create `docs/adr/0000-template.md` with exactly this structure, then use it for every ADR:

```markdown
# ADR-NNNN: <decision title>

- **Status:** proposed | accepted | rejected | superseded by ADR-XXXX
- **Date:** YYYY-MM-DD
- **Depends on:** ADR-XXXX, ... (or none)
- **Evidence:** DM-xxx, GR-xxx, CP-xx, X-n, task-02/<file>#<section>, ...

## Context
The problem, in terms of our project. What forces are in play. What is measured vs. assumed.

## Decision drivers
Numbered list. Each driver cites evidence IDs or measurements.

## Considered options
At least three. Always include the "do what Linux/POSIX does" option as a baseline, and at least one option from a non-Unix system in `alternatives-postmortem.md`.

### Option A: <name>
- **Description:**
- **Evidence for:** (with IDs)
- **Evidence against:** (with IDs)
- **Cost:** implementation effort, performance, compatibility, complexity — estimated, labelled as estimates
- **Precedent:** which systems did this, and how it went

(repeat per option)

## Recommendation
The agent's recommended option and the reasoning, clearly separated from the neutral option analysis above. Marked as a recommendation, not a decision.

## Consequences
If the recommendation is accepted: positive, negative, and follow-up work it creates.

## Invariants
Rules that follow from the decision and can be checked mechanically (by a test, a lint, a CI script, or a type in the `abi` crate). For each: the rule, and how it would be checked. Do not implement the checks.

## Open questions
What is unknown and how it could be resolved (measurement, prototype, research).

## Revisit triggers
Concrete new evidence that should reopen this decision.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
```

## ADRs to write

All with status `proposed`.

1. **ADR-0001: How decisions are made and used.** Statuses and their meaning; only the architect sets `accepted`/`rejected`; how implementation agents must treat accepted ADRs (follow them; any deviation requires a new or superseding ADR, never a silent change); how invariants become checks. Keep it short.

2. **ADR-0002: Boundary between the native ABI and the POSIX personality.** Where POSIX lives (pure library in each process, a userspace personality server, kernel-assisted, or a combination); which kernel facilities the personality may require that native programs never see (for example copy-on-write address-space cloning for `fork` emulation); how a POSIX process and a native process interact (files, pipes, process handles); how POSIX global namespaces (`/`, `/tmp`, `$PATH`) map onto native handles.

3. **ADR-0003: Process creation.** Native primitive (spawn with explicit handle list, or alternatives), and how `fork` is or is not supported inside the personality. Must use the TASK-02 call-site data: which call sites spawn replaces directly, and what the dash fork-without-exec cases require. Include as an option a native shell that runs subshells without fork, and say what it would cost.

4. **ADR-0004: Asynchronous notification without Unix signals (native side).** How native programs learn about child exit, faults/exceptions, and user interrupt (Ctrl-C), and how the personality emulates the signals that the measurements show are actually needed (SIGCHLD in non-interactive work; SIGINT, SIGTERM and job-control signals in interactive shells and interrupted builds). Must cover: interrupt delivery to a group of processes (a "job") vs. forwarding one level at a time, each process's chance to clean up, and how "terminated by interrupt" is represented in the exit status a parent sees. The general waiting mechanism is ADR-0006; ADR-0004 and ADR-0006 must be consistent and say explicitly which parts belong to which.

5. **ADR-0005: Scope of POSIX compatibility.** Tier T1 only, or T1 + T2 (autoconf `./configure` support), with the TASK-02 W4 data on which configure checks fail with tcc and what supporting T2 would add to the surface. Include what is explicitly out of scope (for example `ptrace`, System V IPC, if the data supports excluding them).

6. **ADR-0006: Event and readiness mechanism (X-4).** The native primitive for waiting on several things at once (handle readable/writable, process exited, interrupt arrived, timer), using the TASK-02b event timelines as the primary input. Must show, for each measured scenario (serial build, `make -j` with both jobserver styles, recursive make, Ctrl-C, SIGTERM, shell background jobs), how it would be expressed with the proposed primitive, and how the personality implements `pselect6` + SIGCHLD and the `MAKEFLAGS` jobserver on top of it. Options must include POSIX (`pselect`/`poll` plus signals), a completion-queue design (io_uring-style), and a handle/port design from a non-Unix system (e.g. Zircon ports, NT wait objects).

Also create:
- `docs/adr/index.md` — table of all ADRs with status, one-line summary and dependencies.
- `docs/adr/invariants.md` — all invariants from all ADRs in one list, with the ADR they come from.

## Evidence rules

- Every claim in Context, Decision drivers and option analysis cites a TASK-01/02/02b ID or file section. Check that the cited entry actually says what you claim.
- Where you rely on something not in the research files, label it `[outside evidence]` and give a source, or `[assumption]`.
- Steelman every option you do not recommend. The architect must be able to choose a rejected option with full information.
- Do not hide disagreements between principles: if an ADR violates a candidate principle, say which one and why.

## Process

1. Read all context files.
2. Write the template and ADR-0001.
3. Draft ADR-0002 to ADR-0006 in dependency order (0002 first, since the others depend on the boundary; draft 0004 and 0006 together, since they share the interrupt and child-exit paths).
4. Independent review: a subagent that did not draft the ADRs checks each one for (a) citations that don't support the claim, (b) options that are weakened or strawmanned, (c) recommendations that contradict the evidence, (d) invariants that are not actually checkable. Record findings in `docs/adr/review-notes.md` and fix the ADRs.
5. Fill `index.md` and `invariants.md`.

Work on the branch `adr/first-batch`, branched from the latest research branch. Commit after each ADR. Do not merge or push.

## Git rule

Use the repository's configured git identity. If none is configured, **stop and ask**. Never pass `-c user.name` / `-c user.email` or invent an identity.

## Definition of done

- Template, ADR-0001 to ADR-0006, `index.md`, `invariants.md`, `review-notes.md` exist.
- ADR-0006 walks through every TASK-02b scenario with the proposed primitive.
- Every ADR has at least three options, including a POSIX baseline and a non-Unix precedent.
- Every invariant has a stated checking method.
- Independent review completed and its findings addressed.
- All ADRs have status `proposed`; the "Architect's decision" section is empty.

## Out of scope

- Writing OS code or implementing invariant checks.
- Setting any ADR to `accepted`.
