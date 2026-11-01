# ADR-0056: Memory accounting by job

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0004, ADR-0006, ADR-0021, ADR-0022, ADR-0032, ADR-0040, ADR-0041, ADR-0046, ADR-0050, ADR-0051, ADR-0053, ADR-0054, ADR-0055
- **Evidence:** DM-023, CP-07, CP-14, DM-030, DM-034, DM-100, DM-111, X-6, alternatives-postmortem.md#genode, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, stage-4/sources.md#quotas

## Context
Nothing charges a job. ADR-0041 leaves this as an open question, ADR-0006 rule 2 promises that subscriptions are "charged to the holder's job", and ADR-0050 lets handles in flight escape `MAX_HANDLES`. The TASK-09 review (R-23) saw hog programs leave `spawn` with `NoMemory`, and found by reading that 255 suspended children would hold every ASID. That is a denial of service by any process, not a crash. ADR-0051 adds frames in bulk, so a single call can now take most of memory. Nothing here is measured on Zoraya; sizes are assumptions.

## Decision drivers
1. Exhaustion is a value at the request, never an asynchronous kill (CP-14, DM-030).
2. The job is the unit of containment and limits (CP-07, DM-100): a sandbox and everything it starts must stay inside one bound.
3. A limit cannot be raised from inside (ADR-0040's rule that rights only decrease).
4. A failed call has no effect, as for `spawn` and `channel_write`.
5. Charging runs on every allocating call, so its cost must have a bound.
6. One tree, one rule: DM-111 records what several hierarchies cost.

## Considered options

### Option A: A memory controller with an out-of-memory killer (Linux cgroup v2)
- **Description:** A tree of groups. Charges count against `memory.max`; at the limit the kernel reclaims, then kills inside the group (`stage-4/sources.md#quotas`).
- **Evidence for:** In production at large scale. Kernel structures are counted with user pages, and limits near the root cannot be overridden lower down. The killer stays inside the group, and `memory.oom.group` kills a workload as one.
- **Evidence against:** The failure is an asynchronous kill, against driver 1, and it follows from overcommit (DM-030). Zoraya has no reclaim and commits eagerly (ADR-0051), so the kill has no job to do.
- **Cost:** *(estimate)* A killer and a victim policy, on top of the counters.
- **Precedent:** Linux.

### Option B: Limits carved from the parent, charged at the request
- **Description:** Each job has limits, and children's limits are subtracted from the parent's room at `job_create`. So they sum within the parent, and a job's use is never above its limit. A charge touches one job.
- **Evidence for:** Drivers 1 to 5. CP-14's text. A limit is also a guarantee: a sibling cannot take what was carved for another job. Charging needs no walk. It is the kernel half of Genode's budgets (`alternatives-postmortem.md#genode`), without a session protocol.
- **Evidence against:** Memory carved and not used is lost to everyone else until the child goes, so a launcher must size each job, and every component must handle running out (`alternatives-postmortem.md#genode`).
- **Cost:** *(estimate)* 300 lines plus a charge at each allocation site, O(1) per charge.
- **Precedent:** Genode's parent-to-child budgets, without quota moved between clients and servers.

### Option C: Budgets moved down the tree (Genode)
- **Description:** Option B's budgets, and in addition clients attach quota to a session and pay for what a server allocates for them (`stage-4/sources.md#quotas`, `alternatives-postmortem.md#genode`).
- **Evidence for:** Server memory has a payer, which no other option solves. The strongest fit for hostile tenants.
- **Evidence against:** It needs a session protocol in every service, against ADR-0055's plain connections.
- **Cost:** *(estimate)* Kernel part as Option B; protocol work in each service.
- **Precedent:** Genode's full model.

### Option D: Kernel objects in memory the caller holds (seL4)
- **Description:** A process retypes memory it holds into kernel objects, so the bound is exact.
- **Evidence for:** Exact accounting with no kernel heap on the path (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** ADR-0041, as its Option C, recommends against this model; ADR-0022 already built a heap.
- **Cost:** *(estimate)* A rewrite of memory management.
- **Precedent:** seL4.

### Option E: Yes-or-no policy per object type (Zircon job policy)
- **Description:** A job may allow, deny or kill on creating each type of object. Any memory limit is left elsewhere.
- **Evidence for:** Very small, and combines with the parent's policy (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `stage-4/sources.md#quotas`). The Zircon page on jobs says they limit memory, but the policy page gives no quota.
- **Evidence against:** A job allowed to create an object can create as many as memory holds, so R-23 stands.
- **Cost:** *(estimate)* Small. It could be added beside any other option.
- **Precedent:** Fuchsia.

## Recommendation
Recommended: **Option B**, built before channels. It is a recommendation, not a decision. A retrofit would touch every allocation site that stage 4 adds.

- **Limits.** Each job has three, in a `JobLimits` value:
  - `kernel`: bytes of kernel objects.
  - `memory`: bytes of frames in address spaces and memory objects.
  - `processes`: live processes, counted because ASIDs are a scarcer resource than bytes (R-23).
- **What is charged to `kernel`.** Every object and its growth: process, thread and its 64 KiB stack, handle table, port, subscription with its reserved packet space, job, channel endpoint, queued message bytes and handle entries, and page tables. Each is charged at the size class the allocator returns, so a count matches `heap::stats`.
- **What is charged to `memory`.** User frames from `spawn` and `memory_create`, at full size at creation, since commit is eager (ADR-0051).
- **Who pays.** A new process and everything `spawn` allocates for it are paid by the job it starts in, not the caller's job, so a sandbox's use is all in the sandbox. A job's record is paid by its parent. Any other object is paid by the job of the process whose call allocated it, until it is freed, wherever its handles go. A channel is paid by the job that created it: both endpoints and every queued message, whichever side wrote it. So a client that makes its connection (ADR-0055) pays for replies it leaves unread, and a service's quota is not touched by them. Each queue's bound limits what a peer can make its maker pay. Each mapping's page tables are paid by the mapper. A subscription is paid by its caller, as ADR-0006 rule 2 says.
- **A charge holds its job.** An object keeps an `Arc` to the job that pays. A job can then outlive its processes. Objects never hold charges for each other, so this adds no cycle (ADR-0041).
- **Sum of children.** Yes: the sum of children's limits never exceeds the parent's. Each job holds `limit`, `own` (what it pays for) and `carved` (the sum of its children's limits), per field, and `own + carved <= limit` always. `job_create` carves the child's limits from the parent, or fails. The alternative, limits that may sum past the parent's with a charge walking the tree, shares idle memory better, but a sibling could then starve another below its limit (the failure of R-23 in a smaller room), and every allocation would walk. On a machine this size the guarantee is worth more than the idle memory.
- **Check, then allocate.** A charge is one comparison on one job, under that job's lock. If it fails the call fails. If the allocator then fails, the charge is undone and the call gives `NoMemory`.
- **At the limit.** A new `Status`, `QuotaExceeded`, from that call, with no effect. Nothing is killed and nothing is throttled. `NoMemory` still means the machine is out. Keeping them apart tells a supervisor which bound was reached; 
- **No allocation outside a request.** Commit is eager, signals never allocate (INV-0041-3), and exit frees. So no charge can fail where no caller can hear it. `ExitReason::Limit` (ADR-0004) stays reserved: lazy commit would use it, for a fault that cannot be charged.
- **Inheritance.** Limits are not inherited or copied. `job_create` takes a required `JobLimits`, three explicit numbers, and 0 means 0. A child can carve from its own limits for its children, and nothing from elsewhere. So a nested job can never exceed what its parent gave, at any depth. When a child's record goes, its limits return to its parent.
- **Changing a limit.** `job_set_limits(job, limits)` can only lower, and needs a new job right, `LIMIT`. A launcher passes a sandbox a job handle without it. Lowering below `own + carved` is `BadState`. What is lowered returns to the parent.
- **The root job** is the only job with no parent. Its limits are fractions of the frames left after boot, chosen so that `kernel` and `memory` cannot sum past the machine: `memory` 3/4 and `kernel` 1/8. The last 1/8 stays uncharged for the heap's own overhead and for fixed boot-time allocations. `processes` is 250, below the 255 ASIDs. Boot allocations are not charged. Objects the kernel makes at boot, such as ADR-0054's device objects, have no paying job.
- **Defaults in `init`'s table** (ADR-0053), every value `[assumption]`, for the 128 MiB of `tools/run-qemu.sh`, where the root has 15 MiB of `kernel` and 94 MiB of `memory`, and scaled in proportion on a larger machine:

| Job | `kernel` | `memory` | `processes` |
|---|---|---|---|
| Driver | 1 MiB | 4 MiB | 1 |
| Filesystem service | 3 MiB | 16 MiB | 1 |
| All clients, in one job, which `init` may subdivide | 8 MiB | 64 MiB | 32 |
| Left in the root for `init` | 3 MiB | 10 MiB | 216 |

- **Reading use.** `job_get_usage(job, out)` needs `WAIT`, and writes `own`, `carved` and `limit` for each field to an output buffer. It is what a supervisor and the tests need.
- **After `job_kill`.** A job's charges fall as its objects go. An object held from outside stays charged to the dead job until that holder lets go, and the dead job's limits stay carved from its parent until then.
- **DMA.** Quarantined frames (ADR-0054) stay charged to the job that made the memory object until `dma_release_quarantine`.
- **Services.** The kernel sees a service's memory as the service's. ADR-0055 makes clients pay for channels and buffers, by having them create both. A service's own per-connection state stays charged to it. Job limits cannot protect a service's handle table, because slots are not memory and the 1024 of ADR-0040 is per process, whatever job pays. So ADR-0055's connection budgets do that, in the service: reserved shares of its table, carved down from `init`, with `TooManyOpen` at the budget. They also bound the service's per-connection memory at 896 connections, and ADR-0055 caps held replies at 4 MiB, which is what the table's 16 MiB allows for. Budgets are carved as limits are; this is Option C's session quota applied to slots only. Client-paid memory inside services is not adopted.

**Tensions with accepted ADRs.** *ADR-0006 rule 2:* this fulfils it, and nothing changes. *ADR-0022:* `try_box` and `try_arc` gain a paying job, and the rule on where infallible allocation is allowed stands. Fixed boot-time allocations stay uncharged. ADR-0022's heap can take more from its backing than its live bytes, so a `kernel` limit bounds live bytes only, which is what the root's last 1/8 is for. *ADR-0041:* this closes its open question. *ADR-0046:* `job_create` gains an argument, and every caller must state limits, a change to the accepted call that this ADR makes if accepted.

### Effect of the vision
For target 1, this is cost that stage 4 needs anyway, because services must not be stopped by a hog. For target 2 it is a requirement, and the recommendation holds: a sandbox gets a carved job. Moving to client-paid service memory later means changing each service protocol, which is why ADR-0055's cheap answer for target 2 stays a volume per sandbox. For target 3, carving wastes idle memory on a general system. Overcommitting limits under a new flag costs little in the kernel `[estimate: 100 lines]`, but it gives up the guarantee and needs the walk. Lazy commit and pressure signals (DM-034) fit, with `ExitReason::Limit` for the first and a job signal for the second. Threads fit: each stack is already charged, and a thread count would be one more field. Networking fits: socket buffers are queued bytes, paid as channels are (cgroups count TCP buffers too, `stage-4/sources.md#quotas`). CPU time is not covered, so a sandbox can still spin; that needs its own ADR.

### Deferred to stage 6
`snapshot_process` must charge a full copy of the writable pages to the same job, as strict commit requires (ADR-0003, X-6), and `libposix`'s mapping of `QuotaExceeded` to `errno` is decided with it. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** A hostile job fails its own calls, and no job can reduce what was carved for another. R-23's hog and its ASID exhaustion are bounded: clients have 32 processes between them, by the table. Handles in flight are charged, since a channel stays charged to its maker.
- **Negative.** Carved memory that goes unused is idle. An object held by another job pins its maker's charge, so a receiver can keep a sender's use up, bounded by what the sender chose to send. Every `job_create` caller must size limits.
- **Follow-up.** A `no_std` crate for the charge tree, with host tests. A paying job on every kernel allocation path. `QuotaExceeded` in `zoraya-abi`. The rows of `invariants.md`.

## Invariants
- **INV-0056-1.** For every job and field, `own + carved <= limit`, at all times. *Check:* host model test in the charge crate, with random jobs, charges, refunds and lowerings.
- **INV-0056-2.** A failed charge changes nothing. *Check:* the same host test, plus an in-QEMU test comparing `job_get_usage` around a refused call of each allocating type.
- **INV-0056-3.** Reaching a limit never ends a process. *Check:* in-QEMU test with a hog at each limit: it sees `QuotaExceeded`, then exits with reason `Exited`.
- **INV-0056-4.** Children's limits sum to at most their parent's limit, and no call raises a limit. A handle without `LIMIT` changes none. *Check:* unit test of `JobLimits`, plus in-QEMU tests of each refusal.
- **INV-0056-5.** Every allocation on a system call path is charged to a job. *Check:* an in-QEMU test over every allocating call comparing `heap::stats` and `frames::stats` deltas with `job_get_usage`, and a lint that kernel system call code reaches `try_arc` and `try_box` only through the charging form.
- **INV-0056-6.** A job's `own` returns to zero when its objects are gone, and a charge cannot outlive its job's record. *Check:* in-QEMU loop extending INV-0041-1, including an object held from outside past `job_kill`.
- **INV-0056-7.** A job at its limit does not stop any other job from using its own limit. *Check:* in-QEMU test: a hog at each limit while a sibling spawns, opens a port and maps memory up to its own limit, which reproduces R-23.

## Open questions
- Whether the table's numbers fit: measure use at stage 4, and the cost of a charge with `portbench` and a channel round trip.
- Whether a pressure signal on a job is needed before lazy commit exists (DM-034).
- Whether an object should move its charge to the receiver on transfer, closing the pinning cost above. It would let a sender push cost onto a receiver, so the default here is no.

## Revisit triggers
- The architect commits to target 2: consider session quota.
- Lazy commit, or a workload whose idle carved memory matters.

## Architect's decision
