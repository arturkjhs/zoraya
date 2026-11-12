# ADR-0300: Threads in a process: the kernel's thread object and its four calls

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0002, ADR-0004, ADR-0030, ADR-0031, ADR-0032, ADR-0034, ADR-0040, ADR-0041, ADR-0044, ADR-0046, ADR-0051, ADR-0105, ADR-0261, ADR-0264, ADR-0270, ADR-0285
- **Evidence:** CP-05, CP-10, CP-14, DM-003, DM-004, alternatives-postmortem.md#amoeba, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, task-02/posix-surface.md#key-observations

## Context

Stage 9 (TASK-27, part 9b) must run C programs that create threads: ld.lld, CPython, SQLite's threaded users and OpenSSL
with its thread support all do. Today a process has exactly one thread. `Process::thread` is one `Option<Arc<Thread>>`
(`kernel/src/process.rs`); the process ends on that thread (ADR-0041); a request wakes that thread; `job_suspend` parks
that thread (ADR-0105 item 8); `Thread::process` is the only link from a thread to its process. The ABI has
`thread_yield` and nothing else about threads, and `Param::EntryPoint` ("code to run, only thread creation may take one",
INV-0004-1) names a call that does not exist yet. A `spawn`ed image starts one thread at its entry with a 64 KiB stack.

What is measured: no T0 to T2 program creates a thread; ld.lld creates four (`task-02/posix-surface.md#key-observations`).
What is read from the code: the blocking calls a user thread can be in are `port_wait` and `sleep_until` only (every other
call returns at once); `Port` already keeps a list of waiters, so several threads may wait on one port; a thread's
`TPIDR_EL0` is saved and restored at a switch and a snapshot child gets the caller's value, so a thread pointer needs no
kernel support beyond its first value; the user mapping table holds 256 mappings and the handle table 1024 slots;
`AddressSpace::find_gap` leaves one unmapped page above every mapping, so every mapping has an unmapped page below the
next one. What is assumed: that threads are used for concurrency of blocking work and for parallel reasoning on one CPU
(the kernel is one CPU, ADR-0030), not for speed.

DM-004 is the lesson: Unix made the process the unit of both ownership and execution and then retrofitted a rule for
threads onto every older feature. Zoraya's processes are already containers (CP-05, handles, address space, job
membership), so the split is free here if it is made now: the process owns the address space, the handle table and the
job membership; the thread owns a stack, registers, a thread pointer and a blocked state.

## Decision drivers

1. Threads must be objects named by handles, with rights, like every other kernel object (ADR-0002 INV-0002-1, CP-05).
2. No asynchronous interruption of user code (CP-10, INV-0004-1): the only call that takes code to run is thread creation,
   and nothing may run on a thread that did not ask.
3. Every limit surfaces at the request (CP-14): thread creation can fail for a job's limit, for the machine, and for the
   process's own bound, and fails with nothing changed.
4. The process-level behaviours that exist (end, stop, kill, fault, requests) must keep working with several threads
   and must not leave a user instruction running on an address space being torn down.
5. Bounded kernel work (ADR-0270): no call walks more than one step of 64 with interrupts masked.
6. The smallest ABI that lets musl's pthread code run (ADR-0302): create, exit, a way to name a thread to interrupt it.

## Considered options

### Option A: `clone`, Linux's way: one call that makes a task sharing parts of the creator's state

- **Description:** `thread_create` is `process_create_shared(flags)`: a new schedulable entity that shares the address
  space, handle table and signal state of its creator according to flag bits; its identity is a number (a tid) that other
  calls take (`tkill(tid)`).
- **Evidence for:** It is what musl's retained `pthread_create` already calls; the shim would be one function.
- **Evidence against:** Flag bits that grow (DM-004: every process-level feature needed a retrofitted rule); numbers as
  identity break ADR-0002 INV-0002-1 (every acted-on object is a `Handle`); the tid reuse problem is DM-003's pid reuse
  again.
- **Cost:** *(estimate)* small in the kernel, large in rules.
- **Precedent:** Linux (`clone`, LinuxThreads to NPTL, DM-004).

### Option B: A thread object inside a process, named by a handle (Zircon's split)

- **Description:** A process is a container of one address space, one handle table and its threads. `thread_create`
  returns a handle to a new thread object (signals: `TERMINATED`; rights `WAIT`, `SIGNAL`); `thread_interrupt` takes that
  handle. A thread has no id in the kernel.
- **Evidence for:** Drivers 1 to 3; Fuchsia keeps separate process and thread objects, and the thread is the only unit of
  scheduling (DM-004's "alternatives that exist"); Starnix emulates `clone` over exactly this split
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** A handle per thread uses a slot of 1024; the library must keep a table from its own thread
  numbers to handles (ADR-0302).
- **Cost:** *(estimate)* 400 lines in the kernel (the thread list in `Process`, two-phase end, the calls), 150 in the ABI.
- **Precedent:** Windows NT and Zircon.

### Option C: User-level threads in `libposix`, one kernel thread (M:1)

- **Description:** `pthread_create` makes a coroutine on the one kernel thread and switches at the library's wait points.
- **Evidence for:** No kernel change; `Posix` needs no lock.
- **Evidence against:** A thread that computes starves every other, and a blocking call in C code that bypasses the library
  blocks all of them; Amoeba's run-to-completion threads were "probably the worst mistake" of version 3.0 because a
  blocking call inside a critical region broke mutual exclusion (`alternatives-postmortem.md#amoeba`). It also makes no use
  of the CPU ceiling per thread (ADR-0264) and cannot be preempted by the 10 ms slice.
- **Cost:** *(estimate)* 600 lines of context switch in the library; the scheduler's guarantees are lost.
- **Precedent:** Early Unix thread libraries, Amoeba 3.0.

### Option D: A process per thread sharing memory (Plan 9's `rfork` with shared memory)

- **Description:** A new call makes a second process that maps the same memory objects; "threads" are processes with the same
  job.
- **Evidence for:** No new object type; the job already contains the group.
- **Evidence against:** An address space identifier per thread (255 live processes at most, ADR-0031); a handle table per
  thread, so a descriptor opened by one is not seen by the others, which POSIX requires; a fork copies only one of the group.
- **Cost:** *(estimate)* small in the kernel and unusable for POSIX.
- **Precedent:** Plan 9 `rfork(RFMEM)` `[outside evidence: Plan 9 manual, rfork(2)]`.

## Recommendation

**Recommendation (not a decision): Option B.** In detail:

1. **The process.** One address space, one handle table, one job, and from one to `MAX_THREADS` (128, a kernel constant)
   threads. `Process::thread` becomes a list of threads in a `Vec` reserved to its bound when the process is made, so a
   thread's creation and end allocate no list space. The first thread is the one `spawn` and `snapshot_process` make.
   128 threads leave room in the 256 mappings (one per stack) and in the 1024 handle slots (two per thread in the library,
   ADR-0302). A system-wide bound, `MAX_THREADS_SYSTEM` (1000), protects the deadline queue and the scheduler's walks
   (ADR-0301 item 5, ADR-0307 item 4).
2. **`thread_create(entry: EntryPoint, stack: Address, argument: Opaque, tls: Address) -> Handle`** (call 53). The new
   thread starts at `entry` in EL0 with `x0 = argument`, `SP = stack` (16-byte aligned, and `stack - 16` must lie in a
   writable mapping of the caller's space, else `InvalidArgument`), `TPIDR_EL0 = tls` (any value; the kernel never
   dereferences it), every other register zero. `entry` must be a user address aligned to 4. The caller gets a handle
   with rights `WAIT | SIGNAL`, not duplicable and not transferable, in the first free slot (`LimitReached` if there is
   none). The thread joins the end of the run queue. Order of checks, so that a refusal changes nothing: the process has
   room (`LimitReached`), the system has room (`LimitReached`), a handle slot is free, the job's `threads` figure then its
   memory (`OverQuota`, ADR-0307), the kernel stack and record (`NoMemory`); then the handle is put in the table last.
   The charge is the 64 KiB kernel stack and the thread record, to the caller's job, as ADR-0261 point 5 says of every
   kernel object; the user stack and the thread's TLS block are a memory object the caller made and pays for (ADR-0261
   point 8, ADR-0303).
3. **`thread_exit(clear: Address, unmap_base: Address, unmap_length: Size)`** (call 54), which does not return. In this
   order: if `unmap_length` is not zero, remove the caller's mapping that is exactly `[unmap_base, +unmap_length)` (a
   mismatch is ignored: the thread is ending), so a detached thread can free the stack it ran on; if `clear` is not zero,
   store the 32-bit value 0 at `clear` with one aligned store and wake every thread of the process waiting on it
   (ADR-0301), ignoring a bad address; then assert `TERMINATED` on the thread object, drop the thread, and switch away
   for good. The stack is never touched after the first step. If this was the last thread of the process, the process ends
   as `Exited` with code 0 (`libc` always calls `exit` first; this is a guard, not a feature).
4. **`thread_self() -> Handle`** (call 55) makes a new handle, rights `WAIT | SIGNAL`, to the calling thread, for the
   library to name its own thread (the first thread of a process is made by `spawn`, not by `thread_create`, so nobody
   gave it a handle). Each call spends a slot.
5. **`thread_interrupt(thread: Handle)`** (call 56), needing `SIGNAL`. It sets the target's one-shot `interrupted` flag and
   wakes the target if it is blocked in `port_wait`, `sleep_until` or `futex_wait`. The blocking call that consumes the
   flag returns `Interrupted` (a new `Status`, 17; the word in ADR-0155's table is `interrupted`, `EINTR` in `libposix`);
   a flag set while the target was not blocked makes its next such call return at once, so an interrupt is never lost.
   Nothing runs on the target (INV-0004-1 stands: the call takes no code). An interrupt of a thread that has ended is
   `BadState`.
6. **Ending a process with several threads is in two phases.** Any way of ending a process (`process_exit`,
   `process_exit_with_reason`, a fault, `job_kill`, a `Terminate` request, the last thread's exit) first fixes the exit
   record once (`Requests::exit`, with a new flag `ending`, set by the first to ask) and wakes every thread of the process
   that is blocked, in steps of 64 (ADR-0270). A thread that sees `ending` at its next return to EL0, or in a blocked
   call (which returns `Cancelled`), leaves: it drops its handle slot's object, counts itself out and wakes the owner.
   The thread that fixed the record, or the last to leave when the process ends from outside, is the owner: it waits for
   the count to reach one, then runs the teardown that `exit_current` runs today (handles, mappings, the zombie record).
   No sibling therefore runs a user instruction after the record is fixed, and none is running when the space is freed.
   A sibling parked for a spent CPU budget (ADR-0285 point 7) leaves at the next period boundary, as a parked single
   thread ends there today.
7. **Stop and resume.** `job_suspend` parks every thread of the process at its next return to EL0
   (`park_if_stopped` runs for each) and `job_resume` wakes all of them; a thread blocked in the kernel finishes its wait
   first, as today.
8. **Faults** end the process (ADR-0034 and INV-0004-6 are unchanged: there is still no exception channel bound to a POSIX
   job). **Requests** (`process_signal`, `process_take_request`) are per process, as now (ADR-0304 says which thread takes
   one).
9. **Scheduling.** Every thread is an equal on the one run queue; the CPU ceiling charges a thread's run time to its job as
   now (ADR-0264 point 3). A job's threads together get the ceiling.
10. **Bounded work** (ADR-0270). `thread_create` allocates one stack (16 frames) and a record: constant. Waking the threads
    of an ending process is at most 127 wakes in two steps of 64. `thread_exit` does one unmap of at most the stack's
    pages in steps. No call walks more than a step with interrupts masked.

**Tensions with accepted ADRs.** *ADR-0030:* the "one thread per process" sentence of design.md becomes "one or more";
the scheduler is unchanged. *ADR-0041:* a process ends on one of its threads, now the owner of item 6; the
zombie rule is unchanged. *ADR-0102 item 5:* a snapshot child still has one thread (ADR-0306). *ADR-0270 and ADR-0285:*
`exit_current`'s comment "the process has one thread, which is here" becomes true at the point where the owner has
the last thread. *ADR-0031:* `map_reserve`'s "the process's one thread is in this call" is false; ADR-0301 item 6 serializes
mapping calls per process.

## Consequences

- **Positive.** The kernel gets the smallest thread ABI that is object-shaped; no flag word; no number as identity. Every
  other state stays per process.
- **Negative.** `process.rs` changes in every place that says "its thread" (about 12). `Thread` gains a flag and a futex
  node. The 64 KiB kernel stack per thread is expensive: 128 threads are 8 MiB of kernel memory charged to the job.
- **Follow-up.** The `abi` table lines, a `Thread` object type, `Status::Interrupted`, the dispatcher, `zoraya-rt` stubs,
  ADR-0155's status word, `docs/design.md`; the host crate `zoraya-thread` (below) for the list and phase logic.

## Invariants

- **INV-0300-1.** `thread_create` is the only call in the `abi` crate that takes an `EntryPoint`, and a thread's handle has
  neither `DUPLICATE` nor `TRANSFER`. *Check:* INV-0004-1's lint over the `abi` table (it already names `thread_create` as the one allowed call) and a test over `OBJECT_TYPES` for the thread row.
- **INV-0300-2.** A refused `thread_create` changes nothing: the handle table, the job's usage and the process's thread list are
  as before, whichever of the checks refused. *Check:* an in-QEMU test per refusing check (process bound, system bound,
  handle table full, `threads` limit, memory limit), comparing `job_get_usage` and the frame count before and after.
- **INV-0300-3.** After a process's exit record is fixed, none of its other threads executes a user instruction, and the
  address space is freed with no thread of the process running or ready. *Check:* an in-QEMU test in which three threads
  spin in EL0 and a fourth calls `process_exit`, a `job_kill` of the same shape, and a fault; each thread stores a counter
  that the test's watcher reads after the exit, equal on two reads.
- **INV-0300-4.** `thread_interrupt` is never lost: an interrupt sent before the target blocks, or while it is blocked, makes
  exactly one of its next blocking calls return `Interrupted`. *Check:* an in-QEMU test with both orders, 1000 rounds each.
- **INV-0300-5.** `thread_exit` with a `clear` address stores 0 and wakes a waiter on it only after the thread has stopped using
  its stack: a joiner that unmaps the stack on wake never faults. *Check:* an in-QEMU stress of 500 create, join and unmap
  rounds.
- **INV-0300-6.** The thread list and the two-phase end are a `no_std` crate with host tests: any interleaving of "end requested",
  "leave" and "create refused" over four threads ends with the count at zero and one owner. *Check:* `cargo test -p zoraya-thread`.

## Open questions

- Whether `MAX_THREADS` should be 64: musl's thread list walks (`__synccall`, `pthread_getattr`) are linear.
- A cheaper kernel stack (16 KiB) for threads: the steps of ADR-0270 and the user copies need depth; measure what a thread's calls use.
- A native `thread_wait` (join by handle) is not added: musl's join is a futex wait on the `clear` word.

## Revisit triggers

- SMP: item 6's count and the futex lists need the locks ADR-0030 left out.
- A program that needs more than 128 threads, or a measured cost of the kernel stack.

## Architect's decision

(Left empty for the human.)
