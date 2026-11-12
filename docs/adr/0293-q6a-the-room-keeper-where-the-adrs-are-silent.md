# ADR-0293: Q6a, the room keeper where ADR-0266 and ADR-0269 are silent

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0052, ADR-0153, ADR-0172, ADR-0221, ADR-0261, ADR-0262, ADR-0263, ADR-0265, ADR-0266, ADR-0268, ADR-0269, ADR-0292
- **Evidence:** CP-05, CP-07, quotas/survey.md#genode-resource-trading, quotas/survey.md#linux-cgroups-v2

## Context
ADR-0269 says what the keeper is and which messages three kinds of channel may send. Building it (Q6a) needed answers
to things it leaves open: how a shell in a room gets its `Room(R)` channel, how a launcher says it is done with an
untrusted room, how a launcher reaches a restarted keeper, what `home` is to the keeper, when an idle room is let go,
who checks a limit against its parent, and how the shell learns that a line was refused. Each answer below was the
simplest, least surprising and most reversible one; none changes an accepted decision.

## Decision drivers
1. The keeper stays small and trusts no launcher with `LIMIT` (ADR-0269 point 1, INV-0269-2, CP-05).
2. One place knows the ceilings (CP-07, as ADR-0261 cites it): a limit above its parent is refused where the job is made or changed.
3. Console and desktop work the same (ADR-0269 driver 4).
4. Nothing here may weaken a refusal at the request or the creator-pays rule (ADR-0261).

## Considered options

### Option A: the smallest additions to the protocol, built
- **Description:** `Open` (request 7, `Manage` only) answered by `Opened` with a new `Room(R)` or `Usage` channel, so
  the panel can give each shell in a room its own; `Leave` (request 2) from a launcher that is done with an untrusted
  room (a lease: the other launcher may enter once the lease is given back and the room's processes have ended, or the
  holder's channel closes); `Keep` and `Forget` from the keeper to `init`, `Attach` from `init` to the keeper (the
  start-up channel of ADR-0269 point 5); a reconnect channel from `init` to each `Manage` holder, one byte in and a new
  `Manage` channel out, which a launcher uses once when its channel is gone. `home` is a room the keeper serves
  without a line: its launch jobs are sub-jobs of the session job with no limits, so the panel's launches into `home`
  are in the session job (the orchestrator's note: they were outside it until Q6a). A room with no processes, no lease
  and no launch for 5 s is let go (`IDLE_MS`), which is what lets a fifth untrusted room run after one has ended. The
  keeper checks every line against the parent's ceilings when it makes the job and at `Set`, also for a room that does
  not run; the shell checks syntax only, and `room set` writes the line first and puts the old one back if the keeper
  refuses. The shell learns of a refusal by reading the room's counters from `Usage` at the end of each line (a
  synchronous count, no race with the notice), prints at most one hint, and the keeper's `Refused` notices go to the
  channels for the panel's pills (Q6b).
- **Leases and restarts.** A lease is per `Manage` channel; `Leave` and a closed channel remove only their own, and a
  room is idle only with no lease. "Another launcher" is decided by the holder's kind (`Launcher`), never by the
  channel's slot, which is reused. `Keep` carries the room's trust (for every room job, not only the untrusted job) and
  the kind of its last holder; `init` replaces its copy when the holder changes. A resumed room counts as leased by
  that kind until a launch of that kind claims it, that kind sends `Leave`, or any `Manage` channel of that kind
  closes, or the room is stopped; there is no time limit, because a client that reconnects late (`zosh` rebinds before
  each prompt, but may be idle for long) must still find its room and not have it collected under its launch job; a resumed room that has processes and no known holder
  answers `BUSY`. `home` is refused `Stop` and `Set`: it is the session's terminals, not a room. A `Room(R)` or `Usage`
  channel cannot take the last four of the 64 channel places: `init`'s `Attach` of a reconnecting `Manage` holder is
  never dropped for want of one. The keeper writes `Keep` to `init` before it gives a launcher the launch job, and
  refuses the `Launch` if that write fails.
- **`init` holds a `LIMIT` duplicate of each job.** This is acceptable: `init` already holds `LIMIT` on the session
  job, so the duplicates give it no authority it lacks, and they go only to a restarted keeper, over the start-up
  channel `init` made.
- **Evidence for:** Drivers 1 to 4; the keeper is 700 lines with its host model; every rule has a test.
- **Evidence against:** Three requests and four `init` messages beyond ADR-0269's list; the lease is a convention
  (a launcher that never sends `Leave` and keeps its channel holds an idle room until its channel closes).
- **Cost:** *(estimate)* as built.
- **Precedent:** Genode's parent hands out sessions on request (`quotas/survey.md#genode-resource-trading`); a cgroup
  subtree is delegated by its owner the same way (`quotas/survey.md#linux-cgroups-v2`).

### Option B: no lease; busy while the room has processes
- **Description:** Admit the other launcher whenever the room has no process.
- **Evidence for:** No `Leave`.
- **Evidence against:** The console's shell that has entered a room and not yet run a command (a launch job and no
  process) would lose the room to the desktop between two lines, and its launch job would then be in a room another
  launcher uses (ADR-0269: one launcher at a time).
- **Cost:** *(estimate)* none.
- **Precedent:** none.

### Option C: the shell checks ceilings
- **Description:** `room new` and `room set` ask for the session's and the untrusted job's sizes and refuse above them
  before writing.
- **Evidence for:** The error appears before the file changes.
- **Evidence against:** Driver 2: two places hold the numbers; the keeper must check anyway (a file edited by hand).
- **Cost:** *(estimate)* a size request and 40 lines in the shell.
- **Precedent:** none.

## Recommendation
**Recommendation (not a decision): Option A, as built.** The invariants it serves are ADR-0261, -0263, -0266 and
-0269's, checked as their rows say.

## Consequences
- **Positive.** `home` and every room are under the session job through the one keeper; limits are applied and
  changed in one place; the console and the desktop use the same protocol.
- **Negative.** The console shell's broker and `tmpd` are `init`'s, in the session job, not in an untrusted room's job
  (INV-0268-3 is open for the console path); the panel's broker is in the room's launch job. A POSIX program's
  processes descend from the line's job, so they are in the room's whatever the broker's place. An untrusted room's
  `/tmp` (`--tmp`, a `tmpd` per shell in the room's job) is not built: the word is in the line and the record only.
  `+net` and `+listen` words are not read from a room's line yet (ADR-0268 point 7): the settings crate has no
  network words, and the gate's refusals stay in place.
- **Follow-up.** Q6b reads `Usage` and `Refused` for `top`, the pills and the settings page; Q7 decides ADR-0292's
  option B for the room directory's limits and builds the `tmpd` in the room's job.

## Invariants
None.

## Open questions
- Whether `IDLE_MS` (5 s) should be longer for a slow launcher under TCG.
- A resumed room's orphan lease has no time limit (it had 30 s in the first two rounds; round 3 removed it because
  the old holder's client may come back after any delay). It ends when a launch of the holder's kind claims the room, that
  kind sends `Leave`, ANY `Manage` channel of that kind closes (not only the old holder's: the keeper cannot tell which
  `zosh` held the old launch job), or the room is stopped (`room remove`, `Stop`). So a kind that never attaches
  again (a desktop session without a panel) keeps an empty orphaned room, which holds one of the four untrusted
  places and answers `BUSY` to the other kind, until `Stop`. Only trusted `init`, the shell and the panel hold
  `Manage`, so this is a nuisance and not an escalation. A holder that had already sent `Leave` before the keeper ended
  is still treated as holding until then (`Keep` does not say).
- `room set` of an untrusted line takes the words it lacks from the keeper's `Usage` of the running room and refuses for a
  stopped room; a request for the keeper's machine would let it fill them for a stopped one.
- Whether a launcher that crashes holding a launch job should have the room's processes ended (today they run on).
- Whether the keeper should ask `init` for the machine's free disk instead of assuming 8 GiB for the default of
  `--disk` when it fills a line in memory.

## Revisit triggers
- A third launcher (an agent supervisor).
- The first untrusted room used from the console: the console shell's broker and `tmpd` must then be in the room.

## Architect's decision

(Left empty for the architect.)
