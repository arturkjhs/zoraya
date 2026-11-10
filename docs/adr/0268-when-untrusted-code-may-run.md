# ADR-0268: When untrusted code may run: the named tests that end the refusal of `room new --untrusted`

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0001, ADR-0056, ADR-0104, ADR-0172, ADR-0224, ADR-0260, ADR-0266, ADR-0267, ADR-0269, ADR-0270
- **Evidence:** CP-07, CP-14, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, research-log.md#verification-phase-findings, quotas/survey.md#zoraya-today

## Context
Since stage 4 the rule has been: no untrusted code runs until quotas are accepted and implemented (ADR-0056's
Context). `room new --untrusted` is refused with a hint naming ADR-0056 (`docs/design.md`, "Rooms";
`quotas/survey.md#zoraya-today`). "Implemented" has no precise end: this ADR gives it one, as a list of accepted ADRs,
implemented tasks and named tests, so that the refusal is removed by a check and not by a judgement made once.

## Decision drivers
1. The rule must have an end a later agent can verify without the architect's memory (ADR-0001's checkable claims).
2. Each harm the task names must be shown bounded by a test that runs in CI: the fork bomb, the memory hog, the CPU
   spinner next to a terminal, the disk filler, and floods of services (CP-14, CP-07).
3. Quotas are not the only precondition of untrusted rooms: ADR-0172's own untrusted-room invariants wait for them.
4. A wrong opening is costly and a late one is cheap: when unsure, keep the refusal.

## Considered options

### Option A: No gate; untrusted code runs wherever the operator chooses (Linux containers, the baseline)
- **Description:** Isolation and limits are configured per deployment; the system does not refuse.
- **Evidence for:** No waiting; the person decides.
- **Evidence against:** Driver 4: the person cannot know the limits are complete; the fork bomb measured in stage 6
  shows they are not (`quotas/survey.md#zoraya-today`).
- **Cost:** None.
- **Precedent:** Linux, where cgroups, namespaces and seccomp are each optional.

### Option B: Acceptance of the ADRs ends the rule
- **Description:** When ADR-0260 to ADR-0270 are accepted, the refusal goes.
- **Evidence for:** Simple.
- **Evidence against:** An accepted ADR is not code; driver 2 fails.
- **Cost:** None.
- **Precedent:** none.

### Option C: Proof (seL4's verified isolation)
- **Description:** Open only once isolation and resource bounds are proved.
- **Evidence for:** The strongest guarantee; seL4 shows it can be done for a kernel
  (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** Zoraya's kernel is not verified and is not designed for it; the rule would never end.
- **Cost:** *(estimate)* Years.
- **Precedent:** seL4.

### Option D: Named tests green on `main`'s full CI, every invariant checked, and the architect's go-ahead
- **Description:** A list of conditions in this ADR, mirrored as a file the CI reads; the refusal is removed in one
  commit only when all hold.
- **Evidence for:** Drivers 1 to 4.
- **Evidence against:** Tests show the cases tested, not every case; a kernel bug a hostile program finds is not a
  quota question and is not closed by this list (the list includes fuzzing of the decoders, not more). All of it must
  be done before anything opens.
- **Cost:** *(estimate)* The tests are each task's own; the gate file and its check, 100 lines.
- **Precedent:** release criteria.

### Option E: Open in stages, each behind its own tests (D, cut by what the room may hold)
- **Description:** Option D's conditions for what an untrusted room can hold without the network (files, memory,
  processes, CPU, the GUI), and `+net` and `+listen` words in an untrusted room refused until the network's own gate
  tests (the `netd` account flood) are in the list and green. Fuchsia's job policies remove whole kinds of object
  from a job in the same spirit (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence for:** Drivers 1 to 4, and the network (stage 8) need not be finished for untrusted rooms to be usable
  for local work; each stage's opening is as checkable as D's.
- **Evidence against:** Two openings to track; an agent that needs the network waits for the second.
- **Cost:** *(estimate)* D's, plus one refusal of `+net` in untrusted lines, 20 lines.
- **Precedent:** none here; Fuchsia's per-type policy is the nearest.

### Option F: D plus a recorded review of the system call surface
- **Description:** D's conditions, and a report by an independent reviewer (the `opus-review` agent) of every system
  call an untrusted program can make, filed next to Q7's task.
- **Evidence for:** Covers what tests miss: a reviewer reads paths no test reaches.
- **Evidence against:** A report is a judgement, not a check; driver 1 wants an end a later agent can verify.
- **Cost:** *(estimate)* One review.
- **Precedent:** TASK-01's verification passes, which found unsupported claims in every slice (`research-log.md#verification-phase-findings`).

## Recommendation
**Recommendation (not a decision): Option E**, with Option F's review as part of Q7's task (point 6). `room new
--untrusted` stops being refused when all of the following hold on one commit of `main`, and the architect gives the
go-ahead for the task (Q7 in `quotas/plan.md`) that removes the refusal:

1. **Accepted:** ADR-0260 to ADR-0267, ADR-0269 and ADR-0270, as recommended or as the architect changes them.
2. **Implemented and checked:** every invariant of those ADRs has the status `checked: COMMAND` in
   `docs/adr/invariants.md` (none `not yet applicable`), except INV-0262-4 until point 7's second stage, and the
   invariants of ADR-0172 that wait for untrusted rooms (INV-0172-2's in-QEMU half, INV-0172-3) are checked too.
3. **The named tests,** each in `tools/untrusted-gate.txt`, run by `main`'s full CI (TCG runs and crash rounds
   included), on a 512 MiB guest unless a test says otherwise:
   - **fork bomb**: `:(){ :|:& };:` from `sh` in an untrusted room at its defaults stops at a limit of the room
     (processes or memory, whichever comes first: a POSIX process holds a 1 MiB heap step and `fork` charges a full
     copy); meanwhile a command in `home` starts and finishes within 2 s `[assumption]`; `room remove agent-1 --kill`
     brings the room's use to zero (INV-0261-6, INV-0263-1);
   - **many rooms**: four untrusted rooms, the most the keeper runs (ADR-0269), each with a fork bomb and a memory hog, while
     `home` starts processes and maps memory (INV-0261-8);
   - **memory hog**: `memory_create` and `malloc` loops in the room get `OverQuota` and `ENOMEM`; a sibling room maps
     memory to its own limit (INV-0261-4);
   - **CPU spinners**: two untrusted rooms at their defaults, spinning and bursting at period boundaries; keystrokes in
     a `home` terminal are echoed within INV-0264-3's bound, and the rooms together stay within INV-0264-4's;
   - **disk filler**: `dd` into `~/sandbox/agent-1` and into the room's `/tmp` stop at their limits; `home` still
     writes (INV-0265-3); a `sync` loop in the room leaves `home`'s `sync` within INV-0265-5's bound;
   - **service floods**: from the room, opening files and creating surfaces without end stop at the account's bounds
     while a `home` program does the same within its own (INV-0262-1);
   - **masked time**: INV-0270-1, over every call it lists and a room's exit;
   - **charged allocation**: INV-0261-5's lint and test;
   - **keeper**: INV-0269-1 to -3;
   - **hostile programs**: TASK-21's hostile cases (signals ignored, malformed broker and terminal messages) run from
     inside an untrusted room, and nothing outside it crashes;
   - **fuzzing**: a `fuzz/` target for each decoder an untrusted program reaches that has none yet (`spawn`'s
     request and the file and compositor protocols; `fuzz/` holds the volume walk, the raw image, the shell's
     parser, descriptions and the editor's keys today) runs 10 minutes `[assumption]` without a finding.
4. **The room's POSIX support is the room's:** the broker and `tmpd` beside each shell of an untrusted room run in the
   room's job and hold no handle the room's line does not grant (INV-0268-3). ADR-0104's broker trusts what its
   processes say of their pids, which is acceptable inside one room (they could already signal each other through
   their shared job) and not across rooms; with a broker per shell, no broker serves two rooms.
5. **The check.** `make check-untrusted-gate`, part of `make check`, fails if the refusal of `room new --untrusted` is
   absent from `zosh` and the panel while any test named in `tools/untrusted-gate.txt` is missing from the test map or
   from the full CI's steps, or any invariant of point 2 is not `checked:`. That the tests *passed* on the commit that
   removes the refusal follows from the merge rule (`CLAUDE.md`: merge only after the branch's full CI run is green,
   then `main`'s), since the gate tests are in that run (INV-0268-2). Removing a test from the file needs an ADR.
6. **The review.** A report by the `opus-review` agent of every system call and protocol message a program in an
   untrusted room can send, with its findings fixed or recorded, filed as `docs/tasks/TASK-NN-review.md` of Q7.
7. **The network, a second stage.** Until `tools/untrusted-gate.txt` also lists the `netd` account flood (connecting
   without end from the room stops at `--conns` while `home` connects, INV-0262-1 and INV-0262-4) and the fuzz target
   of the network protocol, and they are green, `room new --untrusted` and `room set` refuse `+net` and `+listen`
   words in an untrusted room's line: `room: agent-1: +net in an untrusted room needs the network's quota tests`.
   The same check enforces it.

**Tensions with accepted ADRs.** *ADR-0104:* its "Trust at stage 6" names target 2 as needing a kernel query or
spawning by the broker, and its revisit trigger "ADR-0056 accepted: the broker's trust assumptions become checks"
would read ADR-0260 to ADR-0270; point 4 answers it by confinement (a broker per shell, inside the room) instead of
checks. *ADR-0172:* "the broker and the session stay one" holds per shell; an untrusted room's sessions are inside its
job. *ADR-0224:* `+net` words are refused in untrusted lines until point 7's stage.

**What the gate does not claim.** It bounds what a program in an untrusted room can make the machine spend, and shows
the grants of ADR-0172 hold for the cases tested. It does not show the kernel free of bugs a hostile program could
exploit; that is the kernel's own review and fuzzing, of which point 3 requires only the decoders' part.

**Examples.** Before the gate:

```
zosh ~> room new agent-1 --untrusted +w ~/sandbox/agent-1
room: agent-1: untrusted rooms are not available yet
hint: they need the quotas of ADR-0260 to ADR-0270; see docs/adr/0268
```

After it, the same line creates the room with ADR-0266's defaults and `room` lists them.

### Effect of the vision
This is the door to the isolation direction: once it opens, an agent can be given a room and nothing else, and the
machine stays the person's. For the general-purpose base nothing changes until it opens.

## Consequences
- **Positive.** The rule "no untrusted code before quotas" has a checkable end; the refusal cannot vanish silently.
- **Negative.** The list is long and the network's part depends on stage 8; untrusted rooms wait for all of it.
- **Follow-up.** `tools/untrusted-gate.txt` and `make check-untrusted-gate` (Q1, kept current by each Q task); Q7.

## Invariants
- **INV-0268-1.** The refusal of `room new --untrusted` is present while any condition of points 2 and 3 is unmet,
  and the refusal of `+net` and `+listen` in untrusted lines while point 7's tests are not listed.
  *Check:* `make check-untrusted-gate`, which reads `tools/untrusted-gate.txt`, `tools/test-map.txt` and
  `docs/adr/invariants.md`, and greps the shell's and panel's sources for the refusal.
- **INV-0268-2.** Every test in `tools/untrusted-gate.txt` runs in the full CI. *Check:* the same target compares the
  file with the CI workflow's step list.
- **INV-0268-3.** The broker and `tmpd` beside a shell of an untrusted room are in the room's job and hold only handles
  the room's line grants, their own channels to that shell, and the room's `tmpd`. *Check:* host test of the shell's
  launch table for its broker in an untrusted room; in-QEMU test reading the broker's and `tmpd`'s `slots` against the
  room's record.

## Open questions
- Whether point 6's review should be repeated whenever a system call is added after the gate opens.

## Revisit triggers
- A harm found that an untrusted room can cause and no gate test covers: add the test, and the gate closes again
  until it passes.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The gate opens in two stages, first without `+net`. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
