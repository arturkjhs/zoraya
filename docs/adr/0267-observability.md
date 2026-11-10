# ADR-0267: Observability: `top` shows each room's and job's use against its limits, and the settings window edits them

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0153, ADR-0155, ADR-0157, ADR-0169, ADR-0172, ADR-0261, ADR-0262, ADR-0263, ADR-0265, ADR-0266, ADR-0269
- **Evidence:** CP-23, DM-034, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#apples-memory-limits-and-jetsam, quotas/survey.md#fuchsia-jobs-and-memory-attribution, quotas/survey.md#lessons-for-zoraya

## Context
A limit a person cannot see used is a limit they set blind. ADR-0261 gives `job_get_usage` (limit, used, peak,
refused, and the memory split); ADR-0262 gives each service's account counts; ADR-0265 a directory's limit record.
This ADR says how a person reads them: a command in `zosh`, the desktop's room cards and hover cards, and the
settings window (ADR-0169). Every element has its text (ADR-0172).

## Decision drivers
1. One view per room, the unit a person named (ADR-0172), with the jobs below it on request.
2. The text is the truth (CP-23): the desktop shows what the command prints, in the same words.
3. Say which limit, not only that one was hit (`quotas/survey.md#lessons-for-zoraya`).
4. Cheap: reading use must not cost the machine more than it measures; once a second at most.
5. Calm by default: nothing moves on screen until a limit is near (ADR-0172's rules).

## Considered options

### Option A: Files of counters per group (cgroups' interface files, the Linux baseline)
- **Description:** `memory.current`, `memory.events`, `cpu.stat`, `pids.current` per group, read with `cat`; `top`
  read them (`quotas/survey.md#linux-cgroups-v2`), as does `systemd-cgtop` `[outside evidence: systemd documentation, not fetched]`.
- **Evidence for:** Any tool can read them; nothing new to learn for a Unix user.
- **Evidence against:** Zoraya has no global namespace for such files (CP-23's typed records instead); a person must
  know which file to read.
- **Cost:** *(estimate)* A pseudo-filesystem service.
- **Precedent:** Linux.

### Option B: A query and violation records for a supervisor (Windows job objects)
- **Description:** A supervisor queries accounting and learns which limit was violated
  (`quotas/survey.md#windows-job-objects`); Task Manager shows it.
- **Evidence for:** Precise; the violation names the limit.
- **Evidence against:** For programs, not for a person, unless a tool is built on it, which is Option D.
- **Cost:** *(estimate)* Covered by `job_get_usage`.
- **Precedent:** Windows.

### Option C: Attribution reports only (Fuchsia's `ps`, Apple's jetsam reports)
- **Description:** A report per process grouped by job, with private, shared and proportional bytes
  (`quotas/survey.md#fuchsia-jobs-and-memory-attribution`), or a report after a kill
  (`quotas/survey.md#apples-memory-limits-and-jetsam`).
- **Evidence for:** Detailed per process.
- **Evidence against:** No limit to compare with; the Fuchsia page admits miscounting; a report after the fact.
- **Cost:** *(estimate)* A listing tool.
- **Precedent:** Fuchsia, iOS.

### Option D: `top` by room, from the room keeper; the same rows on the desktop
- **Description:** A native command `top` asks the room keeper (ADR-0269) for each room's usage, which the keeper reads with `job_get_usage` on the room jobs and joins with the service counts the launchers report (ADR-0269's `Report`), and prints one row per room, `USED of
  LIMIT` per word; `top ROOM` adds the room's job tree. The desktop's cards and the settings window show the same rows.
- **Evidence for:** Drivers 1 to 5; it is Option B's data in a person's words.
- **Evidence against:** Needs the room keeper's channel; per-process detail is coarser than Option C.
- **Cost:** *(estimate)* 400 lines for `top`, 200 in the panel, a settings page.
- **Precedent:** a per-group view on top of Option A's files, as `systemd-cgtop` gives `[outside evidence: not fetched]`.

## Recommendation
**Recommendation (not a decision): Option D.**

1. **`top`**, a native program in ADR-0155's style whose result is its table:
   - `top`: one row per room, then `session`: `ROOM MEM CPU PROCS DISK CONNS`, each `USED` for a field with no limit
     of the room's own, `USED of LIMIT` otherwise; `CPU` is the share of the last second. A trailer per room in
     parentheses names refusals since the room started and how often it was parked.
   - `top ROOM`: the room's job tree with each job's memory, processes and CPU, and the program each job's first
     process runs, indented by depth; held memory (ADR-0261 point 9) on its own line.
   - `top --session`: the kernel reserve, `init`'s job, the session's ceiling and use, and each service with its
     accounts' counts against their bounds (ADR-0262).
   - `--watch` reprints every second until Ctrl-C (in a terminal); `--once` is the default.
   - Its data comes from the room keeper (ADR-0269) over the channel its shell gives it as a designated slot: `Usage`
     in `home` and trusted rooms (every room), `Room(R)` in room R (that room only). Without one, `top` prints `top:
     no room keeper` and exits 1. Usage is read at most once a second (driver 4).
2. **In `room`** (ADR-0266), each limit word is followed by its use when the room is running: `--mem 512M (498M)`.
3. **On the desktop**, from the panel's once-a-second `Usage` over its `Manage` channel (it already reads `/config/rooms`
   every second, `docs/design.md`):
   - the room's card in the rooms overview: one line per word, `mem 498M of 512M`, with a neutral bar (no colour,
     ADR-0172 point 7) and the room's `top` line under it in show-commands;
   - the hover card of a window: the same lines after the grants;
   - the badge and dock chip carry a pill only at 90% or more (ADR-0263 point 7).
4. **The settings window** (ADR-0169) gains a page `Rooms` listing each room with its limit words as fields (sizes and
   percentages), its use beside each, and the `room set` line under the field being edited; on Apply it writes the line through the rooms crate (ADR-0157's one
   writer, the same code `room set` uses) and sends `Set` over the `Manage` channel the panel gives it (ADR-0269); an
   untrusted room's fields cannot be emptied.
5. **For programs.** `job_get_usage` (ADR-0261) and the `OVER_QUOTA` signal (ADR-0263), which only the keeper reads on
   room jobs; the keeper's `Usage` and `Refused` messages for launchers and `top`. No pressure signal yet (DM-034).

**Tensions with accepted ADRs.** *ADR-0169:* the settings window gains a page whose Apply also talks to the room
keeper, not only to `/config`. *ADR-0153:* `top`'s description designates a keeper channel. *ADR-0172 point 5:* the
overview's cards and the hover card gain the usage lines.

**Examples.**

```
zosh ~> top                       # on a 2 GiB guest
ROOM      MEM             CPU          PROCS     DISK          CONNS
home      41M             4%           12        -             -
project   12M             0%           3         -             -
agent-1   498M of 512M    25% of 25%   14 of 64  1.2G of 2G    3 of 16   (refused 3 mem; parked 9/s)
session   551M of 1.4G    29%          29
zosh ~> top agent-1
JOB                       MEM     PROCS  CPU
agent-1                   498M    14     25%
  term                    2M      1      0%
  zosh --room agent-1     3M      1      0%
  broker, tmpd            19M     2      0%
    make -j4              464M    9      24%
      cc (4 jobs)         152M    4      6%
  top agent-1             1M      1      1%
  held by others          9M
zosh ~> top --session
kernel reserve   127M
init             1.9G
  services       29M   fsd 9 of 63 accounts; netd 2 scopes; compositor 5 clients
  session        551M of 1.4G
```

### Effect of the vision
For agents, a supervisor (a person or a program with the room keeper's channel) sees at once which room is near which
wall. For the general-purpose base, `top` is a familiar command that shows the session's use, with rooms as its rows.

## Consequences
- **Positive.** One table in the shell and on the desktop; every limit visible with its use; settings edits through
  the same command.
- **Negative.** Per-process detail is coarse (by job); the panel reads usage every second.
- **Follow-up.** `top`, the panel's readings, the settings page (Q6).

## Invariants
- **INV-0267-1.** For every room, `top`'s row, `room`'s words and the compositor's hover card show the same limits and
  the same use from one reading. *Check:* host test rendering the three from one usage record.
- **INV-0267-2.** `top` reads each room's usage at most once a second under `--watch`. *Check:* host test of the
  command's loop with a fake clock and a counting room keeper.

## Open questions
- Whether `top ROOM` should show per-process memory too (the kernel knows per process; jobs make it redundant for
  POSIX processes, each in its own job).

## Revisit triggers
- A pressure signal (DM-034): `top` would show pressure beside use.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
