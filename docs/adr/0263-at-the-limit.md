# ADR-0263: At the limit: a refusal with its own status, a signal on the job, and a hint the person can act on

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0004, ADR-0006, ADR-0032, ADR-0046, ADR-0056, ADR-0100, ADR-0102, ADR-0153, ADR-0155, ADR-0160, ADR-0172, ADR-0225, ADR-0261, ADR-0262, ADR-0264, ADR-0266, ADR-0267, ADR-0269
- **Evidence:** CP-14, DM-030, DM-034, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#apples-memory-limits-and-jetsam, quotas/survey.md#genode-resource-trading, quotas/survey.md#storage, quotas/survey.md#lessons-for-zoraya

## Context
ADR-0261 refuses a charge that would pass a limit, with no effect. This ADR settles what the refusal is called, how a
supervisor learns of it, what a POSIX program sees, and what the person sees in `zosh` and on the desktop. ADR-0056
proposed a status `QuotaExceeded` and kept `ExitReason::Limit` (ADR-0004) reserved for lazy commit. Zoraya's statuses
today separate `NoMemory` (the machine is out) from `LimitReached` (a fixed bound per object, such as a full request
queue, ADR-0046).

## Decision drivers
1. The failure is a value at the request, never an asynchronous kill (CP-14, DM-030).
2. A supervisor must tell "this room is full" from "the machine is full" and from "this object is full": they need
   different actions (raise the room, end something else, wait) (`quotas/survey.md#lessons-for-zoraya`).
3. POSIX programs must see the `errno` they already handle for the same situation.
4. Every element has its text (ADR-0172's rules): the person sees which limit, its value, and the line that changes it.
5. Calm by default (ADR-0172): no repeated alarms; colour is for rooms only.

## Considered options

### Option A: Kill inside the limited group (Linux `memory.max` with the OOM killer)
- **Description:** At the limit, try to reclaim, then kill a process of the group or the whole group
  (`quotas/survey.md#linux-cgroups-v2`).
- **Evidence for:** The program never sees an error it may mishandle; `memory.oom.group` ends a workload cleanly.
- **Evidence against:** Asynchronous, and the victim may not be the allocator (DM-030); oomd exists because the
  kernel's kill comes late (`quotas/survey.md#linux-cgroups-v2`). With eager commit there is always a caller to tell.
- **Cost:** *(estimate)* A victim policy and a kill path.
- **Precedent:** Linux.

### Option B: Notice, then kill (Apple jetsam)
- **Description:** A low-memory notice, then termination with a reason in a report
  (`quotas/survey.md#apples-memory-limits-and-jetsam`).
- **Evidence for:** Gives programs a chance to shed caches; the reason is recorded.
- **Evidence against:** The page itself says the kill looks like a crash to the person; a notice is DM-034's missing
  signal, not a bound.
- **Cost:** *(estimate)* A pressure signal and a kill policy.
- **Precedent:** iOS.

### Option C: Block the call until the person raises the limit (a pause, Genode's resource request in its blocking form)
- **Description:** The charge waits; the room's chip shows that it is waiting; the person raises the limit or ends
  the room. Genode's resource request lets a parent decide (`quotas/survey.md#genode-resource-trading`).
- **Evidence for:** No program sees an error; the person decides, which suits an agent that should not fail silently.
- **Evidence against:** A blocked allocation can hold a lock another room needs (a service waiting on a client); a
  program with a deadline waits past it; a room with no person watching hangs. Windows' notification limits, the
  closest relative, let the job continue instead (`quotas/survey.md#windows-job-objects`).
- **Cost:** *(estimate)* A wait queue per job and a timeout policy.
- **Precedent:** Genode's resource requests (asynchronous, may be ignored).

### Option D: Refuse with `NoMemory` (no new status)
- **Description:** The existing status for every memory refusal.
- **Evidence for:** No ABI change; `libposix` maps it to `ENOMEM` already.
- **Evidence against:** Driver 2: a supervisor cannot tell the room from the machine.
- **Cost:** None.
- **Precedent:** Zoraya today: every refusal of memory is `NoMemory`.

### Option E: Refuse with a new status `OverQuota`, assert a job signal, show the limit
- **Description:** The call returns `OverQuota` and has no effect; the limited job whose limit refused it asserts a
  signal; the launcher that holds the job shows the person which limit, with the line that raises it.
- **Evidence for:** Drivers 1 to 5. cgroups' `memory.events` counts the same events per group, and Windows' violation
  query names the limit that was hit (`quotas/survey.md#linux-cgroups-v2`, `quotas/survey.md#windows-job-objects`).
- **Evidence against:** A new status every native caller must handle; programs that treat any failure as fatal still
  fail, but at their own call and with a reason.
- **Cost:** *(estimate)* One status, one signal bit, a `libposix` table row per call, text in `zosh` and the panel.
- **Precedent:** `EDQUOT` for disk quotas (`quotas/survey.md#storage`), distinct from `ENOSPC`.

## Recommendation
**Recommendation (not a decision): Option E.**

1. **The status.** `OverQuota`, new in `zoraya-abi`'s `Status`; its word in ADR-0155's table is `over quota`. It
   means: a limit of a job or of an account in a service refused this request, and nothing changed. The kernel returns
   with it, in x1, the field that refused (`QuotaField::Memory`, `Processes`; ADR-0032's value register, otherwise 0
   on failure), and `fsd` and the compositor carry the same field in their reply (`Disk`, `Account`). `NoMemory` keeps
   meaning the machine (or the kernel reserve) is out, `LimitReached` a fixed bound of one object; `fsd`'s
   `TooManyOpen` stays as it is for its account bound (ADR-0262). ADR-0056's and ADR-0102's name `QuotaExceeded` is
   shortened; the meaning is the same.
2. **Nothing is killed, suspended or throttled for memory, processes or disk.** CPU is a ceiling the scheduler
   applies by not running the job's threads (ADR-0264); that is the only throttle. `ExitReason::Limit` stays reserved
   for a charge with no caller (lazy commit, never in stage 8).
3. **The signal and who hears it.** A job gains `job_signals::OVER_QUOTA`, asserted on the limited job whose limit
   refused a charge. Room jobs are held by the room keeper alone (ADR-0269), so the keeper is the one subscriber: it
   reads `job_get_usage` (whose `refused` counts say which field, ADR-0261 point 11), clears the signal with that call's
   `CLEAR` flag (ADR-0261 point 11), and sends `Refused { room, field, used, limit }` on every keeper channel that watches the room: the
   panel's, the console's shell's and the room's own shells' (ADR-0269's room-bound channel). Services report account
   refusals to the account's launcher, which forwards them to the keeper as the same message (ADR-0262).
4. **POSIX** (`libposix`), by the call and the field, as rows added to ADR-0100 item 9's table (one `errno` per status
   and field for each call) and to ADR-0225 item 11's:

   | Refused | Calls | `errno` | Why this one |
   |---|---|---|---|
   | memory | `malloc`, `mmap`, `brk`, `posix_memalign` | `ENOMEM` | what they return when memory is short |
   | processes | `fork`, `vfork`, `posix_spawn` | `EAGAIN` | as for `RLIMIT_NPROC` and `pids.max` (`quotas/survey.md#linux-cgroups-v2`) |
   | memory | `fork`, `posix_spawn` | `ENOMEM` | as ADR-0102 item 6 already maps `NoMemory` |
   | memory | `pipe`, `socket`, `open` | `ENOMEM` | allowed by POSIX for these |
   | disk | `write`, `creat`, `open` with `O_CREAT`, `mkdir`, `rename` | `EDQUOT` | the disk-quota error, not `ENOSPC` |
   | account: files (`TooManyOpen`) | `open` | `ENFILE` | ADR-0100's existing row, unchanged |
   | account: connections | `connect`, `socket` | `ENOBUFS` | the sockets' "no buffer space"; a new row of ADR-0225 item 11 |

   `getrlimit` keeps reporting ADR-0100 item 6's fixed limits: a room's limits are not a process's, and a program
   that sizes itself from `RLIMIT_AS` or `RLIMIT_NPROC` would otherwise see a number it cannot raise.
5. **What a command prints.** A native command reports `NAME: SUBJECT: over quota`, the status word alone: it holds no
   handle on its room's job and does not know the numbers. The numbers are the shell's hint's.
6. **What `zosh` adds.** After a line ends, if the keeper sent `Refused` for the shell's room during the line, the
   shell prints one hint for the first refused field: `hint: agent-1 is at its memory limit (256M of 256M)`, then, if
   the limit can still be raised, the line that raises it, `room set agent-1 --mem 512M` (double the limit, capped at
   what the room's parent allows), prefixed `from home, to allow more:` in a room's own shell, which may not run it
   (ADR-0269), and `to allow more:` elsewhere. If the limit is already at its parent's, the second line is `the
   untrusted rooms hold 178M of 178M` (or `the session holds ...`) and no `room set` is offered. One hint per line,
   never more.
7. **What the desktop shows.**
   - The room's badge on its windows' title bars and its chip in the dock carry, while any field is at 90% or more of
     its limit, that field's pill in the room's words, `mem 249M of 256M`, in `fg.text` with no colour (colour is for
     rooms, ADR-0172 point 7).
   - At the first `Refused` for a room, and then at most once a minute per field, the panel shows a notice in the
     trusted material (ADR-0160): `agent-1 reached its memory limit (256M). Its programs are told "over quota".`, the
     line `room set agent-1 --mem 512M`, and two buttons, `Raise to 512M` (which runs that line's change through the
     keeper) and `Dismiss`; when the limit is at its parent's, the line and the `Raise` button are left out and the
     notice says what holds the rest.
   - Show-commands (System+?) shows `room set agent-1 --mem 512M` under the pill.
   - Nothing changes for a trusted room, which has no limits of its own (ADR-0266), until the session's ceiling is
     reached, when the same notice names `session`.

**Tensions with accepted ADRs.** *ADR-0100 item 9:* the table maps a status to one `errno` per call; `OverQuota` maps
by call and field. *ADR-0100 item 6:* kept (fixed `getrlimit`). *ADR-0225 item 11:* `ENOBUFS` for `connect` and
`socket`. *ADR-0102 items 6 and 7:* the status named `QuotaExceeded` there is `OverQuota`, and `fork` maps it by field.
*ADR-0032:* a failure's value register carries the field for this status. *ADR-0155:* a status word `over quota`.
*ADR-0006* and *ADR-0046:* a new job signal, `OVER_QUOTA`. *ADR-0153 rule 11:* the hint's form is the refused path's.

**Examples.**

```
agent-1 ~/sandbox/agent-1> ./hog                      # on a 2 GiB guest (untrusted rooms' job about 718M), in the room's own shell
hog: memory_create: over quota
hint: agent-1 is at its memory limit (256M of 256M)
from home, to allow more: room set agent-1 --mem 512M

agent-1 ~/sandbox/agent-1> sh -c ':(){ :|:& };:'
sh: fork: Resource temporarily unavailable
...
hint: agent-1 is at its process limit (32 of 32)
from home, to allow more: room set agent-1 --procs 64

agent-1 ~/sandbox/agent-1> dd if=/dev/zero of=big bs=1M count=4096
dd: big: Quota exceeded
hint: ~/sandbox/agent-1 is at its disk limit (2G of 2G)
from home, to allow more: room set agent-1 --disk 4G
```

(The POSIX programs' texts are their `strerror` of `EAGAIN` and `EDQUOT` `[assumption: musl's table]`.)

### Effect of the vision
For an agent, a refusal at its own call is the honest failure: the agent's program can retry smaller, report, or
exit, and the person sees which wall it hit and the line that moves it. For the general-purpose base nothing appears
until the session itself is full, which is today's `NoMemory` with a name. A memory-pressure signal (DM-034) can be
added later as a second job signal at a threshold below the limit; nothing here precludes it.

## Consequences
- **Positive.** Every limit has one name, one signal, one hint; supervisors can act; POSIX programs see familiar
  errors.
- **Negative.** A new status for every native caller; a program that aborts on `ENOMEM` still ends, by its own hand.
- **Follow-up.** `OverQuota` and `OVER_QUOTA` in `zoraya-abi` (Q1); the `libposix` rows (Q2); the shell's hint and the
  panel's notice (Q6).

## Invariants
- **INV-0263-1.** Reaching a limit never ends, suspends or slows a process (CPU's ceiling aside). *Check:* in-QEMU
  test: a hog at each of memory, processes and disk sees `OverQuota` (or the mapped `errno`), then exits with reason
  `Exited` and its own code.
- **INV-0263-2.** A refused charge asserts `OVER_QUOTA` on exactly the limited job whose limit refused it. *Check:*
  in-QEMU test with nested limited jobs, each filled in turn, subscribing every level.
- **INV-0263-3.** `libposix` maps each refusal in the table to its `errno`. *Check:* host tests of the mapping table
  and a POSIX test program per row run in a limited room.
- **INV-0263-4.** `zosh` prints at most one quota hint per line, naming the field and the room. *Check:* host test of
  the shell's hint over a line whose commands are refused several times.

## Open questions
- Whether the notice's `Raise` button should double, or open the settings page on the room (ADR-0267).
- Whether `OVER_QUOTA` should also be raised at a lower threshold as a pressure signal (DM-034): wait for a program
  that would shed memory on it.

## Revisit triggers
- Lazy commit: refusals without a caller need `ExitReason::Limit`.
- A measured class of programs that ignore `ENOMEM` and spin: consider a per-room policy to end them.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
