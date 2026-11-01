# ADR-0056: Memory accounting by job

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0004, ADR-0006, ADR-0021, ADR-0022, ADR-0040, ADR-0041, ADR-0046, ADR-0050, ADR-0051, ADR-0054, ADR-0055
- **Evidence:** CP-07, CP-14, DM-030, DM-034, DM-100, DM-111, X-6, alternatives-postmortem.md#genode, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, stage-4/sources.md#quotas

## Context
Nothing charges a job. ADR-0041 leaves this as an open question, ADR-0006 rule 2 promises that subscriptions are "charged to the holder's job", and ADR-0050 lets handles in flight escape `MAX_HANDLES`. The TASK-09 review (R-23) saw hog programs leave `spawn` with `NoMemory`, and found by reading that 255 suspended children would hold every ASID. That is a denial of service by any process, not a crash. ADR-0051 adds frames in bulk, so a single call can now take most of memory. Nothing here is measured on Zoraya; sizes are assumptions.

## Decision drivers
1. Exhaustion is a value at the request, never an asynchronous kill (CP-14, DM-030).
2. The job is the unit of containment and limits (CP-07, DM-100): a sandbox and everything it starts must stay inside one bound.
3. A limit cannot be raised from inside (ADR-0040's rule that rights only decrease).
4. A failed call has no effect, as for `spawn` and `channel_write`.
5. Charging runs on every allocating call, so its cost must have a bound. ADR-0046 caps job depth at 32.
6. One tree, one rule: DM-111 records what several hierarchies cost.

## Considered options

### Option A: A memory controller with an out-of-memory killer (Linux cgroup v2)
- **Description:** A tree of groups. Charges count against `memory.max`; at the limit the kernel reclaims, then kills inside the group (`stage-4/sources.md#quotas`).
- **Evidence for:** In production at large scale. Kernel structures are counted with user pages, and limits near the root cannot be overridden lower down. The killer stays inside the group, and `memory.oom.group` kills a workload as one.
- **Evidence against:** The failure is an asynchronous kill, against driver 1, and it follows from overcommit (DM-030). Zoraya has no reclaim and commits eagerly (ADR-0051), so the kill has no job to do.
- **Cost:** *(estimate)* A killer and a victim policy, on top of the counters.
- **Precedent:** Linux.

### Option B: Hard limits on the job tree, charged at the request
- **Description:** Each job has limits. A charge walks from the job to the root and fails with a status if any limit would be passed. Limits of children may sum to more than their parent's.
- **Evidence for:** Drivers 1 to 5. CP-14's text. The same walk as cgroups, without the kill. No transfer of budget, so a launcher only states bounds.
- **Evidence against:** A limit is not a reservation: siblings can together starve each other below their limits unless a launcher keeps the sum within the parent. Each allocation costs a walk.
- **Cost:** *(estimate)* 300 lines plus a charge at each allocation site, O(depth) per charge.
- **Precedent:** The tree and the walk are cgroup v2's. Returning a status at the limit is this ADR's own.

### Option C: Budgets moved down the tree (Genode)
- **Description:** A parent moves part of its budget to a child, so children's budgets cannot exceed what the parent gave. Clients attach quota to a session and pay for what a server allocates for them (`stage-4/sources.md#quotas`, `alternatives-postmortem.md#genode`).
- **Evidence for:** A guarantee, not only a bound: a sibling can take nothing that was given away. Server memory has a payer, which no other option solves. The strongest fit for hostile tenants.
- **Evidence against:** Memory given but unused is lost to everyone, and every component must handle running out (`alternatives-postmortem.md#genode`). It needs a session protocol in every service, against ADR-0055's plain connections.
- **Cost:** *(estimate)* Kernel part as Option B; protocol work in each service.
- **Precedent:** Genode.

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
- **What is charged to `kernel`.** Every object and its growth: process, thread and its 64 KiB stack, handle table, port, subscription with its reserved packet space, job, channel endpoint, queued message bytes and handle entries, handoff, and page tables. Each is charged at the size class the allocator returns, so a count matches `heap::stats`.
- **What is charged to `memory`.** User frames from `spawn` and `memory_create`, at full size at creation, since commit is eager (ADR-0051). `snapshot_process` charges a full copy of the writable pages to the same job, as strict commit requires (ADR-0003, X-6), so a frame shared by copy-on-write counts twice, an overestimate kept on purpose.
- **Who pays.** The job of the process whose call allocated it, until it is freed, wherever its handles go. A message is paid by its writer until it is read or destroyed. Each mapping's page tables are paid by the mapper. A subscription is paid by its caller, as ADR-0006 rule 2 says.
- **A charge holds its job.** An object keeps an `Arc` to the job that pays. A job can then outlive its processes, and its own record is paid by its parent. Objects never hold charges for each other, so this adds no cycle (ADR-0041).
- **Check, then allocate.** The charge is made before allocating and walks from the job to the root under the job tree lock. If any level would pass a limit, every level is undone and the call fails. If the allocator then fails, the charge is undone and the call gives `NoMemory`.
- **At the limit.** A new `Status`, `QuotaExceeded`, from that call, with no effect. Nothing is killed and nothing is throttled. `NoMemory` still means the machine is out. Keeping them apart tells a supervisor which bound was reached; `libposix` maps both to `ENOMEM`, and to `EAGAIN` for `fork` and `spawn`.
- **No allocation outside a request.** Commit is eager, signals never allocate (INV-0041-3), and exit frees. So no charge can fail where no caller can hear it. `ExitReason::Limit` (ADR-0004) stays reserved: lazy commit would use it, for a fault that cannot be charged.
- **Inheritance.** `job_create` gains a `JobLimits`, and 0 in a field means "no limit of its own". Otherwise a field must not exceed the parent's limit for that field, or the call gives `AccessDenied`, as with rights. A child's use counts in every ancestor, so a tree is held by its smallest bound. Children's limits may sum past the parent's: that is the launcher's choice. Limits are not copied, so a later cut of a parent's limit binds its children at once.
- **Changing a limit.** `job_set_limits(job, limits)` can only lower, and needs a new job right, `LIMIT`. A launcher passes a sandbox a job handle without it. Lowering below current use fails nothing already held: later charges fail until use falls.
- **The root job** has the machine's sizes as its limits, and boot allocations are not charged. Objects the kernel makes at boot, such as ADR-0054's device objects, have no paying job.
- **Reading use.** `job_get_usage(job, out)` needs `WAIT`, and returns use and limits for the job's whole tree. It is what a supervisor and the tests need.
- **After `job_kill`.** A job's charges fall as its objects go. An object held from outside stays charged to the dead job until that holder lets go.
- **DMA.** Quarantined frames (ADR-0054) stay charged to the job that made the memory object until `dma_release_quarantine`.
- **Services.** The kernel sees a service's memory as the service's. ADR-0055 makes clients pay for channels and buffers, by having them create both. A service's own per-connection state stays charged to it, so one client can still use up a shared service's quota or its 1024 handles. A service caps connections per client; this is a limit, not a charge. Option C's session quota is the complete answer and is not adopted.

**Tensions with accepted ADRs.** *ADR-0006 rule 2:* this fulfils it, and nothing changes. *ADR-0003 rule 3:* a `fork` counts double for shared frames, which is the commit its strain note already accepts. *ADR-0022:* `try_box` and `try_arc` gain a paying job, and the rule on where infallible allocation is allowed stands. Fixed boot-time allocations stay uncharged. *ADR-0022's fragmentation bound:* a limit bounds live bytes, and the heap may take up to 3 times the peak from the backing, so the root's limits must leave room for that. *ADR-0041, ADR-0046,* both proposed: this closes ADR-0041's open question, and adds an argument to `job_create`.

### Effect of the vision
For target 1, this is cost without a user until a test needs it, though a hog already stops `spawn` today. For target 2 it is a requirement, and the recommendation is the same, with one gap: a limit gives no guarantee. Two sandboxes with limits summing past the parent's can starve each other. A launcher that keeps the sum within the parent's limit gets the effect of Option C's kernel half. A `reserved` field, a floor charged at `job_create`, can be added later without changing callers `[estimate: 100 lines]`. Moving to client-paid service memory later means changing each service protocol, which is why ADR-0055's cheap answer for target 2 stays a volume per sandbox. For target 3, a general system wants lazy commit and pressure signals (DM-034): both fit, with `ExitReason::Limit` for the first and a job signal for the second. Threads fit: each stack is already charged, and a thread count would be one more field. Networking fits: socket buffers are queued bytes paid by the writer, as cgroups charge TCP buffers. CPU time is not covered, so a sandbox can still spin; that needs its own ADR.

## Consequences
- **Positive.** A hostile job fails its own calls. R-23's ASID exhaustion is bounded by `processes`. Handles in flight are charged, since an endpoint stays charged to its maker.
- **Negative.** A walk per allocation. An object held by another job pins its maker's charge, so a receiver can keep a sender's use up, bounded by what the sender chose to send. A limit bounds, and does not reserve. Shared frames count twice.
- **Follow-up.** A `no_std` crate for the charge tree, with host tests. A paying job on every kernel allocation path. `QuotaExceeded` in `zoraya-abi`. A launcher policy for default limits. The rows of `invariants.md`.

## Invariants
- **INV-0056-1.** No level of a job tree ever has use above its limit, unless the limit was lowered below it. *Check:* host model test in the charge crate, with random jobs, charges, refunds and lowerings.
- **INV-0056-2.** A failed charge leaves every level as it was. *Check:* the same host test, plus an in-QEMU test comparing `job_get_usage` around a refused call of each allocating type.
- **INV-0056-3.** Reaching a limit never ends a process. *Check:* in-QEMU test with a hog at each limit: it sees `QuotaExceeded`, then exits with reason `Exited`.
- **INV-0056-4.** No job's limit exceeds its parent's, and no handle without `LIMIT` changes one, and no call raises one. *Check:* unit test of `JobLimits`, plus in-QEMU tests of each refusal.
- **INV-0056-5.** Every allocation on a system call path is charged to a job. *Check:* an in-QEMU test over every allocating call comparing `heap::stats` and `frames::stats` deltas with `job_get_usage`, and a lint that kernel system call code reaches `try_arc` and `try_box` only through the charging form.
- **INV-0056-6.** A job's use returns to zero when its objects are gone, and a charge cannot outlive its job's record. *Check:* in-QEMU loop extending INV-0041-1, including an object held from outside past `job_kill`.
- **INV-0056-7.** A job at its limit does not stop a job outside its tree. *Check:* in-QEMU test: a limited hog at each limit while a sibling spawns, opens a port and maps memory, which reproduces R-23.

## Open questions
- Default limits for a sandbox, and the cost of a walk per allocation: measure with `portbench` and a channel round trip.
- Whether a pressure signal on a job is needed before lazy commit exists (DM-034).
- Whether an object should move its charge to the receiver on transfer, closing the pinning cost above. It would let a sender push cost onto a receiver, so the default here is no.

## Revisit triggers
- The architect commits to target 2: consider `reserved`, then session quota.
- Lazy commit, or `snapshot_process` measured to fail on double-counted frames.
- A charge walk that shows up in a measured system call path.

## Architect's decision
