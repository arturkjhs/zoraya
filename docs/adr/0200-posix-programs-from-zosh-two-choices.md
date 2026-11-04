# ADR-0200: Running POSIX programs from `zosh`: where trusted descriptions live, and stopping mixed lines

- **Status:** proposed
- **Date:** 2026-11-04
- **Depends on:** ADR-0153, ADR-0156, ADR-0158
- **Evidence:** CP-01, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

TASK-18 step 10 built ADR-0153 rule 10: `zosh` runs a program of the volume that lacks the native mark through the
broker, with the toolchain set and what a trusted description designates. Two parts of the accepted ADRs could not be
built as written with what exists, and the code took the least committing option in each; this ADR proposes them for
the architect's decision (`CLAUDE.md`: a deviation needs a proposed ADR).

1. **Where trusted descriptions live.** Rule 10 says a POSIX program's description is trusted only from
   `/zoraya/commands/` in the ramdisk, each naming the program path it applies to. No call reads a ramdisk entry
   other than a program image (ADR-0156's open questions): every description `zosh` uses today is compiled into it from
   `user/describe/` and also kept in the ramdisk, unread. The step compiles the descriptions of `tcc`, `make` and
   `dash` (`user/describe/posix/`, with `program PATH`) into `zosh` the same way. Either way they come only from the
   system image (CP-01: authority only from what the system gave), and `grants::trusted_description` applies one only
   to the path it names (INV-0153-6).
2. **Ctrl-Z of a line that mixes native and POSIX programs.** ADR-0158 rule 4 stops a native job with `job_suspend`
   and asks the broker to stop a POSIX element, which applies each process's stop disposition, saying
   `[N] did not stop: LINE` when one does not stop. A line's native elements are spawned directly into the line's job,
   and there is no call that suspends one process; `job_suspend` of the line's job also stops its POSIX elements
   whatever their dispositions. The step sends `SIGTSTP` through the broker when every element is a POSIX program, and
   uses `job_suspend` of the line's job when any element is native.

## Decision drivers
1. Trust comes only from the system image (CP-01; ADR-0153's Decision on descriptions as authority).
2. No new kernel call or ramdisk protocol for TASK-18; the step must not change the kernel.
3. Ctrl-Z must stop the line the person sees in front, every process of it (INV-0158-1).
4. A POSIX program that handles or ignores `SIGTSTP` (a shell, an editor later) should be able to, as on Unix.

## Considered options

### Option A: As built (descriptions compiled into `zosh`; `job_suspend` for mixed lines)
- **Description:** Item 1 as the step built it; item 2 as built.
- **Evidence for:** the same trust root (the image) as rule 10 (CP-01); no new call; mixed lines always stop.
- **Evidence against:** a description change needs a new `zosh`, as for every other description; a POSIX element of
  a mixed line cannot catch the stop.
- **Cost:** none beyond the step (estimate).
- **Precedent:** Fuchsia components carry their manifests in packages of the system image, read by the component
  manager, not by the component (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

### Option B: As ADR-0153 and ADR-0158 say (POSIX baseline for item 2)
- **Description:** a ramdisk read call or protocol so that `zosh` reads `/zoraya/commands/NAME.cmd`; and for a mixed
  line, `SIGTSTP` to the POSIX elements' groups as a POSIX shell does to its process group, plus some way to stop the
  native elements alone.
- **Evidence for:** the text of the accepted ADRs; POSIX shells stop a job by signalling its group.
- **Evidence against:** a new kernel or service interface (driver 2); native elements cannot be stopped alone today.
- **Cost:** a ramdisk read call (kernel, ADR) and a per-element job for native elements (estimate: a day each).
- **Precedent:** Unix `tcsetpgrp`/`kill(-pgid, SIGTSTP)`.

### Option C: Each native element in a job of its own, POSIX elements through the broker
- **Description:** item 1 as in A; for item 2 `zosh` spawns each native element into a sub-job of the line's job, so
  that a mixed line is stopped element by element: `job_suspend` for each native sub-job, `SIGTSTP` through the
  broker for each POSIX one, and `[N] did not stop` when one does not.
- **Evidence for:** ADR-0158 rule 4 exactly, with no kernel change; Zircon's job tree is the model (CP-01).
- **Evidence against:** one more job per element; a short window in which some elements are stopped and others not.
- **Cost:** a few hours in `zosh` (estimate).
- **Precedent:** Zircon jobs (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

## Recommendation

Recommended: **Option A for item 1 and Option C for item 2.** It is a recommendation, not a decision. Item 1 changes
where trusted bytes are kept, not who supplies them, and matches how every other description is used. Item 2's Option
C meets ADR-0158 rule 4 without a kernel change; until the architect decides, the code stays at Option A, the least
committing.

## Consequences
- **Positive.** ADR-0153's trust rule holds as built; Ctrl-Z of all-POSIX lines honours dispositions.
- **Negative.** With A for item 2, a POSIX element of a mixed line cannot handle Ctrl-Z.
- **Follow-up.** With C: per-element sub-jobs in `zosh`, and a test of a mixed line whose POSIX element ignores
  `SIGTSTP`.

## Invariants
- **INV-0200-1.** A POSIX program's description applies only if it is compiled into `zosh` and names the path the
  command resolved to. *Check:* the host test of `grants::trusted_description` (INV-0153-6's).
- **INV-0200-2.** Ctrl-Z of a line whose elements are all POSIX programs stops it only through the broker, and says
  `[N] did not stop: LINE` when an element's process job is not suspended within half a second. *Check:* an in-QEMU
  test with a POSIX program that ignores `SIGTSTP`.

## Open questions
- Whether a ramdisk read call is wanted for other reasons (help pages, settings defaults); then item 1 moves to it.

## Revisit triggers
- A description must change without a new `zosh`.
- A POSIX program used from `zosh` handles `SIGTSTP` (an interactive one, after ADR-0105 item 9's revisit).

## Architect's decision
