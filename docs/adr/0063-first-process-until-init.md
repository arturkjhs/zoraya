# ADR-0063: The shell as first process, until `init`

- **Status:** superseded by ADR-0053
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0043, ADR-0062
- **Evidence:** CP-01, alternatives-postmortem.md#hubris

## Context
The normal kernel runs `hello` and `parent`, then powers off. ADR-0053, proposed on `adr/stage-4-design`, will have the kernel start `init` alone, but it needs channels and services. A shell is wanted now. This is a temporary decision that ADR-0053 replaces when accepted.

## Decision drivers
1. A person boots to a usable prompt.
2. One first process, as ADR-0053 plans, so the change is small.
3. No ambient authority (CP-01): the shell holds only what ADR-0043 and ADR-0062 give.

## Considered options

### Option A: A kernel-run script of programs (rc style, Linux baseline)
- **Description:** The kernel starts a fixed list in turn, as the stage 3 kernel does.
- **Evidence for:** It is what exists.
- **Evidence against:** No person in the loop.
- **Cost:** None.
- **Precedent:** Stage 3; a Linux `init=` script is the nearest baseline.

### Option B: The kernel starts the shell directly
- **Description:** One process, from the ramdisk, with the startup handles of ADR-0043 and `Power`.
- **Evidence for:** Drivers 1 to 3.
- **Evidence against:** The shell is a single point of failure, and it is doing `init`'s job.
- **Cost:** *(estimate)* 15 lines.
- **Precedent:** Hubris has a fixed task set at build time (`alternatives-postmortem.md#hubris`); ADR-0053's plan.

### Option C: Build a small `init` now
- **Description:** `init` starts the shell and restarts it if it dies.
- **Evidence for:** Closer to the end state, and a crash is survivable.
- **Evidence against:** It decides ADR-0053's content before that is accepted, for one child.
- **Cost:** *(estimate)* 80 lines of premature design.
- **Precedent:** Unix `init`.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision. It is temporary: ADR-0053 replaces it when accepted.

- **Start.** After its boot checks the normal kernel starts `zosh` from the ramdisk with ADR-0043's slots and `POWER` at slot 4, and waits for it.
- **If it ends,** the kernel reports the record and powers off.
- **Stage 3's boot checks.** `hello` and `parent` run from the shell in `make test-boot`, not from the kernel. `make run` no longer exits by itself.
- **The test kernel** starts programs as before.

## Consequences
- **Positive.** A prompt on boot.
- **Negative.** `make test-boot` needs scripted input.
- **Follow-up.** ADR-0053 replaces this ADR.

## Invariants
- **INV-0063-1.** The normal kernel starts exactly one process. *Check:* the scripted boot test shows `zosh` first and the kernel starts no other.
- **INV-0063-2.** The kernel powers off when the first process ends. *Check:* scripted serial test ending the shell with `exit`.

## Open questions
- As of TASK-12: INV-0063-2 has no test. `init` turns the machine off when the shell ends, so nothing ends the first process, and the kernel's path for that is checked by review only.

## Revisit triggers
- ADR-0053 accepted.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.

Superseded by ADR-0053 when TASK-12 built `init`, as that task instructs, 2026-11-01.
