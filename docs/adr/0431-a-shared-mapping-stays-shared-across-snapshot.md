# ADR-0431: A `SHARED` mapping of a memory object stays shared across `snapshot_process` (plan part F3)

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0051, ADR-0102, ADR-0261, ADR-0270, ADR-0413
- **Evidence:** CP-14, GR-005, X-6

## Context

ADR-0102 item 2 makes `snapshot_process` an eager copy: a fresh frame for every page the process owns, and a fresh memory
object for every mapped one (`AddressSpace::snapshot`). That is right for the private anonymous memory `mmap` made until
now, and wrong for a shared mapping. ADR-0413 item 3 lets a program map a memory descriptor `MAP_SHARED`; POSIX says the
mapping stays shared with the object in a `fork` child, and the plan's criterion for part F3 is that a child reads what the
parent wrote and the parent sees what the child writes. With the eager copy the child's mapping would be a private copy of
the frames at the moment of the fork, silently. **Measured:** `t_snapshot` (`a_shared_mapping_stays_shared`) in
`make test-qemu`. **Assumed:** a program that forks with shared mappings expects them shared (every POSIX program does).

The kernel cannot tell a shared mapping from a private one: both are a `Holder::Memory` of a memory object, and a mapping of
a received object is the same call as a mapping of the process's own scratch buffer.

## Decision drivers

1. **POSIX semantics of `MAP_SHARED` across `fork`**, without a silent copy that diverges (a program that coordinates
   through shared memory would hang or corrupt, not fail).
2. **Exhaustion at the request** (CP-14): sharing takes no frame for the object's pages, where the copy took one for each;
   the object's frames stay charged to its creator alone (INV-0413-4), so a fork bomb of shared mappings costs the page
   tables only.
3. **Rights only decrease**: the child's mapping has the parent's attributes, never more, and never executable
   (INV-0051-2).
4. **The strict-commit cost of fork emulation** (X-6, GR-005): a change that makes `fork` cheaper without making any
   process able to reach more.

## Considered options

### Option A: Linux: every mapping the parent made `MAP_SHARED` is shared in the child

- **Description:** The kernel records, per mapping, whether it is shared, and the child maps the same pages.
- **Evidence for:** Drivers 1 to 3; it is POSIX's definition.
- **Evidence against:** The kernel needs the bit from the caller.
- **Cost:** *(estimate)* A flag, a field per mapping and a branch in the snapshot: 80 lines.
- **Precedent:** Linux, FreeBSD, every Unix.

### Option B: The library repairs the child (unmap the copy, map the object again at the same address)

- **Description:** After a fork the child's library unmaps each copied shared mapping and maps the object at the old address.
- **Evidence for:** No kernel change.
- **Evidence against:** `memory_map` chooses the address (there is no fixed mapping, ADR-0051), so the old address cannot be
  had; every pointer into the mapping would dangle. The copy was also paid for first.
- **Cost:** *(estimate)* Not possible.
- **Precedent:** none.

### Option C: A per-object flag at `memory_create` (a shared object is never copied)

- **Description:** The object is marked at creation, and every mapping of it is shared in a child.
- **Evidence for:** No field per mapping.
- **Evidence against:** The creator decides for everyone who maps it; a `MAP_PRIVATE` mapping of a memory descriptor is a
  copy made by the library (ADR-0413 item 3), but a program could not map an object it was sent privately without a copy.
  The flag is on the wrong party.
- **Cost:** *(estimate)* The same as A.
- **Precedent:** Windows' sections are marked at creation (`SEC_*`), and are mapped per view.

### Option D: Refuse `snapshot_process` while a shared mapping exists

- **Description:** `BadState`, as for a device mapping or a pin (ADR-0102 item 2).
- **Evidence for:** Nothing to build; no silent copy.
- **Evidence against:** `fork` of any program that mapped a memory descriptor fails (`EAGAIN`-like), which breaks the plan's
  criterion and CPython's `multiprocessing` with `fork`.
- **Cost:** *(estimate)* 5 lines.
- **Precedent:** the same file's device mappings.

## Recommendation

**Recommendation (not a decision): Option A, as a flag of `memory_map`.**

1. **`MapFlags::SHARED`** (bit 1 of `memory_map`'s flags; `WRITE` stays bit 0; the decoder accepts 0 to 3 and nothing else).
   A mapping made with it records the offset of its first page in the object. `snapshot_process` maps those pages into the
   child as they are, with the parent's attributes, and copies no page of it; every other mapping is copied as ADR-0102
   item 2 says. The child's mapping keeps the object alive (a `Holder::Memory`) and is itself `SHARED`, so a fork of the
   child shares it again.
2. **Who sets it.** `libposix` does, for `mmap(MAP_SHARED)` of a memory descriptor (ADR-0432), and for nothing else: the
   library's own scratch buffer, the copy of a `MAP_PRIVATE` mapping, thread stacks and every anonymous mapping stay
   copied.
3. **Charges.** The child pays the page tables of the shared pages (as for any mapping, ADR-0261) and nothing for the
   frames, which are the creator's.
4. **Rights.** A `SHARED` mapping needs `DUPLICATE` on the handle, besides `READ` (and `WRITE`): the snapshot gives the child a
   handle only if it can be duplicated, so without the right the child would keep the frames with no handle. A mapping that is
   not shared needs no more than before. The handle of the object is duplicated into the child by the snapshot's table copy as
   before; `memory_unmap` of a shared mapping is as any; no right is added to what a process may do.

### Effect of the vision

Unchanged: a process reaches the frames a handle gave it and no more. The child of a fork holds the handles its parent held
and the mappings they made, with no more rights; sharing is the parent's mapping, repeated.

## Consequences

- **Positive.** `fork` after `mmap(MAP_SHARED)` has POSIX's meaning; a snapshot of a process with a large shared buffer
  copies nothing for it (frames saved: the object's size, measured by `t_snapshot`).
- **Negative.** One more bit in the kernel ABI (before anything outside the repository depends on it); `snapshot_plan` carries
  the object for a shared mapping.
- **Follow-up.** A shared anonymous mapping (`MAP_SHARED|MAP_ANONYMOUS`, Python's `mmap.mmap(-1, n)`) can use the same bit;
  ADR-0413 item 7 does not offer it, and this part does not add it.

## Invariants

- **INV-0431-1.** After `snapshot_process`, a mapping the parent made `SHARED` is in the child the same object at the same
  offset with the same attributes (so a write by either is read by the other), and every other mapping is a copy that
  diverges. *Check:* `t_snapshot` (`a_shared_mapping_stays_shared`) in `make test-qemu`, which writes through whole and
  partial shared mappings and a private one on both sides.
- **INV-0431-2.** A snapshot of a process with shared mappings takes no frame for their pages. *Check:* the same test compares
  the frames a snapshot takes with the mapping shared and not.
- **INV-0431-3.** `memory_map` accepts flags 0 to 3 and refuses any other value; `SHARED` with `WRITE` through a handle without
  `WRITE`, and `SHARED` through a handle without `DUPLICATE`, are `AccessDenied`. *Check:* `t_snapshot` for the second; `cargo test -p zoraya-abi` (`memory_map_flags_are_zero_to_three`) for the first.

## Open questions

- Whether a shared mapping of the child should be recorded as shared in a *spawned* child: a spawn gives a child handles, not
  mappings, so the question does not arise until a handle is mapped there.

## Revisit triggers

- A port needs `MAP_SHARED|MAP_ANONYMOUS` or a shared mapping of a regular file (a pager, ADR-0413).

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
