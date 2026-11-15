# ADR-0432: Memory descriptors as built (plan part F3): the wire form, seals as handle rights, `fork`, `spawn`, the mapping rules and the names

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0051, ADR-0100, ADR-0102, ADR-0103, ADR-0107, ADR-0261, ADR-0342, ADR-0412, ADR-0413, ADR-0430, ADR-0431
- **Evidence:** CP-01, CP-14, CP-16, DM-031, DM-032

## Context

ADR-0413 (accepted 2026-11-14) decides anonymous memory descriptors over memory objects, `MAP_SHARED`, and seals expressed as
rights. Plan part F3 builds them. The code needed choices that ADR-0413 leaves open: how a descriptor looks in a `SCM_RIGHTS`
message, what a seal does to a handle the process already has, what `fork` and a spawn do with a descriptor, what an `mmap`
length and offset may be, and which calls came with it. None is against ADR-0413; the kernel half (a `SHARED` mapping that a
`fork` child keeps shared) is its own ADR-0431, because it changes the kernel's ABI. This ADR records the rest, so that none is
silent, and carries the `posix-surface-added` block (ADR-0342 item 1). **Measured:** the host tests of
`crates/zoraya-posix/tests/memfd.rs`, `t_memfd` (`make test-posix`), `t_unix memfd` (`make test-libposix`), `t_snapshot` and
`t_quota` (`make test-qemu`), recorded in `docs/design.md`. **Assumed:** WebKit's `createHandle(ReadOnly)` uses a seal or a
second descriptor (ADR-0413's open question); both work with item 2.

## Decision drivers

1. **A seal that binds is one the kernel enforces** (CP-16, DM-032): after `F_SEAL_FUTURE_WRITE` no handle the library gives out
   can map the object writable, whatever the program does next.
2. **The size never changes under a mapper** (DM-031): a descriptor's object is replaced only while nothing else can hold it.
3. **No global names** (CP-01) and **no later surprise** (CP-14): a size is committed at `ftruncate`, never at a touch.
4. **Fail closed where a thing is not built**: an `errno`, never a descriptor with no handle.

## Considered options

### Option A: the choices below (the recommendation)

### Option B: Linux's semantics for each of them

Seals shared by every holder (one inode), `F_SEAL_WRITE` waiting for writable mappings to go (`EBUSY` only while they exist),
a descriptor of any size mapped with `SIGBUS` past its end, `memfd_create` surviving `exec`. Rejected where the kernel has no
object to hold the state (seals are per holder), kept for every `errno` a Linux program tests.

### Option C: Capsicum's and Fuchsia's way: a seal is a new handle with fewer rights, the old one stays

Nothing is closed when a seal is added. Rejected as the default: the program keeps a writable handle it could still send or
`fork` with, which is what the seal is for; the library closes its own writable handle instead (item 2).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The wire form.** A memory descriptor in a `SCM_RIGHTS` message is a 24-byte record of kind 7 with one handle: a *copy* of
   the descriptor's object handle (the sender keeps its own), the record's `access` 2 (read-write: the handle has `WRITE`) or 0
   (read-only), `read_off` the size in bytes, `write_off` the seals (bits 0 to 4) and bit 8 for "accepts seals". A decoder takes
   any bytes: access 1 or 3, a nonzero field a memory descriptor has no use for, a size above 1 TiB or a seal bit above 4 drops
   the message and closes its handles. The receiver's descriptor is fixed in size (item 3), is sealable as the sender's was, and
   has only the seals it can check: `F_SEAL_SHRINK`, `F_SEAL_GROW` and `F_SEAL_SEAL` as the sender had them *at the send* (the size never changes; a seal against sealing only takes from the receiver), and `F_SEAL_FUTURE_WRITE` if its own handle is read-only. `F_SEAL_WRITE` and a `F_SEAL_FUTURE_WRITE` over a handle that can write are the sender's word, which the receiver cannot check, and are not reported; a seal is the holder's, not shared (a limit: Linux shares one set). A descriptor
   whose size is 0 has no object and is `EINVAL` to send. A record that says read-write over a handle that is not still cannot
   map writable: the kernel refuses (`EACCES`, INV-0413-2).
2. **Seals are handle rights.** `F_SEAL_FUTURE_WRITE` and `F_SEAL_WRITE` replace the library's writable handle by one
   without `WRITE` and close the old one, so every later mapping, copy for a send, `fork` child and `dup` of the object is
   read-only in the kernel; existing mappings keep their access. After either seal `write`, `pwrite` and a writable
   `MAP_SHARED` are `EPERM`; a descriptor that arrived read-only is `EACCES` for the same mapping, and `F_ADD_SEALS` on it is
   `EPERM` (it is not open for writing). `F_SEAL_WRITE` is `EBUSY` if a writable handle has left the process (a send, a
   `fork`, or the descriptor arrived writable) or the descriptor has a writable mapping here; a writable mapping of a *different*
   descriptor does not matter. `F_SEAL_SHRINK`, `F_SEAL_GROW` and `F_SEAL_SEAL` are as Linux has them; without
   `MFD_ALLOW_SEALING` the only seal set is `F_SEAL_SEAL`, and `shm_open(SHM_ANON)` accepts seals. A new object made by a
   `ftruncate` after the seal is made read-only at once.
3. **What fixes the size** (INV-0413-1). A shared `mmap` that succeeded, a send that happened, a `fork` (the parent after the
   snapshot, the child at its start), and a descriptor that arrived. Not a `dup`, not a private `mmap`, not an `munmap`
   (unmapping does not unfix it). A `write` past the end of a fixed descriptor takes what fits and is `EFBIG` for the rest.
   An unfixed one grows by a new object of at least twice the old capacity, with a copy, so a program that writes a memfd as a
   file is linear. `ftruncate` above 1 TiB is `EFBIG`; `ftruncate` to 0 gives the object back.
4. **`fork`.** The kernel keeps a `SHARED` mapping shared in the child (ADR-0431); the handle is duplicated by the snapshot at
   the same slot; both processes mark every descriptor fixed and writable-handle-out. A private copy made by `MAP_PRIVATE` is
   copied by the snapshot like any private memory, and its handle too.
5. **`spawn` and `exec` do not carry a memory descriptor yet** (as for a Unix socket, ADR-0430 item 7). An open descriptor
   without `O_CLOEXEC` makes them fail with `EBADF`; one with it is simply not inherited. `memfd_create` without `MFD_CLOEXEC`
   was a tmpd file that survived `exec`, and no port of the image relies on that (dash's heredocs use a pipe or a temporary
   file). Plan part F4's bootstrap record is where a handle for an fd arrives. The plan's "a child reads what the parent wrote"
   is checked with a fork child (`t_memfd fork`) and in one process over a socketpair (`t_unix memfd`), and the two-process
   half of INV-0413-2's "in another process" by the host tests over a fake kernel.
6. **`mmap` of a descriptor.** `MAP_SHARED` or `MAP_PRIVATE`, not both and not neither, no `MAP_FIXED`, a length above 0
   (rounded up to pages) and an offset that is a page multiple, inside the descriptor's size rounded up to a page (`EINVAL`
   past it, and for a descriptor with no object: Linux maps and faults, this does not). `PROT_WRITE` of a shared mapping needs
   a descriptor open for writing (`EACCES`) that is not sealed (`EPERM`). `MAP_PRIVATE` is a copy in an object of its own,
   writable, made once; it is not fixed-size-making. `PROT_EXEC` on any `mmap` is `EACCES`, anonymous included, and `mprotect`
   with `PROT_EXEC` is `EACCES` where it is `ENOSYS` for every other `prot` (ADR-0100 item 6). A memory descriptor is
   not a file: `MAP_SHARED` of a regular file stays `ENODEV`, and `MAP_SHARED|MAP_ANONYMOUS` too (ADR-0431's follow-up).
   `read`, `pread`, `write`, `pwrite` and `lseek` copy through a mapping the library makes for the call and unmaps; `fstat` says
   a regular file of the descriptor's size, mode 0600.
7. **`shm_open`** takes `SHM_ANON` (FreeBSD's `((char *)1)`, in `<zoraya/shm.h>`, which musl's `<sys/mman.h>` includes by
   `ports/musl/zoraya/0004-shm-anon.patch`) with `O_RDWR`, optionally `O_CREAT|O_EXCL|O_TRUNC|O_CLOEXEC`; the descriptor is
   close-on-exec as musl's own `shm_open` makes it. Another access mode or flag is `EINVAL`; any name is `EACCES`;
   `shm_unlink` is `ENOENT`. It is not musl's `shm_open` (that opens `/dev/shm`).
8. **Not built here.** `msync`, `mremap`, `madvise` and `mlock` (ADR-0413 item 7 lists `madvise`'s no-ops; no port in the image
   calls it, so no name is added until one does); `F_SEAL_EXEC`; `MFD_HUGETLB` (`EINVAL`); memory descriptors in the bootstrap
   record (plan part F4).
9. **The names.**

```posix-surface-added
shm_open t_memfd
shm_unlink t_memfd
```

### Effect of the vision

Unchanged from ADR-0413: shared memory is a handle a program was given, with the rights the giver chose, and a seal is the giver
giving less. A receiver can never write what it was handed read-only, and a writer cannot be made to lose bytes by a resize.

## Consequences

- **Positive.** WebKit's `SharedMemoryUnix.cpp` and Ladybird's `anon_create` have what they call; Python's
  `multiprocessing.shared_memory` with `SHM_ANON`-style use works within a `fork`; every refusal is an `errno` a Linux
  program tests; the frames of a shared buffer are charged to the maker alone.
- **Negative.** A seal is the holder's own; a sealed handle's earlier copies (a receiver's writable one) stay writable, which is
  why `F_SEAL_WRITE` is refused after one left; `memfd_create` no longer survives `exec` and a spawn cannot be given one until
  F4; a descriptor cannot grow once mapped or sent.
- **Follow-up.** Plan part F4 gives a spawned child a memory descriptor; the browser parts use `F_SEAL_FUTURE_WRITE` for
  read-only buffers; `MAP_SHARED|MAP_ANONYMOUS` if a port asks.

## Invariants

- **INV-0432-1.** A message that carries memory descriptors that a peer's library did not build (any bytes, any handle count,
  any size or seal bits) is dropped with its handles closed and never panics the receiver, and a record that overstates its
  access holds a handle the kernel refuses to map writable. *Check:* `cargo test -p zoraya-posix` (`unix::tests`, the memory
  record's decoder; `tests/memfd.rs`, `a_record_that_lies_about_write_access_still_holds_a_read_only_handle`).
- **INV-0432-2.** No memory descriptor crosses `spawn` or `exec` before plan part F4: an open one without `O_CLOEXEC` makes the
  spawn `EBADF` with no handle left made, and one with it is not inherited. *Check:* `cargo test -p zoraya-posix`
  (`tests/memfd.rs`, `a_memory_descriptor_does_not_cross_a_spawn_yet`).
- **INV-0432-3.** Every handle a memory descriptor holds is closed exactly once: by `close` of its last fd, by a replacement of
  its object, or by a seal's downgrade; a private mapping's copy goes with its `munmap`. *Check:* `cargo test -p zoraya-posix`
  (`tests/memfd.rs`, the handle counts of `the_first_ftruncate_makes_the_object_and_a_later_one_replaces_it` and
  `a_private_mapping_is_a_copy_made_once`).

## Open questions

- **Known limitations of the review (F3 notes 5 and 6).** A `fork` child keeps a handle to each of its parent's `MAP_PRIVATE` copy
  objects (the snapshot duplicates it) and does not use it; the parent's copy, charged to the parent's job, lives until the child
  unmaps it or exits, also after the parent unmapped it (the anonymous `mmap` table has the same pattern from before F3). In
  `memory_mmap` the `EACCES` for a writable shared mapping of a handle that is not writable (`!m.writable`) is unreachable: a
  handle loses `WRITE` only by a seal (`EPERM` first) or arrives read-only (`access` `EACCES` first); the kernel's refusal of a
  record that lies is the check that holds. A mutant of that branch is equivalent.
- Seals are not shared between holders (item 1). WebKit's `createHandle(ReadOnly)`, read in its 2.54 source by the first engine
  part, decides whether a second descriptor made by `fcntl`-less duplication with fewer rights is also wanted.
- Whether a memory descriptor sent to a spawned child should carry the size and seals in the bootstrap record (F4) as the wire
  record here does.
- A `ftruncate` that replaces the object while another thread of the process maps it for a `read` is serialised by the library
  lock (every call holds it); a mapping made by `mmap` of the old object stays on the old object (a descriptor fixed by a shared
  map is never replaced).

## Revisit triggers

- A port that needs `MAP_SHARED` of a file, `msync`, `mremap` or a growing shared buffer (ADR-0413's triggers).
- Channel endpoints or memory objects that can be resized: item 3's `EBUSY` becomes a resize.

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
