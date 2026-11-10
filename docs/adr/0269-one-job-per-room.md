# ADR-0269: One job per room, held by a room keeper that every launcher asks

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0052, ADR-0053, ADR-0080, ADR-0097, ADR-0153, ADR-0168, ADR-0172, ADR-0221, ADR-0242, ADR-0261, ADR-0262, ADR-0263, ADR-0266, ADR-0267
- **Evidence:** CP-05, CP-07, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#genode-resource-trading, quotas/survey.md#zoraya-today

## Context
A room's job is per launcher today: the panel's job for a room and a shell's job for the same room are two jobs with
the same grants (ADR-0172 point 2 and its first open question; `quotas/survey.md#zoraya-today`). Harmless while a job
holds no limits; with ADR-0261, a room entered by two launchers would get its limits twice, and `room set` would
have to find every copy. A limit on a room needs one job for the room.

## Decision drivers
1. A room's limits bound everything run in the room, from any launcher (CP-07, ADR-0261).
2. Launchers stay as they are: they build launch tables from the room's grants (ADR-0153, ADR-0172), and nothing in
   them needs the person's whole authority to change (CP-05).
3. `room set` and `top` find the room's job in one place (ADR-0266, ADR-0267).
4. The console without a GUI works the same as the desktop.
5. Small: one more component at most, with a small protocol.

## Considered options

### Option A: Keep a job per launcher, and give each the room's limits (the status quo)
- **Description:** Every launcher's room job carries the line's limits.
- **Evidence for:** No new component.
- **Evidence against:** Driver 1 fails: two launchers, twice the limit; `room set` reaches only its own copy.
- **Cost:** None now.
- **Precedent:** Zoraya today.

### Option B: One manager owns the containment tree; others ask it (Linux's systemd, the baseline)
- **Description:** On Linux one manager owns the cgroup tree and delegates subtrees; a program that wants a scope
  asks the manager instead of writing the tree itself (`quotas/survey.md#linux-cgroups-v2`: delegation, and nothing
  a delegatee does escapes its parent's limits).
- **Evidence for:** Drivers 1, 3, 4; it is what a single owner of the tree gives.
- **Evidence against:** Linux's manager is large; here only the room level needs an owner.
- **Cost:** *(estimate)* As Option D.
- **Precedent:** systemd's slices and scopes `[outside evidence: systemd documentation, not fetched]`.

### Option C: The panel owns every room's job; shells ask the panel
- **Description:** The panel, which already launches into rooms (ADR-0168), creates each room's job; a shell it
  launched asks it for a launch job over a new channel.
- **Evidence for:** No new process; the panel reads `/config/rooms` already.
- **Evidence against:** Without a GUI there is no panel (driver 4); the console's shell would need its own path, and
  then two owners again. A panel restart would end every room's job held only by it.
- **Cost:** *(estimate)* 300 lines in the panel and the shell.
- **Precedent:** none here.

### Option D: A room keeper started by `init`, which owns every room's job (Genode's parent as the broker of resources)
- **Description:** A small process `rooms`, started by `init` in GUI and console modes alike, holds the session job
  and creates one job per room under it with the line's limits; a launcher asks it for a *launch job* in a room (a
  sub-job of the room's job without `LIMIT`) and launches in that as it does today. In Genode, the parent is the one
  place where resources are routed (`quotas/survey.md#genode-resource-trading`); Windows' nested jobs give the same
  single tree (`quotas/survey.md#windows-job-objects`).
- **Evidence for:** Drivers 1 to 5; it also settles ADR-0172's open question for trusted rooms.
- **Evidence against:** One more process and protocol; if it dies, new launches cannot find running rooms.
- **Cost:** *(estimate)* 500 lines with its protocol crate and host tests; one slot in every launcher.
- **Precedent:** systemd's role, at the size of one table.

## Recommendation
**Recommendation (not a decision): Option D.**

1. **The keeper.** `rooms`, started by `init` after `fsd` (ADR-0053's table), in `init`'s job, holding the session job
   (ADR-0261 point 10) with `CREATE_PROCESS` and `LIMIT`, a read connection to `/config/rooms`, and nothing else. At
   start it creates the *untrusted* job under the session with ADR-0266's ceilings, unless `init` hands it one (item 5). It trusts no launcher with `LIMIT`.
2. **Its protocol** (a `zoraya-proto-rooms` crate, ADR-0052's framing), over three kinds of channel:

   | Kind | Who holds it | Messages |
   |---|---|---|
   | `Manage` | the panel, the console's shell, the settings application (from the panel) | all below |
   | `Room(R)` | each shell launched in room R, and `top` run in it | `Launch { R }`, `Usage { R }`, and the `Refused` notices of R |
   | `Usage` | `top` run in `home` or a trusted room (from its shell) | `Usage { any }`, and `Refused` of any room |

   - `Launch { room }` → a job handle: a new sub-job of the room's job, with every right but `LIMIT`. The keeper
     creates the room's job at the first `Launch` (under the untrusted job for an untrusted room, under the session
     for a trusted one), with the limits of its line (ADR-0266), and keeps it while it has live processes or launch jobs. At most four untrusted rooms run at once (ADR-0262's carve); a fifth `Launch` gets `room: agent-5: four untrusted rooms are running`. An untrusted room is launched into by one `Manage` holder at a time: the first that asks, until the room's
     processes have ended; another gets `room: agent-1: running from the desktop` (or `from the console`). So the
     service accounts its launcher makes are the room's (ADR-0262).
   - `Set { room }` (`Manage` only): re-reads the line and applies its limits to the running room's job.
   - `Usage { room | all }` → each room's `job_get_usage` record with the service counts its launcher reported.
   - `Report { room, service, counts }` (`Manage` only): a launcher's forward of its accounts' counts and refusals.
   - `Stop { room }` (`Manage` only): `job_kill` on the room's job (`room remove NAME --kill`, the overview's End room).
   - `Refused { room, field, used, limit }`, sent by the keeper: on a room job's `OVER_QUOTA` (ADR-0263 point 3) or a
     forwarded account refusal.
3. **Who gets a channel.** `init` gives `Manage` to the panel and to the console's shell; the panel gives `Room(R)` to
   each shell it launches in room R and `Manage` to the settings application; a shell gives `top` a `Room(R)` in a room
   and a `Usage` channel elsewhere, as a designated slot of `top`'s description (ADR-0153's rule that only the system
   image's descriptions designate), and gives no keeper channel to any other command. So nothing inside a room can
   launch into another room, change limits, or stop rooms.
4. **Launchers.** The panel and `zosh` keep building launch tables from the room's grants as today (resolved against
   their own tables, ADR-0172 point 3); they ask the keeper for the job to launch in instead of `job_create`. A shell
   with no keeper channel (a test harness) launches in its own job, and refuses to enter an untrusted room
   (`room: agent-1: an untrusted room needs the room keeper`). The console's shell entering a trusted room shares the
   room's job with the panel's launches into it.
5. **If the keeper dies,** running rooms keep their jobs and limits. At the untrusted job's creation and at each room job's, the keeper hands `init` a duplicate (with `LIMIT`) over its start-up channel; `init` restarts the keeper at most three times (ADR-0080's
   bound) and gives the new one the untrusted job and the table of room names and jobs, so `Stop`, `Set` and `Usage` keep working, and a
   room never gets a second job. Launchers re-request their channels from `init`'s services channel (ADR-0221 item 6's
   shape).

**Tensions with accepted ADRs.** *ADR-0172 point 2:* the room's job moves from each launcher to the keeper, and an
untrusted room is launched into from one launcher at a time; its open question on one job per room is answered.
*ADR-0053* and *ADR-0097:* `init` starts one more process, holds room-job duplicates, and hands out channels.
*ADR-0168* and *ADR-0242:* the panel's launch path asks the keeper; shells it launches get one more slot, and
INV-0168-1's "nothing else" gains the `Room(R)` channel. *ADR-0153:* `top`'s description designates a keeper channel.

**Examples.** Nothing changes in what a person types. What changes is visible:

```
zosh ~> room project                 # from the console's shell, while the panel has a terminal in project
project ~/project> top project
JOB                       MEM     PROCS  CPU
project                   61M     5      12%
  term (panel)            2M      1      0%
  zosh --room project     3M      1      0%
  make                    53M     2      12%
  zosh line (console)     3M      1      0%
zosh ~> room agent-1                 # while the desktop has a terminal in agent-1
room: agent-1: running from the desktop
hint: use its terminal, or end it first: room remove agent-1 --kill
```

The console's shell and the desktop's terminal launch into the one `project` job, under one set of limits.

### Effect of the vision
For agents, a room is one bound whoever launched into it, only trusted launchers can ask, and a room's own programs
can see only their room. For the general-purpose base, trusted rooms gain one job each too, which is what ADR-0172's
overview already assumes.

## Consequences
- **Positive.** One job, one limit, one place to read and change it, in both modes.
- **Negative.** A new process and three channel kinds; an untrusted room is usable from one launcher at a time.
- **Follow-up.** The keeper and its protocol (Q6), the panel's and shell's launch path.

## Invariants
- **INV-0269-1.** At any time each room has at most one job, and every process launched in the room by any launcher is
  in it; an untrusted room is launched into by at most one `Manage` holder at a time. *Check:* in-QEMU GUI test:
  launch a terminal in `project` from the dock, enter `project` from the console's shell, run a command in each, and
  compare their jobs' ancestry through the keeper's `Usage`; then do the same with `agent-1` and see the console's
  `room agent-1` refused.
- **INV-0269-2.** No launch job the keeper hands out carries `LIMIT`; a `Room(R)` channel is refused every message but
  `Launch { R }` and `Usage { R }`; and no program in a room but its shells and `top` holds a keeper channel.
  *Check:* host test of the keeper's protocol per channel kind; in-QEMU test reading a room program's `slots`.
- **INV-0269-3.** After a keeper restart, `Stop` ends a room started before it. *Check:* in-QEMU test killing the
  keeper with a room running, then `room remove NAME --kill`.

## Open questions
- Whether the keeper should also own the per-room service accounts, which would need it to hold the grants' roots
  (more authority than it has now), and would let an untrusted room be used from two launchers at once.

## Revisit triggers
- A third launcher (an agent supervisor service) that would multiply per-launcher accounts.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The rooms keeper process, and one launcher at a time per untrusted room. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
