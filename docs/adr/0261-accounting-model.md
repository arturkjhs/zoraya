# ADR-0261: The accounting model: nested ceilings on jobs, refused at the request, paid by the creator

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0001, ADR-0004, ADR-0006, ADR-0022, ADR-0040, ADR-0041, ADR-0046, ADR-0050, ADR-0051, ADR-0054, ADR-0056, ADR-0102, ADR-0104, ADR-0172, ADR-0221, ADR-0222, ADR-0260, ADR-0262, ADR-0263, ADR-0264, ADR-0266, ADR-0267, ADR-0269, ADR-0270
- **Evidence:** CP-07, CP-14, DM-030, DM-111, X-6, alternatives-postmortem.md#genode, alternatives-postmortem.md#l4-family-and-sel4, stage-4/sources.md#quotas, quotas/survey.md#linux-cgroups-v2, quotas/survey.md#windows-job-objects, quotas/survey.md#fuchsia-jobs-and-memory-attribution, quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts, quotas/survey.md#genode-resource-trading, quotas/survey.md#zoraya-today

## Context
Nothing charges a job today (`quotas/survey.md#zoraya-today`). Proposed ADR-0056 recommends limits *carved* from the
parent at `job_create`: children's limits sum to at most the parent's, so a limit is also a guarantee, and every
`job_create` caller must state three numbers. Two facts arrived after it. ADR-0104 (accepted) has the broker make a
session job, a job per process group and a job per POSIX process, so `make -j4` in a room creates jobs at five levels below the room (the shell's command, the broker's, the session, the group, the process), and none of their callers can know how much each
child needs. And ADR-0172 (accepted) makes the room the unit a person names: the limit a person sets is on the room,
and nothing below it should need sizing. This ADR settles the model: what a limit is, how it nests, what a charge
touches, who pays for kernel objects and for memory shared between jobs. Objects a service creates for a client are
ADR-0262.

**ADR-0056 is not revised in place.** It is proposed, so it cannot be superseded (ADR-0001 rule 1 keeps
`superseded` for accepted ADRs), but its text is a reviewed record of the carving model that this ADR keeps as
Option B, and accepted network ADRs quote its sentences (ADR-0221 item 7, ADR-0222's open question). Editing it would
erase the option and change what those ADRs cite. ADR-0260 to ADR-0270 replace it; if the architect accepts them,
the expected status for ADR-0056 is `rejected`, with a note pointing here. Its parts that carry over are named below.

What is measured: nothing. What is read: the job structure and the allocator (`quotas/survey.md#zoraya-today`).

## Decision drivers
1. Exhaustion is a value at the request (CP-14, DM-030), and Zoraya commits eagerly (ADR-0051), so every charge has
   a caller to refuse.
2. The job is the containment unit (CP-07), and a room is a job (ADR-0172): a limit on the room must bound
   everything below it, at any depth, without anyone below sizing anything.
3. No limit can be raised from inside (ADR-0040's rights only decrease).
4. A charge runs on every allocating call, so its cost must have a small bound (ADR-0056 driver 5).
5. One tree, one rule (DM-111): limits live on the job tree that already exists.
6. Memory on a 128 MiB guest is scarce: memory set aside and not used should not be lost
   (`quotas/survey.md#zoraya-today`: 9,391 free frames with 255 suspended processes).

## Considered options

### Option A: Ceilings with an out-of-memory killer (Linux cgroup v2)
- **Description:** A limit per group that may over-commit; a charge counts in every ancestor; at the limit the
  kernel reclaims, then kills inside the group (`quotas/survey.md#linux-cgroups-v2`, `stage-4/sources.md#quotas`).
- **Evidence for:** The most used design; no group below the limited one needs sizing; `memory.oom.group` ends a
  workload as one; protections (`memory.min`) give guarantees where wanted.
- **Evidence against:** The failure is an asynchronous kill (against driver 1), which exists because Linux
  overcommits (DM-030); Zoraya has no reclaim and commits eagerly, so there is nothing for a killer to do that a
  refusal does not do earlier. Shared memory is charged at random (`quotas/survey.md#linux-cgroups-v2`).
- **Cost:** *(estimate)* Counters and a walk, plus a victim policy.
- **Precedent:** Linux; production at large scale, with user-space killers (oomd) added because the kernel's acts late.

### Option B: Limits carved from the parent (ADR-0056's recommendation; Genode's budgets without donation)
- **Description:** `job_create` takes the child's limits and subtracts them from the parent's room; `own + carved <=
  limit` on every job; a charge touches one job (ADR-0056).
- **Evidence for:** A limit is a guarantee: no sibling can take what was carved for another job. A charge is one
  comparison under one lock. It is Genode's parent-to-child transfer (`quotas/survey.md#genode-resource-trading`),
  whose quota moves only along the parent link and so cannot be stolen across branches.
- **Evidence against:** Every `job_create` caller must size the child, and ADR-0104's broker makes three jobs per
  POSIX process whose needs it cannot know: a `cc` under `make` might need 2 MiB or 40. Carved and idle memory is
  lost to everyone (driver 6); Genode's answer, resource requests and yield, is a protocol every component must speak
  (`alternatives-postmortem.md#genode`). A change to the accepted `job_create` (ADR-0046).
- **Cost:** *(estimate)* 300 lines in the kernel, plus sizing logic in `init`, the panel, `zosh` and the broker.
- **Precedent:** Genode; ADR-0056.

### Option C: Nested ceilings that may over-commit, refused at the request, charged to the creator
- **Description:** Any job may have a limit per field (memory, processes; CPU in ADR-0264), or none. A job with none
  is bounded by its nearest limited ancestor. A child's limit may not exceed its parent's effective limit, but
  siblings' limits may sum past the parent's. A charge is counted in the paying job and in every *limited* ancestor,
  and refused, with no effect, if any of them would pass its limit. No kill, no throttle.
- **Evidence for:** Drivers 1 to 6: only the room is sized; jobs below it need nothing; idle memory stays usable.
  Windows' nested jobs apply the strictest limit in the chain the same way, and aggregate accounting upward
  (`quotas/survey.md#windows-job-objects`); cgroups' limits over-commit by design
  (`quotas/survey.md#linux-cgroups-v2`). With refusal instead of a kill it is CP-14's text.
- **Evidence against:** A limit is not a guarantee: two rooms whose limits sum past the machine can starve each
  other. The walk costs one comparison per limited ancestor, not one. Kept against: the session's own ceiling keeps
  services' memory out of every room's reach (the reserve below), and limited ancestors are few.
- **Cost:** *(estimate)* 350 lines in the kernel and a `no_std` charge crate of 400 with host tests; a charge is one
  to four comparisons and adds.
- **Precedent:** cgroup v2 limits without the killer; Windows nested job memory limits.

### Option D: Kernel objects from memory the caller holds (seL4)
- **Description:** Each job holds untyped memory and retypes it into kernel objects; the caller pays by construction
  (`quotas/survey.md#sel4-untyped-memory-and-scheduling-contexts`).
- **Evidence for:** Exact, with no kernel heap on the charging path; a resource manager can divide memory exactly.
- **Evidence against:** A rewrite of memory management; ADR-0041 and ADR-0022 built a kernel heap, and ADR-0056 noted
  the same. The catalog records that building native systems on seL4 is hard (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Cost:** *(estimate)* Several thousand lines, and every allocating call's ABI changes.
- **Precedent:** seL4.

### Option F: Carving only where a limit is set, ceilings below (Option B's strongest form)
- **Description:** Limits are carved, as in Option B, but only at the levels a person names: the session is divided
  into the rooms that have limits, and every job below a room has no limit of its own and shares the room's. So a
  room's limit is also a guarantee against other rooms, and nothing below a room is sized.
- **Evidence for:** It keeps Option B's guarantee between rooms and `home` at no sizing cost for ADR-0104's jobs; the
  quota moves only along the parent link, as Genode's does (`alternatives-postmortem.md#genode`).
- **Evidence against:** Memory carved into idle rooms is lost to `home` (driver 6): on 128 MiB, two untrusted rooms
  at their defaults would set aside half of the session whether or not they run anything. A room created but not
  entered still holds its carve, unless carving happens at entry, which makes entering a room fail when the session
  is busy.
- **Cost:** *(estimate)* Option C plus 100 lines: a carve at `job_set_limits`, returned at the room's end.
- **Precedent:** Genode's subdivision of budgets along the tree.

### Option E: Attribution without limits (Fuchsia's memory reports)
- **Description:** Count and show what each job uses; let a supervisor kill.
  (`quotas/survey.md#fuchsia-jobs-and-memory-attribution`).
- **Evidence for:** Cheapest; useful for `top` regardless.
- **Evidence against:** The fork bomb is still bounded only by the machine; Fuchsia's own page admits double counting.
- **Cost:** *(estimate)* The counters of Option C without the checks.
- **Precedent:** Fuchsia's `ps`.

## Recommendation
**Recommendation (not a decision): Option C.** The counters of Option E come with it. The guarantee that matters
now, `home` against all untrusted rooms together, is given by one ceiling above them (point 10), not by carving;
Option F is the step if a guarantee between two rooms is needed.

1. **Limits.** `JobLimits { memory: Option<u64> bytes, processes: Option<u32> }` (CPU in ADR-0264). `None` means
   "no limit of my own". A job is created with none (`job_create` is unchanged, ADR-0046) and its creator sets them
   with `job_set_limits(job, limits)` before handing the job on. The effective limit of a job is the smallest in its
   chain.
2. **Nesting.** `job_set_limits` refuses (`InvalidArgument`) a limit above the parent's effective one, so a room's
   limit is never weaker than its launcher's. Siblings may sum past their parent: the parent's own limit is what
   bounds them together.
3. **The right.** `job_set_limits` needs a new job right `LIMIT`. A launcher keeps it on the room's job and gives
   programs sub-jobs without it (ADR-0172 point 2), so nothing inside a room can raise the room. Raising is allowed to
   the holder of `LIMIT`, up to the parent's effective limit; lowering below the current use is allowed, and every
   charge after it is refused until use falls (the person can squeeze a room).
4. **A charge.** Each job keeps, per field, `used` (its own and its subtree's) only if it or a descendant is limited;
   each job points at its nearest limited ancestor. A charge of `n` walks from the paying job through its limited
   ancestors, checks each `used + n <= limit`, and then adds to each. If any check fails, nothing changes and the
   call returns `OverQuota` (ADR-0263). If the allocator then fails, the charge is undone and the call returns
   `NoMemory`. The walk holds one lock (the job tree's, ADR-0046) and visits at most six limited jobs in the layouts of ADR-0266 (`init`'s job, the session, the untrusted rooms' job, the room, and up to two below), at most 32 by ADR-0046's depth.
5. **What a job's memory counts.** User frames, at 4 KiB each, from `spawn`, `memory_create`, `snapshot_process` and
   page tables; every kernel object at the size class `try_box` and `try_arc` return (process, thread with its 64 KiB
   stack, handle table, port with its packets, subscription, job record, channel endpoint, stream ring, event, timer).
   `job_get_usage` reports `user` and `kernel` apart.
6. **Who pays for a kernel object.** The job of the process whose call created it, until it is freed, wherever its
   handles go (ADR-0056's rule, kept). Exceptions, each because another job is the one being built:
   - `spawn`: the new process, its address space, stack, handle table and image frames are paid by the job it starts
     in, so a room pays for its programs, not its launcher.
   - `snapshot_process`: the child's full copy is paid by the child's job, as ADR-0102 item 7 and X-6 require; the
     call is refused before copying if it does not fit.
   - `job_create`: the new job's record is paid by the new job (and so by its ancestors).
   - Page tables of a mapping: by the mapper's job.
   - Boot objects and device objects (ADR-0054) have no paying job.
7. **Queued messages are paid by the writer.** A message's bytes and handle entries in a channel's queue are charged
   to the writing process's job when written and refunded when read or destroyed. This replaces ADR-0056's "a channel
   is paid by the job that created it: both endpoints and every queued message": with it, an untrusted program could
   make its launcher or a service pay for up to 256 KiB per direction of every channel it holds; with this rule it
   pays for what it writes, and the reader pays nothing it did not ask for. The endpoints themselves stay paid by
   their creator. A handle in flight keeps its object charged to the object's creator.
8. **Memory shared between jobs.** A memory object's frames are paid by its creator's job for its whole life, however
   many jobs map or hold it; mapping charges only page tables. The charge does not move on transfer (ADR-0056's open
   question, answered no: a sender could push cost onto a receiver). So a holder can keep a creator's charge up,
   bounded by what the creator chose to share and, for services, by ADR-0262's per-client bound on attached buffers.
   No proportional sharing: Fuchsia's page shows how such attribution miscounts
   (`quotas/survey.md#fuchsia-jobs-and-memory-attribution`), and cgroups' shared charge is non-deterministic
   (`quotas/survey.md#linux-cgroups-v2`).
9. **A charge holds its job.** An object keeps an `Arc` to the job that pays (ADR-0056, kept), so a job's record
   lives until its last charge is refunded. After `job_kill`, what is held from outside stays charged to the dead job
   and its ancestors; `job_get_usage` reports it as `held` so `top` can say "held by others".
10. **The reserve.** The root job has no limit. The kernel starts `init` in a job whose memory limit is the frames
    free after boot less a kernel reserve of 1/16 `[assumption]` for the heap's overhead and fixed allocations. `init` puts every program the person launches in one *session* job with a ceiling of its own, on memory and on
    processes (below the 255 address space identifiers less what `init` and the services need), so that services
    (drivers, `fsd`, the compositor, `netd`) keep what the session cannot take. Inside the session, every untrusted
    room is a child of one *untrusted* job with ceilings on memory, processes and CPU below the session's, so that the
    untrusted rooms together cannot fill the session that `home` lives in, however many there are (ADR-0266 gives
    the sizes). These two ceilings are the guarantees the machine needs, at two places, without carving below them.
11. **Usage.** `job_get_usage(job, out, flags)` needs `WAIT`; the flag `CLEAR` also clears the job's `OVER_QUOTA`
    signal (ADR-0263). It writes per field: `limit` (or none), `used`, `peak`,
    `refused` (charges refused since creation), and for memory `user`, `kernel` and `held`.
12. **Rights of a new job.** `job_create` returns a handle with every job right, `LIMIT` included, so a process may
    limit the jobs it creates; by point 2 such a limit can only be narrower than its own, so it gains nothing. A
    launcher passes a room's programs their jobs without `LIMIT` on the room.
13. **Later, not now:** a `reserve` field or Option F (a guarantee between rooms, as cgroups' `memory.min` and
    ADR-0056's carving give), when two rooms that must not starve each other are measured to; and moving a charge,
    never.

**Tensions with accepted ADRs.** *ADR-0046:* `job_create` is unchanged; two calls are added (`job_set_limits`,
`job_get_usage`) and a job right `LIMIT`, under ADR-0040's rights rules. *ADR-0006 rule 2:* subscriptions are charged
to the holder's job, as it says. *ADR-0022:* `try_box` and `try_arc` gain a paying job on system call paths; boot stays
uncharged. *ADR-0041:* a job gains a fourth kind of holder, an object it pays for (as ADR-0056 said). *ADR-0050:* its
"who pays for a channel is ADR-0056's" is answered by point 7, the writer. *ADR-0102 item 7:* fulfilled.
*ADR-0104:* none of its jobs needs sizing; the room above them is limited. *ADR-0172 point 2:* a room's job is where
its limits are, which needs one job per limited room (ADR-0269). *ADR-0056* (proposed): replaced, not revised in
place (Context); ADR-0001 rule 1 is why it cannot be marked superseded.

**Examples.** A launcher, in Rust (not typed by a person):

```
let room = job_create(&untrusted, false)?;            // the handle carries LIMIT
job_set_limits(&room, JobLimits { memory: Some(512 << 20), processes: Some(32), cpu: Some(250) })?;
let cmd = job_create(&room, false)?;                  // no limits of its own
let room_view = handle_duplicate(&room, Rights::ALL - Rights::LIMIT)?;  // what a program may see of the room
```

What a person sees when a program in the room runs into it is ADR-0263's; the numbers are ADR-0267's `top`:

```
zosh ~> top agent-1
JOB             MEM               PROCS
agent-1         498M of 512M      14 of 32   (user 431M, kernel 67M; refused 3)
  make -j4      464M              9
    cc (4 jobs) 152M              4
  ...           34M               5   (term, zosh, broker, tmpd, top; held by others 9M)
```

### Effect of the vision
For untrusted agents this is what makes a room a bound: everything under the room's job, at any depth, counts against
the room, and nothing inside can raise it. For the general-purpose base, trusted rooms and `home` have no limits of
their own and lose nothing to carving. Lazy commit later would make a page fault a charge with no caller to refuse,
which ADR-0004's `ExitReason::Limit` is reserved for (ADR-0056, kept). SMP needs the walk's lock to become per-job
counters with atomics, one `fetch_add` and compare per limited ancestor.

## Consequences
- **Positive.** Only rooms (and the session) are sized; POSIX's job layers need nothing; a room's use is everything
  in it; the writer pays for messages, which closes the cheapest way to make another job pay.
- **Negative.** Limits are ceilings, not guarantees: untrusted rooms whose limits sum past the untrusted job's can
  starve each other inside it (`home` is kept out of their reach by that job's ceiling, not by a guarantee per room). A holder can keep a creator's shared memory charged. Every system call path that allocates gains a
  paying job.
- **Follow-up.** The charge crate and the kernel's paying job on every path (Q1), `OverQuota` (ADR-0263), the reserve
  in `init` (Q1), `job_get_usage` for `top` (Q6). ADR-0056's invariants INV-0056-1 to -7 are replaced by the ones
  below; INV-0056-8 and -9 by ADR-0262's.

## Invariants
- **INV-0261-1.** For every limited job and field, `used <= limit` after every successful charge, where `used` is the
  sum of the charges of the job and its subtree. *Check:* host model test in the charge crate with random trees,
  charges, refunds, limit changes and kills, comparing every counter with a recomputation.
- **INV-0261-2.** A refused charge changes no counter and the call has no effect. *Check:* the same host test, and an
  in-QEMU test comparing `job_get_usage` and the frame and heap statistics around a refused call of each allocating
  type.
- **INV-0261-3.** No call made with a job handle lacking `LIMIT` changes a limit, and no limit is set above the
  parent's effective limit. *Check:* in-QEMU tests of each refusal; unit test of the check.
- **INV-0261-4.** A job at its limit does not stop a job outside its subtree from using its own limit. *Check:*
  in-QEMU test: a memory hog and a fork bomb in one limited room while a sibling room spawns, creates ports and maps
  memory up to its limit.
- **INV-0261-5.** Every allocation on a system call path is charged to a job. *Check:* an in-QEMU test over every
  allocating call comparing frame and heap deltas with `job_get_usage`, and a lint that kernel system call code
  reaches `try_box` and `try_arc` only through the charging forms.
- **INV-0261-6.** A job's `used` returns to zero when its objects are gone, and a charge never outlives its job's
  record. *Check:* in-QEMU loop extending INV-0041-1, with an object held from outside past `job_kill`.
- **INV-0261-7.** Queued message bytes are charged to the writer and refunded when read or destroyed. *Check:* host
  test of the channel queue's accounting and an in-QEMU test where a limited writer fills a reader's queue and the
  reader's job shows no change.
- **INV-0261-8.** With every untrusted room at every limit, a program in `home` can still start processes and allocate
  memory up to the session's ceiling less the untrusted job's. *Check:* in-QEMU test: four untrusted rooms, the most the keeper runs at once (ADR-0269), each running a fork bomb and a memory hog, while a `home` program spawns 16 processes and maps
  16 MiB.

## Open questions
- The kernel reserve's size (1/16) against the heap's real overhead: measure `heap::stats` against live bytes on the
  tour and the POSIX session.
- Whether `top` should show `held` per holder: it needs the holder's identity, which the kernel does not keep.
- Whether a guarantee is needed between two untrusted rooms (the `reserve` field): wait for a case.

## Revisit triggers
- Lazy commit or demand paging (a charge without a caller).
- SMP: the walk under one lock becomes a contention point.
- A measured case where two rooms under the session starve each other and the person wanted both to run.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. Ceilings, with the one ceiling over all untrusted rooms; carving (Option F) stays an option. This replaces ADR-0056, which is rejected. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
