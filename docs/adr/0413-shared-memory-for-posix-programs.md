# ADR-0413: Shared memory for POSIX programs: anonymous memory descriptors over memory objects, `MAP_SHARED`, seals by rights

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0051, ADR-0103, ADR-0108, ADR-0261, ADR-0262, ADR-0342, ADR-0410, ADR-0412
- **Evidence:** CP-01, CP-14, CP-16, DM-019, DM-031, DM-032, GR-007, X-6, alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#memory, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WebKit's `SharedMemoryUnix.cpp` makes a `memfd_create` (else `shm_open`), sizes it with `ftruncate`, maps it
`MAP_SHARED` and sends the descriptor over IPC; pages are painted into such buffers and big IPC bodies travel in them
(browser-engine/survey.md#wpe-webkit-in-detail). Ladybird's `anon_create` is `memfd_create` on Linux and FreeBSD,
`shm_open(SHM_ANON)` where it exists, seals `F_SEAL_SHRINK|F_SEAL_GROW`, maps `MAP_SHARED`, and reads payloads back
with `pread` (browser-engine/survey.md#ladybird). Genode implemented its Chromium's shared memory with `mmap`
(alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser).

Zoraya's memory object is what these need: fixed size, frames committed at creation, mappable by several processes,
read-only by a READ-only duplicate, paid by its creator's job for its whole life (ADR-0051, ADR-0261 item 8;
browser-engine/survey.md#memory). `libposix` offers none of it: `mmap` is private and anonymous, `MAP_SHARED` is
`ENODEV`, `memfd_create` is an unmappable `tmpd` file, `shm_open` is absent (browser-engine/survey.md#the-posix-library).

## Decision drivers

1. **No global names** (CP-01): named POSIX shared memory and System V keys are the global namespace DM-019 records.
2. **Truncation hazards** (DM-031): a shrunk shared file faults its mappers; a size fixed once mapped removes the class.
3. **Read-only sharing by rights, not by trust**: a reader gets a READ-only handle (ADR-0051), the kernel enforces it.
4. **Exhaustion at the request** (CP-14): a memory descriptor's frames are committed at `ftruncate`, which fails with
   `ENOMEM`, never later at a touch (the cost X-6 names: memory reserved that may never be touched).
5. **W^X** (CP-16, DM-032): no executable shared mapping.

## Considered options

### Option A: As Linux has it: `memfd_create` with every seal, `shm_open` with names under `/dev/shm`, file mappings

- **Description:** Full `memfd_create` and `fcntl` seals, named `shm_open`, `MAP_SHARED` of regular files
  (browser-engine/survey.md#libraries-and-interfaces); demand paging behind it (GR-007).
- **Evidence for:** Every program unmodified.
- **Evidence against:** Names (driver 1); file mappings need a pager in `fsd` and bring DM-031's `SIGBUS` on truncation;
  demand paging contradicts eager commit (driver 4).
- **Cost:** *(estimate)* A pager and lazy commit in the kernel: months.
- **Precedent:** Linux, FreeBSD.

### Option B: Anonymous memory descriptors over memory objects; `MAP_SHARED` of them; seals expressed as rights

- **Description:** `memfd_create` and `shm_open(SHM_ANON)` return a memory descriptor; its first `ftruncate` creates a
  memory object of that size; `mmap(MAP_SHARED, fd)` maps it; `SCM_RIGHTS` passes the object (ADR-0412);
  `F_SEAL_FUTURE_WRITE` makes later handles and mappings read-only; named `shm_open` is refused.
- **Evidence for:** Drivers 1 to 5; the object exists; Capsicum allows only anonymous `shm_open` in capability mode
  (browser-engine/survey.md#libraries-and-interfaces); Fuchsia's VMOs are passed by handle with rights.
- **Evidence against:** A memory descriptor cannot grow once mapped or shared; `F_SEAL_WRITE` only where no writable
  handle has left the process; file mappings stay absent.
- **Cost:** *(estimate)* 450 lines in `libposix` with host tests.
- **Precedent:** Fuchsia (VMO handles), Windows (sections passed by handle), Capsicum (`SHM_ANON`).

### Option C: A shared-memory service with named regions per program

- **Description:** A service holds regions by name inside a room; programs open them by name.
- **Evidence for:** Named regions for programs that need them.
- **Evidence against:** Neither engine names its regions; a service per region is the System V namespace again (DM-019).
- **Cost:** *(estimate)* A service and a protocol.
- **Precedent:** System V shared memory; Windows' `Global\` names.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **Making one.** `memfd_create(name, flags)` (`MFD_CLOEXEC`, `MFD_ALLOW_SEALING`; `MFD_HUGETLB` is `EINVAL`) and
   `shm_open(SHM_ANON, O_RDWR, mode)` return a memory descriptor of size 0. The name is kept for `fstat` and
   `/proc/self/fd`-style reports only. `shm_open` of any other name is `EACCES`; `shm_unlink` is `ENOENT`.
2. **Sizing.** The first `ftruncate(fd, n)` creates the memory object of `n` bytes rounded up to pages, frames committed
   now (`ENOMEM` when the frames or the room's limit run out, ADR-0263). A later `ftruncate` to the same size succeeds; to
   another size it succeeds only while the descriptor has never been mapped or sent (the library replaces the object and
   copies), else `EBUSY`. So `F_SEAL_SHRINK` and `F_SEAL_GROW` are accepted and always hold once mapped.
3. **Mapping.** `mmap(addr=NULL, len, PROT_READ[|PROT_WRITE], MAP_SHARED, fd, off)` maps `len` bytes at page offset
   `off` of the object; `PROT_WRITE` needs a writable descriptor; `PROT_EXEC` is `EACCES`; `MAP_PRIVATE` of a memory
   descriptor is a private copy (one memory object, copied once). `munmap` takes the whole mapping (as today).
   `read`, `pread`, `write`, `pwrite` and `lseek` on the descriptor copy through a mapping the library keeps.
4. **Seals.** `F_ADD_SEALS`: `F_SEAL_SEAL`, `F_SEAL_SHRINK`, `F_SEAL_GROW` always accepted; `F_SEAL_FUTURE_WRITE` makes
   every later handle the descriptor yields (a `dup` excepted, `SCM_RIGHTS`, a spawn) and every later mapping
   read-only, while existing mappings keep their access, as Linux defines it; `F_SEAL_WRITE` is accepted only if the
   process holds no writable mapping and no writable handle has left it, else `EBUSY`. `F_GET_SEALS` reports them.
   Without `MFD_ALLOW_SEALING` only `F_SEAL_SEAL` is set.
5. **Who pays.** ADR-0261 item 8 stands: the creator's job pays for the frames for the object's life. The Web process
   pays for the buffers it paints and sends; the UI process for the ICU data it shares (ADR-0417).
6. **No executable shared memory.** `PROT_EXEC` on any mapping stays refused (ADR-0051 INV-0051-2).
7. **Not offered.** `MAP_SHARED` of a regular file, `msync`, `mremap`, `madvise` beyond `MADV_DONTNEED` on a memory
   descriptor (a no-op: frames are committed) and `MADV_WILLNEED`/`MADV_POPULATE_READ` (no-ops that succeed).

### Effect of the vision

For target 2 shared memory is a handle an agent was given, never a name it can guess; a reader cannot write what it was
handed read-only, whatever it is. Target 3: painted pages move between processes without a copy. Networking: none.
Threads: none.

### Examples

A WebKit page paints into a 4 MiB buffer the Web process made; `top home` charges the 4 MiB to `WPEWebProcess`; the UI
process maps the same frames read-only and copies them into its window (ADR-0419).

## Consequences

- **Positive.** Both engines' shared memory, and Python's `multiprocessing.shared_memory` with `SHM_ANON`-style use,
  work over the existing kernel object.
- **Negative.** A descriptor's size is fixed once used; no file mappings; named `shm_open` refused.
- **Follow-up.** The memory descriptor kind in `libposix`, a `posix-surface-added` block, tests with ADR-0412.

## Invariants

- **INV-0413-1.** A memory descriptor's object never changes size after it was first mapped or sent. *Check:*
  `cargo test -p zoraya-posix` (every order of `ftruncate`, `mmap`, `dup`, send).
- **INV-0413-2.** A handle or mapping obtained after `F_SEAL_FUTURE_WRITE` is not writable, in this process or another.
  *Check:* host test of the library's rights and an in-QEMU test where a receiver's write to its mapping faults and ends
  it (the test's parent sees the fault reason).
- **INV-0413-3.** `shm_open` succeeds only with `SHM_ANON`. *Check:* host test.
- **INV-0413-4.** The frames of a shared memory object are charged to exactly one job, its creator's, however many
  processes map it. *Check:* the kernel's accounting test over two jobs (ADR-0261's tests) with an object made in one
  and mapped in the other.
- **INV-0413-5.** No `mmap` or `mprotect` of `libposix` produces an executable mapping. *Check:* `cargo test -p
  zoraya-posix` (every `prot` combination) and INV-0051-2's kernel test.

## Open questions

- Whether WebKit's `SharedMemory::createHandle(ReadOnly)` uses seals or a second descriptor; read in the 2.54 source
  by the first engine part.

## Revisit triggers

- A port that needs `MAP_SHARED` of a regular file (a database): a pager in `fsd`, its own ADR.
- Memory objects that can grow (ADR-0051's follow-up): item 2's `EBUSY` can become a resize.

## Architect's decision
