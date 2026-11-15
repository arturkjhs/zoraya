# ADR-0437: The list of mappings as built (plan part F5): chunks, charged by what the heap holds, walks in steps, no handle kept for a private mapping

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0051, ADR-0102, ADR-0260, ADR-0261, ADR-0270, ADR-0300, ADR-0320, ADR-0415, ADR-0431
- **Evidence:** CP-05, CP-14, GR-005

## Context

ADR-0415 item 1 (accepted) raises the limit to 4096 mappings per process, "held in an ordered structure (a B-tree or sorted
array of regions) so lookup and insertion stay logarithmic", each record "charged to the process's job as kernel memory", and
has `libposix` close a private mapping's object handle once mapped. **Read in the code before F5:** the kernel kept a `Vec` of
records sorted by address (`kernel/src/memory/user.rs`), 256 at most, not charged; the place for a new mapping was found by a
scan of the list; `libposix` kept `MAPPING_SLOTS` (256) triples of address, size and handle in a static array of 6 KiB, and the
heap's steps (`Objects::grow`) kept their object's handle for ever, so a 1 GiB heap used 1024 handles, the whole table.
**Measured before the change (not on a 4096 list):** none; ADR-0320's count (256) was the limit. **What 4096 does to a flat
`Vec`:** a record is about 64 bytes, so 4096 are 256 KiB; every insertion or removal moves up to that, and every doubling of
the capacity is an allocation of that size and a copy, in the interval of a system call, with IRQ masked (ADR-0270,
INV-0270-1). **Other loops that scale with the count and were never counted** because 256 was small enough: the plan of a
`snapshot_process` (one `Vec` of records, built under the space's lock), the creation of one object of the copy for each
mapping (a step point only inside an object, so 4096 one-page objects were one interval), the drop of those objects when the
call is cancelled, the child's list of mappings dropped whole, a snapshot's step that counted pages and not the page-table frames
a sparse space adds for each page, and `memory_unmap` of a mapping whose handle is closed (the last holder's frames are freed in
the unmapping, where before the close freed them).

## Decision drivers

1. **No work under a masked lock grows with what a process holds without a stated bound** (INV-0270-1, CP-05, GR-005): 4096
   is 16 times the count the loops were written for.
2. **A process with no mappings pays nothing**; one with 4096 pays for them, to its job (ADR-0260, ADR-0415 item 1, CP-14): the
   kernel bytes per process must not grow 16 times for the rest.
3. **A refusal changes nothing** and is a status, not a panic (ADR-0261 point 6).
4. **Small:** the plan says 500 lines.

## Considered options

### Option A: the Linux baseline, a tree of areas (maple tree) with a limit of 65530

Logarithmic everything; the nodes are allocated under the lock, which Linux allows and this kernel's masked intervals do not.

### Option B: a `Vec` kept sorted, 4096 records

The least change. An insertion or removal moves up to 256 KiB and a growth copies it, so a step of one mapping is the work of
64 pages or more: the bound fails at 4096 and a copy is not separable.

### Option C: a `BTreeMap` of records

Logarithmic. Every insertion may allocate a node inside the lock, which cannot be made ahead of the insertion for an
unknown key, so room cannot be reserved before pages are mapped.

### Option D: a fixed array of 4096 records in every space

Constant time and no allocation; 256 KiB in every process, of which the ten that a session holds are most.

### Option E: a sorted list of sorted chunks of at most 64 records

A lookup is two binary searches, an insertion or removal moves at most a chunk (about 4 KiB), the list grows by one chunk at a
time, room is made ahead (`reserve_for`) and the insertion that follows never allocates. The search for a place is the one walk
of every record.

## Recommendation

**Option E**, as built, with these rules.

1. **`zoraya-mappings`** (a `no_std` crate, host-tested with a model): the list is `Mappings<T>`, 64 records to a chunk. A
   full chunk splits at the half, or when the record comes at the top of the last chunk, starts a new one (so a process that
   maps upward, as `find_gap` makes it, has full chunks). A chunk that empties is given back. `find_gap` reports how many
   records it visited.
2. **The charge is what the heap holds.** `bytes_held` (chunk capacities and the list of chunks) is charged to the space's
   job as kernel memory: the most `reserve_for` can add (`max_growth`) is charged first and what the list did not grow by is
   given back, so a refusal allocates nothing; a removal gives back what the list no longer holds. Measured: **see the report**
   (about 70 bytes a mapping at 4096, and under 1 KiB for a process with a few).
3. **Work is counted.** The search for a place counts one unit for every 128 records (32 for 4096); an insertion or removal one;
   the plan of a snapshot is made in steps of 1024 records, a step point between them; `SnapshotBuild::new` has a step point
   between objects once a step's pages are made; a space's list, a snapshot's copies and a snapshot's child are dropped one
   record at a time with a step point each step (`steps::before`); a snapshot's step and `finish` count the child's
   page-table frames with its pages (a step is 64 frames, not 64 pages); `memory_unmap` has a step point before it frees the
   last holder's frames.
4. **`MAPPING_SLOTS` is one constant in `zoraya-abi` (4096).** The kernel's limit is it; `libposix`'s table has that many
   slots, each an address and a size (16 bytes), in one memory object mapped the first time a private mapping is made (64 KiB, in
   no program that never maps); the table lists no handle: `mmap`, a thread's stack and a heap step close their object's handle
   as soon as it is mapped, so a mapping costs one of the kernel's slots and none of the 1024 handles. A mapping of a memory
   descriptor (ADR-0413) is not in this table.
5. **No panic at the limit.** The 4097th mapping is `LimitReached` (`ENOMEM` from `mmap`), the job's refusal `OverQuota`, a
   failure to allocate the list `NoMemory`, each with nothing changed (INV-0415-1).

## Consequences

- **Positive.** A process holds 4096 mappings at the work of a step per call; a process of ten pays about a kilobyte for its
  list; a `fork` of 4096 mappings is steps of 1024 records and 64 frames; CPython's arenas and a browser's thread stacks fit.
- **Negative.** Another crate; a program that maps pays 64 KiB for the table (before: 6 KiB for every program); the search
  for a place is linear in the records below it (32 units at 4096); two places that scale with the walk of the page tables
  remain (below).
- **Follow-up.** ADR-0415's items 2 and 3 (F6 and E1).

## Invariants

- **INV-0437-1.** The list of mappings is ordered by address, holds no overlapping ranges, has at most 64 records in a chunk and
  no empty chunk but the last; what the heap holds for it is exactly what the space's job is charged beyond its page tables, at
  every size and after every removal; a job that cannot take its growth refuses the mapping with `OverQuota`, nothing changed.
  *Check:* `cargo test -p zoraya-mappings` (against a model, to 4096, at the limit), and the in-QEMU test
  `a_space_holds_4096_mappings_and_pays_for_the_list` (`make test-qemu-one`).
- **INV-0437-2.** With the most mappings a process may hold, each call that walks them holds no more than a step of work between
  two step points: a map (with the search of every record), an unmap, a `snapshot_process` (plan, copies, records, a sparse space
  with one page-table frame for each page) and the end of the child, a process's exit and its kill. *Check:* `t_bound`'s cases
  `memory_map, up to 4032 mappings` and the like, and its `4000 mappings of 4000 objects` teardown (`make test-qemu-one`);
  the units are checked on every host, the time under HVF.
- **INV-0437-3.** `libposix` keeps no handle for a private mapping or a heap step: 3,000 one-page `mmap`s and 900 heap steps are
  held at once by a process whose handle table has 1024 slots. *Check:* `t_maps limit` in a 2 GiB guest (`make test-libposix`);
  `cargo test -p zoraya-posix` (`maptab`).

## Open questions

- **The search for a place is linear** in the records below the place. At 4096 it is 32 units (about 10 us, not measured on
  the machine this part ran on beyond the `t_bound` case). A tree of gaps would make it logarithmic; it is built only if a
  measurement asks for it.
- **A snapshot's walk of a sparse space** still reads the 512 entries of each page-table it meets, uncounted: with one page in
  each of 2016 tables a step reads about 32,000 entries (measured in `t_bound`, "a table for each of 2016 pages"; the number is
  in the report). Counting them needs `zoraya-pagetable`'s walk to report what it read.
- **The frame list copy** of `memory_map` and `adopt` (ADR-0436, F8's known limitation) is not changed by this part.
- Whether the heap's step stays 1 MiB (ADR-0415 item 1: until the engine's measurements say).

## Revisit triggers

- A measured interval over 2 ms in `t_bound`'s 4032-mapping cases under HVF.
- A process that needs more than 4096 mappings.

## Architect's decision

(Left empty for the architect.)
