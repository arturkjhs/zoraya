# ADR-0102: `snapshot_process`

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0003, ADR-0004, ADR-0021, ADR-0031, ADR-0034, ADR-0036, ADR-0040, ADR-0041, ADR-0042, ADR-0046, ADR-0050, ADR-0051, ADR-0056
- **Evidence:** CP-05, CP-14, X-10, X-6, DM-001, DM-030, GR-005, alternatives-postmortem.md#exokernel-mit-aegis--xok, stage-6/sources.md#libc-implementations, stage-6/sources.md#posix-semantics, task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain, task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1

## Context

ADR-0003 rule 3 defines `fork` for `posix` jobs as one kernel call, `snapshot_process`: a new process whose address
space is a copy-on-write clone of the caller's, whose handle table holds duplicates of every duplicable handle at the
same slots, and whose single thread starts with the caller's registers and a distinguishable return value. ADR-0002
rule 3 makes it the one personality-only kernel facility. This ADR decides its mechanics: page tables and frames, what
happens to memory-object mappings (ADR-0051's deferred question), to handles that cannot be duplicated (ADR-0050's,
with ADR-0055's "handoff ... not introduced"), to the thread, and what it costs per fork.

**What is measured.**

- dash is the only program that forks: 1,169 no-exec forks in the core runs, 1,167 of them in W4 (764 in `configure`,
  403 in make); T0 and T1 together have 2 (`task-02/conflict-impact.md#x-2-no-fork-cp-05-vs-hosting-the-c-toolchain`).
  Every other program creates processes vfork-style or with `posix_spawn`, and ADR-0003 maps those to `spawn`.
- W10's background jobs and pipeline stages are forks that exec, plus one subshell
  (`task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1`).
- The kernel as built: every page of a process is mapped at creation and owned by its `AddressSpace`, which keeps the
  frames in a list; the frame allocator keeps two bits per frame and no reference count (ADR-0021, `docs/design.md`
  "Memory (as built)"); memory-object mappings are recorded separately with their holder (ADR-0051); any fault from
  EL0 other than `svc` ends the process (ADR-0034), and the one resumed kernel fault is the user copy (ADR-0036).
- Measured on this kernel under HVF on 128 MiB: `memory_create` of all memory takes about 9 ms, that is zeroing at
  roughly 14 GB/s; a 1 MiB write through a memory object runs at about 4 GB/s, and under TCG at about 500 MB/s
  (`docs/design.md`, "Known limitations" and the storage table). These bound a copy's cost from above and below.
- Linux `fork` copies page tables only and marks pages copy-on-write ([P3], `stage-6/sources.md#posix-semantics`).
  Cygwin's `fork` is "a non-copy-on-write implementation similar to what was present in early flavors of UNIX": it
  copies `.data`, `.bss`, stack and heap into a suspended child and must reproduce the parent's layout, and the
  document calls it inevitably inefficient and occasionally failing ([C1], `stage-6/sources.md#libc-implementations`).
  Xok's fork took about 6 ms against under 1 ms on OpenBSD because it could not share page tables
  (`alternatives-postmortem.md#exokernel-mit-aegis--xok`).

**What is assumed.** dash's resident size at a fork point is not measured (X-6); the estimates below take a dash image
of 1 MiB `[assumption]`: text and data of a static dash, a 64 KiB stack, and a heap of a few hundred KiB.

**Forces from TASK-01.** `fork` is the root of DM-001 and encourages overcommit (DM-030); under strict commit (CP-14) a
fork must reserve a full copy or overcommit (X-6). A snapshot as a separate facility is what DM-001's alternatives
list offers for the cases that want a copy of the whole process. GR-005 records that `posix_spawn`'s constant cost
comes from a vfork-style shared address space, not from avoiding a copy.

## Decision drivers

1. **The data confines fork to the shell and almost entirely to T2** (what is measured): the mechanism may be simple
   now and fast later, and T2 is gated on measuring it (ADR-0005).
2. **Strict commit** (CP-14, ADR-0051's eager frames, ADR-0056's deferred note): a snapshot commits a full copy of the
   parent's writable memory at the call, whatever mechanism copies it.
3. **No new fault class in the kernel without a decision** (ADR-0034, ADR-0036): the normal kernel resumes exactly one
   fault, and a user fault ends the process.
4. **The child is a POSIX process**: a copy of all memory, no sharing with the parent (`MAP_SHARED` is out of scope,
   ADR-0005), its own pid and process job (ADR-0104), the parent's dispositions but an empty request queue ([P3]).
5. **Non-duplicable handles need an answer** (ADR-0003 rule 3, INV-0003-7, ADR-0055): the fd table's channels, the
   broker channel and the port are absent in the child, and the child must still work.
6. **A failed snapshot changes nothing visible** (INV-0042-3's shape): dash sees `fork` return -1 and goes on.

## Considered options

### Option A: Copy-on-write with demand faults (Linux `fork`)

- **Description:** The child gets a copy of the parent's page tables; every writable page in both is marked read-only
  and shared, with a reference count per frame; the first write by either side faults into the kernel, which copies
  the page, remaps it and resumes the thread. Read-only and executable pages are shared for good.
- **Evidence for:** The fastest fork for a large parent: cost proportional to page tables, not pages ([P3]). The
  mechanism Linux has carried for decades, and the shape ADR-0003 rule 3 names.
- **Evidence against:** It needs a frame reference count the allocator does not have (ADR-0021), a new fault class the
  normal kernel resumes (against ADR-0034 and INV-0036-1: the user copy routine would also have to resolve a
  copy-on-write fault on a user page it writes through `sttrb`), and page-table walks on every fault. Under strict
  commit it saves no memory: the full copy must be reserved at the call anyway (driver 2, X-6), so the gain is time
  deferred, not memory saved. For a 1 MiB dash the deferred work is small (Option B's estimate).
- **Cost:** *(estimate)* A reference-count table (2 bytes a frame, 64 KiB per 128 MiB), a resumed-fault path with
  its own superseding ADR for ADR-0034 and ADR-0036, changes to `usercopy.rs`, and the TLB maintenance of
  break-before-make per fault. A thousand lines and the kernel's hardest new path.
- **Precedent:** Unix since 3BSD; Zircon's `ZX_VMO_CHILD_SNAPSHOT` does the same at the memory-object level
  `[outside evidence: fuchsia.dev vmo_create_child, not fetched]`.

### Option B: Eager copy at the call (early Unix, Cygwin's shape, done in the kernel)

- **Description:** `snapshot_process` builds the child's address space by walking the parent's tables, allocating a
  fresh frame for every mapped page, copying its contents through the direct map, and mapping it with the same
  attributes; text and read-only data are copied too, since frames have no reference count. Memory-object mappings
  are copied into fresh objects (below). No page is shared, nothing faults later.
- **Evidence for:** Drivers 2, 3 and 6: the copy is the commit, no fault path, and the call either completes or
  frees what it allocated. The kernel already has every piece: a table walker, frame allocation, the direct map, and
  the loader's copy-and-map path (ADR-0033). The known cost is the copy: for 1 MiB at the measured memory rates about
  0.1 to 0.3 ms under HVF and 2 to 3 ms under TCG `[estimate from the rates above]`, so W4's 764 forks cost about
  0.2 s and about 2 s respectively, against a configure run that takes tens of seconds `[assumption]`. Cygwin's
  troubles came from reproducing a layout across a foreign process creation and from copying from user space; the
  kernel has the parent's tables in hand and copies from the direct map.
- **Evidence against:** Cost grows with the parent's size; a large forking parent would pay for every fork what Linux
  pays once. Text pages are copied although they never change. Interrupts stay masked for the copy (the kernel is not
  preempted, ADR-0030): a 1 MiB copy is a 2 to 3 ms stall under TCG, in the range of the stalls `docs/design.md`
  already lists for `memory_create`. ADR-0003 rule 3 says "copy-on-write", so the letter of an accepted ADR changes.
- **Cost:** *(estimate)* 300 to 500 lines in the kernel, reusing the loader's path; a host-tested walker in
  `zoraya-pagetable` for "every mapped page with its attributes".
- **Precedent:** Early Unix; Cygwin (slow because of Win32, not because of copying as such); the stage-2 loader.

### Option C: Eager copy from user space (library-composed fork, ADR-0003 Option D's shape)

- **Description:** No snapshot call. `libposix` creates an empty process with `spawn` of a stub image, then copies
  its own memory into the child through memory objects and a cross-process write call, and sets the child's registers.
- **Evidence for:** No new process-creation path in the kernel; the primitives would serve debuggers later.
- **Evidence against:** ADR-0003 rejected exposing cross-process construction as ABI before a user exists (X-10), and
  the data gives one user. Copying through user space doubles the copies and needs the child's layout to match,
  which is Cygwin's problem exactly ([C1]). Several system calls per fork.
- **Cost:** *(estimate)* New cross-process ABI with rights and a security review each; slower than B.
- **Precedent:** Cygwin; Zircon's process builder (with a launcher service in front of it).

### Option D: No snapshot: a fork-free dash (ADR-0003 Option C)

- **Description:** Restructure dash so that subshells, command substitution and builtin pipeline stages run
  in-process or by re-executing the shell with serialised state.
- **Evidence for:** No personality-only facility at all, CP-05 absolute, no X-6.
- **Evidence against:** ADR-0003 weighed and rejected it for its unmeasured, probably large cost at the fragile point
  of T2; nothing new has been measured since. It would reopen an accepted decision without new evidence.
- **Cost:** *(estimate)* Thousands of lines in dash, unmeasured.
- **Precedent:** None in the research files runs `configure` without fork.

## Recommendation

**Recommendation (not a decision): Option B**, with Option A as the recorded upgrade if the W4 measurement demands it.

**The call.** `snapshot_process(buffer, length)` takes one flat buffer in ADR-0042's form (magic `ZSN1`, header size,
flags, `job`, `group` handle, move count, reserved zero, then move entries `{slot, handle}`), and returns the child's
process handle with the rights `spawn` gives. It is tagged `#[personality_only]` (INV-0002-2) and fails with
`AccessDenied` in a job without the `posix` bit (INV-0003-2).

1. **Job.** The child is created in `job`, which must carry `CREATE_PROCESS` and be in the caller's job tree; `group`
   is as in `spawn` (ADR-0046), or 0. ADR-0104 makes every POSIX process a job of its own, so `libposix` passes the
   child's process job, which the broker created inside the right group job. This replaces rule 3's "in the same
   job" (tension below).
2. **Memory.** For every page mapped in the caller's tables, the kernel allocates a frame, copies the page through
   the direct map, and maps it at the same address with the same attributes, including the read-only startup block
   (ADR-0042) and the stack. For every memory-object mapping it creates a new object of the mapping's length, copies
   the mapped range into it, and maps the new object at the same address with the same permissions; the child's
   `mappings` hold the new objects. The child therefore shares no memory with its parent or with anyone (driver 4).
   Device mappings and DMA pins (ADR-0054) refuse the snapshot with `BadState`; a POSIX process has none.
3. **Handles.** Every slot whose type is duplicable is duplicated at the same slot with the same rights: processes,
   jobs, memory objects, stream ends (ADR-0101), console, directory, image, event tokens, `Power`. Non-duplicable
   types are absent: ports, channel ends, interrupts, pins, devices (`Device` is duplicable but a POSIX process holds
   none). The `abi` crate marks each object type `Duplicable` or not in `OBJECT_TYPES` (ADR-0040's follow-up), and
   INV-0003-7 covers the rule. The child's handle to a memory object that the parent had mapped refers to the parent's
   object, while the child's mapping is its own copy; `libposix` never uses a mapped object's handle except to close
   it, and `memory_unmap` takes an address (ADR-0051), so the difference is invisible to POSIX code.
4. **The move list.** Each entry moves a handle out of the caller's table into the child's at `slot`, overriding
   whatever duplication would have put there, with the handle's full rights (a move needs `TRANSFER`, ADR-0040). This
   is how the child gets the channels it cannot inherit: `libposix` prepares them before the call (new file
   connections through `share`, ADR-0103; the child's broker channel and process job, ADR-0104) and lists them. The
   list is all or nothing: if any entry is invalid, the call fails before anything is allocated and the caller's table
   is unchanged. An endpoint with handles queued cannot be moved (`BadState`), as ADR-0050 says of a message; the
   ends `libposix` moves are fresh and empty. Up to `MAX_SPAWN_HANDLES` entries.
5. **Thread.** The child's one thread starts with the caller's registers as saved at the `svc`, with `x0` = `Ok` and
   `x1` = 0, so the `fork()` wrapper returns 0 in the child and the handle in the parent. Dispositions are copied
   (ADR-0004 rule 6); the request queue is empty; `REQUEST_PENDING` is clear; the child is not suspended unless
   `START_SUSPENDED` is set. A request arriving during the call reaches the parent only; the child receives requests
   from the moment the kernel adds it to its job, under the dispositions it copied (ADR-0004's open question).
6. **Failure.** `NoMemory` or `LimitReached` (frames, ASIDs, handle table, 255 processes) frees everything allocated
   and returns with no child; the moved handles stay in the caller's table (INV-0042-3's rule). `libposix` returns
   -1 with `EAGAIN` or `ENOMEM`, closes the prepared ends, and tells the broker.
7. **Accounting.** The copy is the full charge of CP-14: when ADR-0056 is built, the child's frames are charged to the
   child's job at the call, and the call fails with `QuotaExceeded` before copying if they do not fit.
8. **Interrupt latency.** The copy runs with interrupts masked, like `memory_create` (docs/design.md, "Known
   limitations"). For 1 MiB that is in the range of stalls the kernel already accepts at stage 4; ADR-0056 must bound
   it before untrusted code runs, as it must those.

**The `fork()` wrapper**, in order: ask the broker for the child's pid, process job and broker channel
(`fork_prepare`, ADR-0104, one round trip); `share` every connection in the fd table and the namespace table
(ADR-0103, one request per connection, replies awaited together); build the move list; `snapshot_process`; in the
parent, send the child's process handle to the broker (`fork_commit`) and record the child; in the child, write the
moved handles into the copied fd and namespace tables, create a fresh port, re-subscribe nothing, drop the parent's
child list, and return 0. Cost per fork `[estimate]`: the copy (above), 2 broker round trips, and one `share` per
connection: about 10 connections for a shell (`/` and `/tmp` directory connections, cwd, three stdio descriptions,
the terminal control connection), unmeasured.

**Tensions with accepted ADRs.**

- *ADR-0003 rule 3* says "copy-on-write clone". This ADR copies eagerly: the same contents, no sharing, no deferred
  work. Accepting it reads rule 3's "copy-on-write" as "a copy", and keeps Option A as the revisit. Rule 3 also says
  "in the same job"; item 1 puts the child in a job of its own inside the parent's group job (ADR-0104), which
  `job_signal` on the group still reaches. Rule 3's "exactly what dash's `forkshell` needs and nothing more" gains the
  move list, which ADR-0055's review found necessary (its finding 1).
- *ADR-0002 rule 3(b)*: the move list is a parameter of the one gated facility, not a second facility; INV-0002-2's set
  stays at one entry. The handoff object of TASK-10 would have been a second one, which is one reason it is not chosen
  (ADR-0103 gives the others).
- *ADR-0051*'s deferred question is answered against the natural reading of "must stay shared with its other holders":
  a POSIX child shares nothing. A native program that wanted a shared mapping across a snapshot cannot call
  `snapshot_process` anyway (INV-0003-2).
- *ADR-0031*: 255 live processes. W1 to W3 and W10 hold a handful at once; a parallel W4 configure may hold more
  `[unmeasured]`; ADR-0110 counts the peak.
- *ADR-0042*: a second flat-buffer call, with its own magic, versioned the same way.

### Effect of the vision

For target 2 the eager copy is the honest choice: a sandbox pays for its fork at the call, and a quota (ADR-0056) can
refuse it before any frame is touched, which copy-on-write with deferred faults cannot do as cleanly. For target 3,
a desktop with large forking programs would want Option A; the ABI does not change, only the kernel's mechanism and
the invariants that measure its cost. Threads: a snapshot copies the calling thread only, as POSIX `fork` does; a
multi-threaded parent is a later ADR. Networking: no effect.

### Deferred to a later stage

Option A's reference counts and resumed fault. Sharing text pages between processes of one image (a reference count
would give it to `spawn` too).

## Consequences

- **Positive.** No new fault class, no reference counts, no change to the memory model; the cost is one copy, visible
  and measurable on W4 as ADR-0005 requires. The move list closes the fork-reconnection question without a second
  gated facility.
- **Negative.** Fork cost grows with the parent; text is copied needlessly; a several-millisecond masked stall per
  fork under TCG. `libposix`'s fork does about a dozen IPC operations before the call.
- **Follow-up.** The `Duplicable` marker in `OBJECT_TYPES`; the flat buffer and its total decoder in `zoraya-abi`;
  the walker in `zoraya-pagetable`; the W4 measurement (ADR-0110).

## Invariants

- **INV-0102-1.** After `snapshot_process`, writing any user page in one process leaves the other's contents
  unchanged, memory-object mappings included. *Check:* an in-QEMU test whose child writes a pattern over every mapped
  page, including a mapped memory object, while the parent checks its own pages, in both directions.
- **INV-0102-2.** The move list is all or nothing: after a refused call every listed handle is still valid in the
  caller, and after a successful one none is, and each appears at its slot in the child. *Check:* in-QEMU tests with
  an invalid entry, with a full child table, and with a valid list, checking both tables.
- **INV-0102-3.** A failed `snapshot_process` returns the kernel's frame count and heap use to their values before the
  call. *Check:* in-QEMU test at frame exhaustion, with INV-0041-1's counters.
- **INV-0102-4.** The child's thread starts at the instruction after the caller's `svc` with the caller's registers
  except `x0` and `x1`, and with an empty request queue. *Check:* in-QEMU test comparing a register dump taken by the
  child with the parent's, and a request queued to the parent during the call observed only there.
- **INV-0102-5.** `snapshot_process` refuses a caller with a device mapping or a live DMA pin with `BadState`.
  *Check:* in-QEMU test in a `posix` job holding each.

## Open questions

- The time and memory per fork on W4 (X-6, ADR-0003's open question): ADR-0110's measurement, with the threshold for
  Option A set by the architect.
- The peak number of live processes in a parallel W4 configure against the 255 ASIDs.
- Whether text pages should be shared with a reference count once `spawn` of the same image many times is measured
  (W4 runs `/bin/sh` 136 times).

## Revisit triggers

- The W4 measurement exceeds the architect's threshold: Option A (copy-on-write, with the superseding ADRs for
  ADR-0034 and ADR-0036 it needs), or ADR-0003's other triggers.
- A process other than the shell is found to fork without exec (ADR-0003's trigger).
- ADR-0056 is accepted: item 7 becomes an invariant with a check.

## Architect's decision

