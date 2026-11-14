# ADR-0415: Memory for big programs: 4096 mappings, read-only segments shared between processes, reservations, a 2 GiB browser guest

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0020, ADR-0021, ADR-0031, ADR-0033, ADR-0051, ADR-0100, ADR-0108, ADR-0260, ADR-0261, ADR-0266, ADR-0300, ADR-0320, ADR-0410, ADR-0413
- **Evidence:** CP-14, CP-16, DM-030, DM-034, GR-007, X-6, alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#memory, browser-engine/survey.md#the-engines-compared

## Context

Four facts of Zoraya's memory meet a browser (browser-engine/survey.md#memory):

- **256 mappings per process**, the same number in the kernel (`MAX_MAPPINGS`) and `libposix` (`MAPPING_SLOTS`); every
  private `mmap`, thread stack and 1 MiB heap step takes one. ADR-0320 measured CPython ending at about 250 MiB of
  small objects and left it, because the kernel's number is the same.
- **Eager commit**: a memory object's frames are taken when it is made (ADR-0051). No reservation of address space
  without frames exists, and `mprotect` is `ENOSYS`.
- **Every segment copied at spawn** (ADR-0108, ADR-0033): two processes of one program share no frames.
- **A 128 MiB guest** by default, with about 86 MiB for the session; the frame allocator was designed for 128 MiB to
  2 GiB (ADR-0021, ADR-0266).

A WPE build is three processes of programs that each link most of WebKit statically; the sum of their text and data is
not measured, and is of the order of 50 to 100 MB a program `(estimate, browser-engine/survey.md#the-engines-compared)`.
JavaScriptCore's allocators (bmalloc, libpas) and its Gigacage reserve large ranges and commit them later
(browser-engine/survey.md#libraries-and-interfaces: V8 and JavaScriptCore cages of 4 GB); WebKit offers
`USE_SYSTEM_MALLOC` to build without bmalloc. Genode named its lack of Linux-style overcommit as a possible show
stopper for its browser (alternatives-postmortem.md#genode, browser-engine/survey.md#how-other-systems-got-a-browser).

## Decision drivers

1. **Failure at the request** (CP-14): no overcommit and no OOM killer (DM-030), at the cost X-6 names: memory
   committed that may never be touched.
2. **Enough mappings for a threaded engine** without a heap that commits in large steps it may not use.
3. **Do not pay N times for one program's code**: read-only segments are the same bytes in every process.
4. **W^X kept** (CP-16): sharing read-only text changes no permission.
5. **A guest that runs a tab**, without changing what the tests run on.
6. **No memory-pressure protocol yet** (DM-034): what the engine's memory-pressure handler would hear is a later ADR.

## Considered options

### Option A: Linux's model: demand paging, overcommit, `mmap(PROT_NONE)` reservations, `mprotect`, a map count of 65530

- **Description:** Lazy commit and reservations as GR-007 describes, `vm.max_map_count` 65530
  (browser-engine/survey.md#libraries-and-interfaces).
- **Evidence for:** The engines' allocators assume it; nothing to configure.
- **Evidence against:** Overcommit is DM-030; a pager is a kernel project; it reverses ADR-0051.
- **Cost:** *(estimate)* Months of kernel work.
- **Precedent:** Linux, FreeBSD.

### Option B: Keep eager commit; raise the mapping limit; share read-only segments; system `malloc` in the engine; reservations only if the first build needs them

- **Description:** The four items of the recommendation below.
- **Evidence for:** Drivers 1 to 6; each item is small and measurable; WebKit's `USE_SYSTEM_MALLOC` is an upstream
  option.
- **Evidence against:** The engine's memory is committed when allocated; no reclaim under pressure; reservations, if
  needed, are a kernel object the first build may prove necessary.
- **Cost:** *(estimate)* 400 lines in the kernel (mapping tree, shared segments), 150 in `libposix`.
- **Precedent:** Fuchsia's VMOs mapped into VMARs, regions of an address space handed out by capability
  (browser-engine/survey.md#libraries-and-interfaces); Genode's explicit memory budgets traded along its component
  tree (alternatives-postmortem.md#genode).

### Option C: Keep everything; build the engine to fit

- **Description:** 256 mappings, copied segments; shrink WebKit's threads and heap.
- **Evidence for:** No kernel change.
- **Evidence against:** CPython already hits the 256; three copies of WebKit's text in a 2 GiB guest.
- **Cost:** Patches to the engine's threading.
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option B.** It changes ADR-0108's rule that every segment is copied (read-only
segments are shared) and the number in ADR-0300's table limit.

1. **Mappings.** The kernel's per-process limit becomes 4096, held in an ordered structure (a B-tree or sorted array of
   regions) so lookup and insertion stay logarithmic; `MAPPING_SLOTS` follows the kernel's number from one constant in
   `zoraya-abi`. A mapping's record is charged to the process's job as kernel memory (ADR-0260). `GROW_SIZE` stays 1 MiB
   until the engine's measurements show the heap's mapping count matters.
2. **Shared read-only segments.** An image object (ADR-0108) keeps the frames of each segment that is not writable
   (text, read-only data), filled once at `image_create`; every `spawn` of that image maps those frames read-only (and
   executable for text, as loaded today) instead of copying. Writable segments are still copied. The image's creator
   pays for the shared frames for the image's life (ADR-0261 item 8). W^X holds: no segment is ever writable and
   executable, and no shared frame is ever writable (INV-0033-2 unchanged).
3. **Reservations, only if needed.** If the first engine build cannot run without reserving address space, the shape
   is: `mmap(NULL, len, PROT_NONE, MAP_PRIVATE|MAP_ANONYMOUS|MAP_NORESERVE)` reserves a range with no frames (a kernel
   reservation object counted as one mapping); `mprotect(addr, n, PROT_READ|PROT_WRITE)` or `mmap(MAP_FIXED)` inside it
   commits that sub-range as a new memory object (one mapping each, frames now, `ENOMEM` now); `mprotect(PROT_NONE)` or
   `munmap` of a committed sub-range releases it. `PROT_EXEC` is refused everywhere (INV-0413-5). Without that need
   `mprotect` stays `ENOSYS`.
4. **The engine's allocators.** WebKit is built with `USE_SYSTEM_MALLOC=ON` (no bmalloc, libpas or Gigacage, so no
   multi-gigabyte reservations); JavaScriptCore's heap blocks come from `libposix`'s `posix_memalign`. Whether its
   structure heap still reserves a range is the first build's question (open questions).
5. **The guest.** `tools/run-qemu.sh`'s default stays 128 MiB (the tests' machine); the browser's test step and
   `make run-browser` pass `--mem 2G`; the frame allocator is tested at 2 GiB. Untrusted rooms keep a quarter of the
   session by default (ADR-0266), 512 MiB on that guest.
6. **What the person sees.** `top ROOM` shows each process's committed memory (all of it is committed); the image's
   shared frames are shown once, on the line of the program's image, not on each helper.

### Effect of the vision

For target 2 nothing changes in what a program can reach: shared text is read-only frames of an image it was given.
An agent's browser fails a large allocation at the call, inside its room's limit, and never takes memory from another
room by overcommit. Target 3: big programs (the browser, CPython with large heaps) run. Networking: none. Threads:
128 stacks fit with room to spare in 4096 mappings.

### Examples

```
$ python3 -c "x = [bytearray(100) for _ in range(5_000_000)]; print(len(x))"
5000000
```

in a 2 GiB guest: it ran out at 256 mappings before (ADR-0320); now it ends only at the room's or the machine's
memory, with `MemoryError`.

```
$ top home
ROOM  JOB                    MEM      PROCS
home  browser                412M     3
        (image, shared)       71M
```

## Consequences

- **Positive.** Helpers of one program share their code; a heap can grow past 250 MiB; reservations have a defined
  shape if needed.
- **Negative.** A bigger kernel structure per process; an image object holds frames for its life; the browser guest is
  sixteen times the default.
- **Follow-up.** The kernel's mapping tree and its tests; shared segments in `image_create` and `spawn`; the 2 GiB
  frame-allocator test; the first engine part's numbers (text size, mappings in use, memory per tab).

## Invariants

- **INV-0415-1.** A process can hold 4096 mappings and the 4097th is refused with `LimitReached` without side effect,
  and `MAPPING_SLOTS` equals the kernel's limit. *Check:* an in-kernel test (`kernel/src/testing.rs`) mapping to the
  limit; a const assertion in `libposix` against `zoraya-abi`'s constant.
- **INV-0415-2.** No frame of an image's shared segments is ever mapped writable, and two processes spawned from one
  image map the same frames for its read-only segments. *Check:* an in-kernel test spawning one image twice and
  comparing the physical frames of a text page; INV-0033-2's test unchanged.
- **INV-0415-3.** If reservations are built, no reserved range ever has frames except where committed, and committing
  fails with `NoMemory` at the call when frames or the job's limit are short. *Check:* `cargo test -p zoraya-posix`
  and an in-QEMU test that reserves 1 GiB in a 256 MiB room and commits until refused.

## Open questions

- Whether JavaScriptCore with `USE_SYSTEM_MALLOC` still reserves its structure heap (4 GB on 64-bit is reported
  `[unverified]`); the first build answers it and item 3 is built only then.
- The sizes: text and data of each engine program, mappings in use by a Web process with a page, memory of one tab.
  Measured by the first engine part (`make test-browser` with the runner's memory report).
- Whether the frame allocator's bitmap and next-fit search stay fast at 2 GiB under TCG.

## Revisit triggers

- Memory per tab over 1 GiB for a simple page: reconsider lazy commit (a superseding ADR of ADR-0051).
- A memory-pressure protocol (DM-034) becomes a decision: the engine's handler is wired to it.

## Architect's decision
