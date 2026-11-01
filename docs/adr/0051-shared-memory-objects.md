# ADR-0051: Shared memory objects

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0020, ADR-0021, ADR-0031, ADR-0040, ADR-0041, ADR-0045
- **Evidence:** DM-023, CP-14, DM-030, DM-160, GR-007, X-6, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#l4-family-and-sel4, alternatives-postmortem.md#hubris, stage-4/sources.md#memory-objects

## Context
ADR-0050 caps a message at 64 KiB, ADR-0010 allows atomic writes of up to 1 MiB, and a driver needs memory a device can reach. So a process must be able to share pages. ADR-0031 maps everything at creation and has no mapping call. It is needed by ADR-0054 and ADR-0055, hence this ADR, which TASK-10 made conditional.

## Decision drivers
1. Memory failure at the request, never later (CP-14, DM-030): no overcommit here.
2. Handles and rights as for all objects (ADR-0040).
3. W^X holds for everything mapped (INV-0031-1).
4. Shared memory can change under a reader (DM-160): the rule must be stated where the object is defined.

## Considered options

### Option A: `memfd` and `mmap` (Linux)
- **Description:** A file-like object, mapped by a general `mmap` with protection flags.
- **Evidence for:** GR-007's unification; familiar; `libposix` will need `mmap` anyway.
- **Evidence against:** Overcommit by default (DM-030), and flags that mix file and anonymous cases.
- **Cost:** *(estimate)* A general mapping layer the kernel does not otherwise need yet.
- **Precedent:** Linux.

### Option B: A memory object with eagerly committed frames
- **Description:** A handle to a set of frames. Mapped explicitly into the caller's space, read-only or read-write.
- **Evidence for:** The Zircon VMO is the standard sharing object there (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `stage-4/sources.md#memory-objects`), though Zircon commits lazily. Eager commit satisfies driver 1 and ADR-0031's "no demand paging".
- **Evidence against:** Large sparse objects cost their full size (X-6's tradeoff).
- **Cost:** *(estimate)* 300 lines, with TLB maintenance on unmap.
- **Precedent:** Zircon VMOs, with this ADR's commit rule.

### Option C: Loan memory for one call (Hubris leases)
- **Description:** A sender lends part of its address space for the duration of a call, and the kernel copies on the borrower's request.
- **Evidence for:** No mapping changes, and no shared state after the call (`alternatives-postmortem.md#hubris`).
- **Evidence against:** It needs a synchronous call, which ADR-0050 does not recommend, and a kernel copy per access.
- **Cost:** *(estimate)* Small; every access is a system call.
- **Precedent:** Hubris. L4's long IPC, a cousin, was abandoned in favour of shared buffers (`alternatives-postmortem.md#l4-family-and-sel4`).

### Option D: Page-frame capabilities mapped by the user (seL4)
- **Description:** A capability per frame, with user-managed tables.
- **Evidence for:** Exact authority (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** ADR-0041 chose kernel-held objects; this needs a different memory model.
- **Cost:** *(estimate)* Very large.
- **Precedent:** seL4.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Calls.** `memory_create(size, contiguous)` commits all frames, zeroed, or fails with `NoMemory`; `contiguous` uses ADR-0021's run allocation and exists for ADR-0054's queues. `memory_map(object, offset, length, write)` returns an address the kernel chose. `memory_unmap(address, length)`. `Param` gains an address kind for the last; an address names no object, only a place in the caller's own space. The size is fixed.
- **Rights.** `READ` to map, `WRITE` to map writable, plus `DUPLICATE` and `TRANSFER`. A duplicate with only `READ` is how a client gets a read-only view. No right maps anything executable: user code comes from `spawn` alone at this stage.
- **Object type.** `NotWaitable` (INV-0006-1).
- **Lifetime.** Each handle, each mapping and each DMA pin (ADR-0054) holds the object. Frames go with the last of them. A process's mappings go at exit, before its handles.
- **Unmapping** is break-before-make with `tlbi` for the ASID, as ADR-0020 requires of the kernel's tables.
- **Limit.** 256 mappings per process `[assumption]`, then `LimitReached`.
- **Tension: INV-0031-2.** It says a process cannot read or write another process's memory. A shared object does that by consent. Its wording needs a superseding ADR if this is accepted.
- **Tension: INV-0045-1.** Its allow-list, which replaced those of INV-0002-1 and INV-0035-1, has no address kind. If this ADR is accepted, INV-0051-5 replaces it, in the way ADR-0035 and ADR-0045 did, and INV-0045-1's row is marked as replaced.
- **The rule for readers.** Memory another process can write is hostile input that can change between two reads. Copy out, then validate, and never validate in place. A service reads the bytes once.

### Effect of the vision
No change. For target 2, `NoMemory` at creation is what a sandbox wants, and charging to a job (ADR-0056) must come first. Demand paging and copy-on-write can be added under the same handles. Language runtimes need executable mappings, which would be a new right plus W^X rules: a later ADR at a modest cost, since nothing here precludes it.

### Deferred to stage 6
What `snapshot_process` does to a mapping of a memory object, which must stay shared with its other holders while ADR-0003 rule 3 makes the address space copy-on-write. `mmap` for `libposix`. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** Failure is immediate. A read-only view is a rights mask.
- **Negative.** No sparse objects, and no resizing. Kernel chooses addresses, so a library must adapt.
- **Follow-up.** Executable mappings for a dynamic loader; lazy commit if a workload needs it.

## Invariants
- **INV-0051-1.** `memory_create` either commits every frame or fails, and changes nothing else. *Check:* in-QEMU test against the frame counter, at exhaustion.
- **INV-0051-2.** No user mapping from a memory object is executable, or writable through a handle without `WRITE`. *Check:* in-QEMU table walk, with INV-0031-1's test extended.
- **INV-0051-3.** After `memory_unmap`, an access faults, on every process that unmapped. *Check:* in-QEMU test with a `Fault` exit record.
- **INV-0051-4.** Frames return when the last handle, mapping and pin go, in any order. *Check:* in-QEMU test with each order.
- **INV-0051-5.** INV-0045-1's rule, with the address parameter of `memory_unmap` added to the allow-list by name. *Check:* the lint of INV-0045-1, extended.

## Open questions
- The first use of a kernel-chosen address policy for ASLR.

## Revisit triggers
- A workload that needs sparse memory.
- A need for executable shared code.

## Architect's decision
