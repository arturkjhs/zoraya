# TASK-25: Design for quotas (resource limits for jobs and rooms)

## Where and how this runs

Locally on the architect's Mac mini, **documents only**: no builds of the repository, no `make`, no `cargo`, no QEMU. Another agent runs stage 7 on the same machine and is its only build-and-test agent; do not slow it down, and do not touch its branches (`work/7*`) or its worktree. Web access is needed. Main model: Opus (this is a design task); cheap work (fetching and summarising sources, checking links) may go to Haiku subagents.

## Why

ADR-0056 (quotas) has been proposed since stage 4 and is still not accepted. The rule since then: **no untrusted code runs until quotas are accepted and implemented.** Three things now wait for it:

- **Untrusted rooms** (ADR-0172): drawn and tested, but `room new --untrusted` is refused until quotas exist. This is the isolation direction of the vision: running AI agents and other untrusted code with only what they were given.
- **The network** (ADR-0220 to ADR-0230): their report lists what quotas must cover: connections per scope, socket buffers in `netd`, listening ports, pending resolutions, datagrams queued for a slow reader.
- **Stage 6's measurements:** a fork bomb in a session stops only when memory runs out (38 forks, TASK-21); `snapshot_process` copies eagerly; each running `hello` takes a 1 MiB heap step (the ASID test's failure).

A capability says *what* a program may reach. A quota says *how much* of the machine it may use. Without the second, an untrusted program that was given nothing can still take the whole machine down.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (Vision), `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0046, ADR-0050, ADR-0051, ADR-0055, **ADR-0056**, ADR-0057, ADR-0080, ADR-0101, ADR-0102, ADR-0104, ADR-0130, ADR-0153, ADR-0172, ADR-0190 to ADR-0192, ADR-0220 to ADR-0230 (and `docs/research/network/`), and whatever stage 7 has merged about the compositor's clients (window buffers are memory a client makes the compositor hold). Read the kernel's job, frame accounting and scheduler code, so the design matches what exists; the task's reports `TASK-21-review.md` and the backlog rows on memory.

ADR numbers **0260 to 0279** are reserved for this task.

## Part 1: research

Write `docs/research/quotas/survey.md`, same evidence rules as TASK-01 (primary sources, every URL fetched, never invent a citation, paraphrase):

- **Systems:** Linux cgroups v2 (memory, pids, cpu, io; the OOM killer and `memory.high` vs `memory.max`); FreeBSD `rctl` and jails' limits; Fuchsia's job policies and memory attribution; seL4's untyped memory (the creator pays for every kernel object, by construction); KeyKOS/EROS space banks and meters; Genode's resource trading (a client donates RAM quota to a server for the session it opens); Windows job objects; macOS/iOS memory limits (jetsam). What each limits, who pays for objects a service creates on a client's behalf, what happens at the limit, and what went wrong.
- **The service-pays problem:** when `fsd`, `netd`, the broker or the compositor allocate for a client, whose budget pays? Genode's session quotas and seL4's caller-provided untyped as the two clean answers; what each costs here.
- **CPU:** fair share between jobs, a ceiling for a room, and what a single-CPU guest (Zoraya today) needs versus SMP later.
- **Storage:** space a room may use on a volume (`/tmp` per session, `~/sandbox` for an agent), and how `fsd` would enforce it.

## Part 2: decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, "Architect's decision" empty, at least three options each with a Linux/FreeBSD baseline and a non-Unix precedent, steelmanned, "Effect of the vision", and **examples of what a person types and sees** in `zosh` and on the desktop. Decide whether to revise ADR-0056 in place (it is still proposed) or supersede it, and say why. At least:

1. **What is limited:** physical memory (frames), kernel objects and handles, processes and threads, queued messages and stream buffers, CPU time, storage, network objects (from the network design). Which are hard limits in stage 8 and which are later.
2. **The accounting model:** budgets on jobs, nested (a child job's budget comes out of its parent's); who pays for kernel objects; who pays for objects a service creates for a client (session quotas donated at connect time, or the client creating them); how memory objects shared between jobs are charged.
3. **At the limit:** refusal statuses (`NoMemory`, a new `OverQuota`?) versus suspending or killing the job; how POSIX sees it (`ENOMEM`, `EAGAIN`); what the person sees (a room's pill, a `zosh` hint, a notice on the desktop).
4. **CPU:** a share per job and a ceiling per room in the scheduler; what "this agent may use at most a quarter of the machine" means on one CPU.
5. **Storage:** a per-room space limit in `fsd`, for `/tmp` and for directories an untrusted room may write.
6. **Defaults and syntax:** what a trusted room gets (no limit, or the machine's); what an untrusted room gets by default; how a person sets limits (`room new agent-1 --untrusted --mem 512M --cpu 25% +w ~/sandbox/agent-1 +net api.example.com:443`), and how `grants`, `room` and the compositor show them, as text (rule 3 of the design).
7. **Observability:** a command that shows each job's or room's use against its budget (a `top` for jobs), and what the settings window shows.
8. **When untrusted code may run:** the exact conditions under which `room new --untrusted` stops being refused (which ADRs implemented, which tests pass), so the rule "no untrusted code before quotas" has a precise end.

For each ADR give the invariants a later implementation can check, and how (kernel tests, the fork bomb as a test that must now stop at the room's limit with the session alive, a memory hog, a CPU spinner next to an interactive terminal whose keystrokes must stay responsive, a disk filler in `/tmp`).

Also write `docs/research/quotas/plan.md`: implementation tasks of comparable size (for example Q1 frame budgets on jobs in the kernel; Q2 process and handle limits; Q3 service quotas for fsd and the broker; Q4 CPU share; Q5 storage limits; Q6 the room syntax, the `top`, the desktop's display; Q7 untrusted rooms enabled), each with a visible result and its dependencies, and how it interleaves with the network tasks N1 to N6.

## Review

Independent review by a subagent that did not write the ADRs (on Opus): citations fetched and accurate; options not strawmanned; consistency with accepted ADRs, listing every tension (especially ADR-0002, ADR-0051, ADR-0102's eager snapshot, ADR-0104, ADR-0172, the network ADRs); whether the defaults are safe and the syntax short. Findings and dispositions in `docs/research/quotas/review-notes.md`; fix; one check of the changed lines; at most one more round.

## Working without the architect

Do not ask questions during the run: where the accepted ADRs are silent, recommend what is simplest, least surprising and safest, write it in the ADR, and continue. The architect decides when reading the report.

## Git

Branch `adr/quotas` from the latest `main`, in its own worktree. Configured identity only. Commit after each part; **you may push this one branch and nothing else**; never merge, never force-push. Touch only `docs/adr/0260-*` to `docs/adr/0279-*`, `docs/research/quotas/`, and, if ADR-0056 is revised in place, `docs/adr/0056-*`. List the `index.md`, `invariants.md` and roadmap rows in `docs/research/quotas/rows-to-add.md`.

## Definition of done

Survey; proposed ADRs for decisions 1 to 8 (merged or split with reasons); `plan.md`; review and the check of fixes; branch pushed. Final report: each ADR's recommendation in one line; the defaults for an untrusted room in one line; the tensions; the questions for the architect, first among them who pays for what a service creates for a client.
