# ADR-0041: Kernel object lifetime

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0004, ADR-0006, ADR-0022, ADR-0030
- **Evidence:** CP-06, CP-14, DM-005, DM-022, DM-023, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#l4-family-and-sel4

## Context
Stage 3 is the first code in which user programs decide when kernel objects die. A port subscription links an object and a port. A process links its job and its thread. CP-06 says the object lives until its last handle is closed, which DM-005 warns turns a leaked handle into pinned state. Stable Rust has no fallible `Arc` constructor, and ADR-0022 forbids infallible allocation on system call paths.

## Decision drivers
1. No object is freed while referenced, and none lives after its last reference (CP-06).
2. No reference cycle can keep memory alive: DM-022 is years of cycle bugs in a kernel.
3. Closing a handle removes its subscriptions without a garbage collector (DM-023).
4. A zombie holds its exit record and nothing else (DM-005): memory goes at exit, not at the last close.
5. Allocation failure on a system call path is a `Status` (ADR-0022, CP-14).

## Considered options

### Option A: Reference counts with pointers both ways (Linux `struct file`, epoll)
- **Description:** Objects hold counted pointers to each other, including subscriptions in both directions.
- **Evidence for:** Direct and fast, and the common kernel idiom.
- **Evidence against:** Exactly the shape behind DM-022's in-flight garbage collection and DM-023's fd-versus-description confusion.
- **Cost:** Low code, high review cost at every new link.
- **Precedent:** Linux.

### Option B: Reference counts for objects; subscriptions in one table named by ids
- **Description:** Objects are `Arc`s. All signal and subscription state lives in one structure keyed by small integer ids, in a crate with host tests. A subscription refers to a source id and a port id, never to an object.
- **Evidence for:** Subscriptions cannot form a cycle, since ids count nothing (driver 2). The hard logic, which ADR-0006 calls epoll-shaped, gets host tests.
- **Evidence against:** One lock over all signal state, which does not scale past one CPU. An id must be freed exactly once.
- **Cost:** *(estimate)* 400 lines plus tests. The lock is free on one CPU.
- **Precedent:** Zircon's handles and refcounted dispatchers for the objects (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). The id-table shape is this project's.

### Option C: Kernel objects in user-supplied memory (seL4 untyped)
- **Description:** A process retypes memory it holds into objects, and revoking the memory destroys them.
- **Evidence for:** No kernel allocator on the path, and accounting is exact (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** A large model that nothing else in the design assumes. ADR-0022 already built a kernel heap.
- **Cost:** *(estimate)* A rewrite of memory management.
- **Precedent:** seL4.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Objects** are `Arc`s. A handle, a thread running for a process, and a job's list of live processes each hold one count. `heap::try_arc` probes the allocator for the layout, frees the probe, then calls `Arc::new`. With one CPU, interrupts masked and no preemption in the kernel, nothing runs between, so the second allocation succeeds. SMP must replace it.
- **Signals and subscriptions** live in `zoraya-events`, in one `SpinLock`. A process or job has a source id, freed in its `Drop`. A port has a port id, whose subscriptions end when it closes, freed with the last `Arc`. Space for every packet a subscription can ever need is reserved when it is made, so asserting a signal never allocates.
- **Process.** Live, then a zombie, then freed. It becomes a zombie at exit, on its own thread: it releases the address space and ASID, closes every handle, records `ExitRecord`, asserts `TERMINATED`, and leaves its job. A zombie is an `Arc` with a record. It is freed when the last handle goes.
- **Ending another process.** The requester sets the exit it wants and wakes the target. The target ends itself on its own stack at the next return to EL0, or as it starts. So nothing is torn down under a blocked thread's feet, and no user instruction runs after the request.
- **Cycle.** A process holds its thread and a thread its process, until exit breaks that. A job's list holds its live processes and each holds its job, until exit breaks that.
- **Job.** A job holds its parent, and a parent holds its children weakly. A job lives while a handle, a live process or a child holds it. So a job tree cannot be cut off from the root while something runs in it.
- **Unreachable process.** A process keeps running when its last handle closes. It is still in its job, which is how it can be ended.

## Consequences
- **Positive.** Ownership has one direction everywhere but two cycles that exit breaks. No garbage collector. The event logic is tested on the host.
- **Negative.** Removing a subscription leaves a stale ready item, bounded by what is live. A suspended process nobody resumes stays until its job is killed. `try_arc` depends on one CPU. One global lock for signals.
- **Follow-up.** A real fallible `Arc` when SMP arrives, or when `Arc::try_new` is stable.

## Invariants
- **INV-0041-1.** After a process ends and its last handle closes, the kernel's frame count and heap use return to their earlier values. *Check:* an in-QEMU loop of spawn, exit and close, comparing `frames::stats` and `heap::stats`.

  *Partly replaced by ADR-0054 (architect's decision, 2026-11-01, on its acceptance):* frames of a DMA pin ended without `dma_unpin` return only after `dma_release_quarantine`.

- **INV-0041-2.** A process's address space, ASID and handles are released at exit, whoever still holds the process. *Check:* in-QEMU test holding a handle to an ended process, plus INV-0034-2's test.
- **INV-0041-3.** Asserting a signal never allocates. *Check:* `zoraya-events` host test under a counting allocator.
- **INV-0041-4.** A subscription cannot outlive its source or its port. *Check:* `zoraya-events` host tests, closing either side first.

## Open questions
- Charging kernel memory to a job (CP-14, ADR-0006 rule 2): no accounting exists yet, so a hostile process can use kernel memory until `NoMemory`.

## Revisit triggers
- SMP, which ends both the `try_arc` argument and the one-lock argument.
- A measured contention or latency problem on the signal lock.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
