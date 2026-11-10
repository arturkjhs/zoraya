# ADR-0260: What quotas limit, and which limits are hard in stage 8

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0002, ADR-0006, ADR-0046, ADR-0050, ADR-0056, ADR-0057, ADR-0101, ADR-0102, ADR-0221, ADR-0222, ADR-0261, ADR-0262, ADR-0263, ADR-0264, ADR-0265, ADR-0266, ADR-0267, ADR-0268
- **Evidence:** CP-07, CP-14, DM-030, DM-100, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#freebsd-rctl-and-jails, quotas/survey.md#windows-job-objects, quotas/survey.md#fuchsia-jobs-and-memory-attribution, quotas/survey.md#genode-resource-trading, quotas/survey.md#zoraya-today, DM-034

## Context
A capability says what a program may reach; a quota says how much of the machine it may use. Proposed ADR-0056 limits
memory, kernel bytes and processes of a job, and leaves CPU, storage and the network to other ADRs. Since then the
network design (ADR-0221 item 7, ADR-0222's "Relation to ADR-0056") listed what `netd` must bound per client, stage 6
showed that a fork bomb in a session stops only when memory runs out (TASK-21; `docs/design.md`, "Known
limitations"), and stage 7 made the compositor hold a copy of every surface of up to 8 clients
(`quotas/survey.md#zoraya-today`). This ADR fixes the list: what is limited, in what unit, where it is enforced, and
which limits must be hard before untrusted code runs (ADR-0268) and which come later. *How* each is charged is
ADR-0261 and ADR-0262; CPU ADR-0264; storage ADR-0265.

What is measured: nothing new. What is read: the bounds that exist today are per object or global, and none is per job
(`quotas/survey.md#zoraya-today`).

## Decision drivers
1. Every resource an untrusted program can make the machine spend without bound is a way to take it down, even with no
   capability (CP-07, DM-100): the list must be complete for what a program in an untrusted room can reach.
2. A limit the person is asked to set must be one they understand: memory, CPU, processes, disk, connections; kernel
   byte counts by object type are not (`quotas/survey.md#freebsd-rctl-and-jails` lists 25 resources, most of which no
   person sets).
3. Fewer counters on the charging path cost less on every allocation (ADR-0056 driver 5).
4. Refusal at the request (CP-14, DM-030): a limit must be one the kernel or a service can check before it allocates.
5. Stage 8 has a fixed budget: what is not needed to bound an untrusted room's damage waits.

## Considered options

### Option A: The rlimit and rctl list (Linux/FreeBSD baseline)
- **Description:** Many named limits, each its own counter: address space, data size, stack, open files, processes,
  threads, CPU seconds, locked memory, message queues, I/O rates, as `setrlimit` and `rctl` have
  (`quotas/survey.md#freebsd-rctl-and-jails`), plus cgroups' memory, pids, cpu and io controllers
  (`quotas/survey.md#linux-cgroups-v2`).
- **Evidence for:** Familiar to anyone who has configured a Unix; each counter answers one question exactly; rctl
  applies rules to running processes at once.
- **Evidence against:** Counters overlap (address space, data size and resident memory bound the same thing three
  ways) and are per process or per user, not per containment unit, except in rctl's jail subject and cgroups. rctl's
  own page warns that its memory limit can thrash the machine. Most counters protect kernel tables Zoraya sizes
  differently (handle tables are 1024 slots per process already, ADR-0057).
- **Cost:** *(estimate)* A counter and a check per resource on every path that allocates it: twenty or more sites.
- **Precedent:** Linux rlimits and cgroups, FreeBSD rctl; in production for decades.

### Option B: Yes-or-no policies per object type (Zircon job policy)
- **Description:** A job may allow, deny or kill on creating each kind of object (`quotas/survey.md#fuchsia-jobs-and-memory-attribution`); amounts are not limited.
- **Evidence for:** Tiny, combines with the parent's, and removes whole kinds of object from a sandbox.
- **Evidence against:** A job allowed to create memory can create all of it; the fork bomb is not bounded. ADR-0056's
  Option E said the same.
- **Cost:** *(estimate)* 100 lines; usable beside any other option.
- **Precedent:** Fuchsia; its memory is attributed, not limited, in the pages read.

### Option C: Five figures a person sets, and bounds per object that the system sets (recommended)
- **Description:** A job (and so a room) has a small set of limits a person can read: **memory** (one byte count
  covering user frames and every kernel object charged to the job), **processes** (live processes, since address
  space identifiers are scarcer than bytes), **CPU** (a ceiling per period); a writable directory has a **disk**
  limit; a client of `netd` has a **connections** count. Everything else that can grow is bounded by a fixed bound per
  object (messages per channel, packets per port, mappings per process) or per client of a service (ADR-0262's
  accounts), so that its cost is inside one of the five.
- **Evidence for:** Drivers 1 to 5. One memory figure is how Windows' job memory limit and cgroups' `memory.max` are
  set (`quotas/survey.md#windows-job-objects`, `quotas/survey.md#linux-cgroups-v2`); kernel structures are charged
  there with user pages in cgroups too. Most of the per-object bounds exist already (`quotas/survey.md#zoraya-today`).
- **Evidence against:** One memory figure hides whether user pages or kernel objects filled it; `top` must show both
  (ADR-0267). A per-object bound times the number of objects can still be large, which is why the objects' memory is
  charged as well.
- **Cost:** *(estimate)* Three counters on the job, one in `fsd`'s directory record, one per `netd` scope.
- **Precedent:** cgroups v2's memory, pids and cpu controllers on one tree; Genode's two budgets, RAM and
  capabilities (`quotas/survey.md#genode-resource-trading`).

### Option D: Memory only (Genode's RAM budget alone)
- **Description:** One budget, bytes; processes, handles and connections are bounded because each costs memory.
- **Evidence for:** The fewest counters; one thing to set.
- **Evidence against:** Address space identifiers run out at 255 processes long before memory does (ADR-0056's
  reason for `processes`); Genode itself added a second budget for capabilities because RAM alone did not bound them
  (`quotas/survey.md#genode-resource-trading`). CPU and disk are not memory.
- **Cost:** *(estimate)* Smallest.
- **Precedent:** Genode before 17.05.

## Recommendation
**Recommendation (not a decision): Option C.**

| Resource | Unit | Enforced by | Paid by (ADR-0261, ADR-0262) | Stage 8 |
|---|---|---|---|---|
| Memory | bytes: user frames and charged kernel objects | the kernel, at the allocating call | the job that creates the object | **hard** |
| Processes | live processes, under a session ceiling below the 255 address space identifiers (ADR-0266) | the kernel, at `spawn` and `snapshot_process` | the job the process starts in | **hard** |
| CPU | a ceiling: a share of each 20 ms period | the scheduler (ADR-0264) | the job the thread runs in | **hard** for rooms; shares later |
| Disk | bytes and objects under a directory | `fsd` (ADR-0265) | the directory tree | **hard** |
| `/tmp` of an untrusted room | bytes | the room's own `tmpd` (ADR-0265) | the room's memory | **hard** |
| Service state | per-client counts: connections, attached buffers, held replies, surfaces | each service (ADR-0262) | the client's account | **hard** in `fsd`, `tmpd`, the broker, `netd`, the compositor |
| Network objects | connections (and smoltcp's buffers with them), listeners, pending resolutions, resolver cache entries, datagram sockets (each with ADR-0050's queue bound) | `netd` (ADR-0262) | the room's account | **hard** |
| Threads | live threads | the kernel | the job | later: one thread per process today |
| Handles | slots | the process's table (1024, ADR-0057) | covered by memory and processes | exists |
| Queued messages, stream rings, port packets, requests | per object bounds (ADR-0050, ADR-0101, ADR-0006, ADR-0046) | the kernel | memory of the writer or creator (ADR-0261) | exists, now charged |
| Kernel deadlines | 288 in one queue | the kernel | covered by processes (one per sleeping thread) and one global period deadline (ADR-0264) | exists |
| Disk bandwidth, network bandwidth | bytes per second | later | — | later |
| Memory pressure signal | — | later (DM-034's question) | — | later |

- **Memory is one figure.** It counts user frames from `spawn`, `memory_create` and `snapshot_process`, and every
  kernel object charged to the job at the size its allocator returns, page tables and the 64 KiB thread stack
  included. ADR-0056's separate `kernel` limit is dropped: a person cannot size it, and the heap's own overhead is
  kept out of every job by the root's reserve (ADR-0261). `job_get_usage` still reports the two parts apart.
- **Hard in stage 8** means: refused at the request with no effect (ADR-0263), checked by a test that must pass before
  `room new --untrusted` stops being refused (ADR-0268).
- **The fork bomb** is bounded twice: by processes, and by memory, since every child is charged a full copy
  (ADR-0102 item 7). **The memory hog** by memory. **The disk filler** by disk. **The CPU spinner** by the CPU
  ceiling. **The connection flood** by connections per scope.
- **Not limited, on purpose:** handles per job (memory and processes bound them); bytes written per second
  (bandwidth waits for a measured need); wall-clock time (a room that runs long is the person's to end).

**Tensions with accepted ADRs.** *ADR-0056* (proposed): its separate `kernel` limit is dropped (one memory figure).
*ADR-0002:* limits apply to every process of a job, native or POSIX alike; the personality adds no limit of its own
and reports fixed `rlimit` values (ADR-0263), so nothing POSIX-specific enters the kernel. *ADR-0057:* the 1024 slots
per process stay the handle bound.

**Examples.** What a person types and sees (the syntax is ADR-0266's, the display ADR-0267's):

```
zosh ~> room new agent-1 --untrusted --mem 512M --cpu 25% --procs 32 --disk 2G +w ~/sandbox/agent-1    # 2 GiB guest
zosh ~> room
* home
  project   +w ~/project +r ~/include
    agent-1   untrusted  --mem 512M --cpu 25% --procs 32 --disk 2G --conns 16 --tmp 16M  +w ~/sandbox/agent-1
```

On the desktop the room's card in the overview and the hover card of a window in it list the same five words, each
with its use: `mem 498M of 512M`, `cpu 25% of 25%`, `procs 14 of 32`, `disk 1.2G of 2G`, `conns 3 of 16`.

### Effect of the vision
This is the list the isolation direction needs: an agent in an untrusted room can exhaust none of the five, and
everything else it can grow is bounded per object or per account. For the general-purpose base the same list costs
nothing to a trusted room, which has no limits of its own (ADR-0266). Threads, when they come, add one field; a GPU
would add one per-client account in its service.

## Consequences
- **Positive.** Five words cover what a person sets; every other growth has a bound that is already written down.
- **Negative.** One memory figure for user and kernel memory means a program that makes many small kernel objects
  fills its limit sooner than its resident size suggests; `top` shows the split.
- **Follow-up.** The counters (Q1, Q2 in `quotas/plan.md`), `fsd`'s directory limits (Q5), the accounts (Q3), the
  CPU ceiling (Q4).

## Invariants
- **INV-0260-1.** Every resource in the table above marked hard has a refusal path with a test that reaches it from a
  program in a limited job. *Check:* a CI script that reads the table's hard rows and requires, for each, a named test
  in `tools/test-map.txt`'s list of quota tests (ADR-0268's gate list).
- **INV-0260-2.** No kernel allocation reachable from a system call grows without a per-object bound or a charge to a
  job. *Check:* INV-0261-5's lint and in-QEMU test, which cover every allocating call.

## Open questions
- Whether threads, when added, should count against processes (one address space identifier each is not needed) or
  have their own field: decide with the threads ADR.
- Whether disk bandwidth matters on one virtio disk: measure a disk filler's effect on another room's `sync`.

## Revisit triggers
- A resource found that a program in an untrusted room can grow without a bound in this table.
- SMP, threads, or a GPU service.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
