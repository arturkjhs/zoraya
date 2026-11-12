# ADR-0306: `fork`, `exec` and process exit in a threaded process

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0003, ADR-0041, ADR-0102, ADR-0103, ADR-0104, ADR-0105, ADR-0270, ADR-0300, ADR-0302, ADR-0304
- **Evidence:** DM-001, DM-004

## Context

`fork` is `snapshot_process` (ADR-0102): the kernel copies every page of the caller's space into a new process in the
child's own job, duplicates the duplicable handles at the same slots, moves the listed ones, and starts one thread with the
caller's registers. The copy runs in steps with interrupts enabled between them (ADR-0270, `snapshot_plan` and
`snapshot_step`), so another thread of the parent may run between two steps and write pages that were copied and pages that
were not. `exec` is `spawn`, `exec_commit` to the broker and an exit of the old process (ADR-0104 item 6). `exit` is
`exit_group`.

DM-001 lists the failure a threaded `fork` has in Unix: the child gets only the calling thread, so locks held by other
threads, the malloc lock among them, stay locked in the child for ever, and only async-signal-safe functions may run
before `exec`. DM-004 records that every older feature needed a rule for threads. In `libposix` the locks that matter are
the library's own: the library lock and `MEM_LOCK` (ADR-0302 item 8), plus musl's (`__thread_list_lock`, the stdio
open-file-list lock, the `atfork` handlers' lock, the `abort` lock). Any of them held by a thread that does not exist in
the child would deadlock `fork`'s child at its first `malloc`, `open` or `printf`.

## Decision drivers

1. POSIX: the child has one thread, a copy of the caller's; its other threads are gone (the specification of `fork`).
2. A forking threaded program followed by `exec` (Python's `subprocess`, `system`, `popen`) must work and must not leak a
   descriptor opened by another thread between the table copy and the exec (DM-001: the `O_CLOEXEC` race).
3. The library's locks in the child are as they were in no thread's hands: free.
4. The cost of the copy is bounded and charged to the child's job before it begins (ADR-0102 item 7, ADR-0261 point 6).
5. After `exec` no thread of the old image runs.

## Considered options

### Option A: Freeze the other threads during the copy; the library takes all locks around it (recommended)

- **Description:** As recommended below.
- **Evidence for:** Drivers 1 to 4; the child's memory is the parent's at one instant with respect to the other threads,
  which is what POSIX describes; reuses the park mechanism that `job_suspend` uses.
- **Evidence against:** A forking thread stops its siblings for the duration of the copy (milliseconds for a 10 MiB image).
- **Cost:** *(estimate)* 80 lines in the kernel, 150 in `libposix` and one C file in the musl objects.
- **Precedent:** Linux holds the memory map's write lock while it copies; glibc and musl take their locks in `fork`
  (`atfork` handlers, `__malloc_atfork`); DM-001 describes the rule.

### Option B: No freeze; the library locks only (what `fork` is on a system with copy-on-write and no step points)

- **Description:** Other threads run between steps of the copy.
- **Evidence for:** Nothing to build in the kernel.
- **Evidence against:** The child's copy is not one instant: a page copied before a sibling's store and a page copied after it
  can disagree, in data not under any lock the library holds. Programs are allowed to call only async-signal-safe
  functions in such a child, but their own structures that they read before `exec` could be torn.
- **Cost:** *(estimate)* none.
- **Precedent:** none that is correct.

### Option C: `fork` is refused with `EAGAIN` when the process has more than one thread

- **Description:** Only `posix_spawn` and `vfork`-then-`exec` patterns work, `vfork` being `fork` (ADR-0100 item 6).
- **Evidence for:** No semantics to define (the Fuchsia route: `fdio_spawn`-style launching only).
- **Evidence against:** Python's `subprocess`, `system()` and `popen()` fork from a threaded process; ADR-0003's whole
  case for `fork` emulation is that real programs fork.
- **Cost:** *(estimate)* 5 lines; breaks 9c.
- **Precedent:** Zircon has no `fork` at all `[outside evidence: Zircon documentation]`.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The `fork` wrapper** (ADR-0102, "The `fork()` wrapper") runs, in this order, before `share` and `clone` of the
   connections: musl's `pthread_atfork` prepare handlers in reverse registration order (`__fork_handler(-1)`); the library's
   locks, in the fixed order `__thread_list_lock`, the stdio list lock, the library lock, `MEM_LOCK`; then the connection
   work, `spawn_prepare` and the move list. A `C` file, `ports/musl/zoraya/atfork.c`, compiled into the musl objects,
   gives Rust `__zoraya_fork_prepare()`, `__zoraya_fork_parent()` and `__zoraya_fork_child()` with access to musl's hidden
   symbols; they do what musl's own `fork.c` does around `_Fork()` and nothing else.
2. **Freeze.** `snapshot_process` sets `snapshot_freeze` on the caller's process for the call. Every other thread of the
   process parks at its next return to EL0 (the `park_if_stopped` mechanism of ADR-0105 item 8, with its own flag), and a
   thread blocked in the kernel parks when it returns. The flag is cleared on success and on every failure path, and when the
   caller is killed. Nothing waits for siblings to park: they are not running on the one CPU while the caller runs
   (a sibling that is mid-call at a step point parks when its call ends). The freeze is the whole of the copy, so the
   child's memory is the parent's at the instant the call began, for everything but the in-kernel copy-out of a sibling's
   blocked call that finishes after the freeze (its buffer is that sibling's own).
3. **Charge before copy.** The child's job pays the copy of *every* mapping, other threads' stacks included, and the call is
   refused before copying if it does not fit (ADR-0102 item 7, ADR-0261 point 6): a 32-thread process forks a child that
   costs 32 stacks until the child frees them (item 5).
4. **The child's kernel state.** One thread, started with the caller's registers (ADR-0102 item 5), its `TPIDR_EL0`
   copied, counted as one thread of the child's job (ADR-0307); no other thread, and no thread handle, since thread objects
   are not duplicable (ADR-0300 item 2). Nothing else about threads is inherited by the kernel.
5. **The child's library state.** `__zoraya_fork_child()`, then `libposix`'s child steps (ADR-0102): musl's
   `libc.threads_minus_1 = 0`, the thread list reduced to the caller's `struct pthread` (`next = prev = self`),
   `__thread_list_lock` and the other musl locks reset to 0, the child's tid kept (the caller's), `libc.need_locks` reset;
   the library lock and `MEM_LOCK` set free; the thread table reduced to the caller, with a new `thread_self()` handle
   and a new port (the old handles are absent or stale: ports are not duplicable, ADR-0102 item 3); the children, the
   process-pending set and the thread-pending sets emptied; `pthread_atfork` child handlers run. **Then the child unmaps
   the stack and TLS mapping of every thread that is not the caller** (the table knows `map_base` and `map_size`; a joinable
   zombie's too), returning their frames to the child's job: they can never run. The unmap is in steps (ADR-0270).
6. **After the call, in the parent:** the locks are released in the reverse order, the `atfork` parent handlers run, and the
   siblings are woken by the kernel when the freeze ends.
7. **`exec`** (ADR-0104 item 6) holds the library lock from `spawn_prepare` to the exit of the old image, so no sibling can
   open, close or change the descriptor table between the table copy for the new image and its start. After
   `exec_commit` the calling thread calls `process_exit` (ADR-0300 item 6): every other thread of the old image leaves at its
   next return to EL0 and no instruction of the old image runs after the new image's pid is handed over, except the exiting
   thread's own last few. Before `exec_commit`, an error from `spawn` or the broker returns to the caller with all threads
   running, and releases the lock. `FD_CLOEXEC` and the namespace table behave as ADR-0107 says.
8. **`posix_spawn`, `system`, `popen`** use `spawn` (ADR-0103), take the library lock for the descriptor snapshot and
   release it before waiting for the child; they do not freeze anything.
9. **`exit`, `_Exit`, `abort` and a return from `main`** end the process: musl's `exit` runs `atexit` handlers and stdio
   flush on the calling thread while the others keep running (as on Linux), then `exit_group`, the two-phase end of ADR-0300
   item 6. A thread's `pthread_exit` of the last thread calls `exit(0)` in musl.
10. **`vfork`** is `fork` (ADR-0100 item 6) and does not suspend the parent thread; the siblings are frozen for the copy only.

**Tensions with accepted ADRs.** *ADR-0102 item 2* ("the copy runs with interrupts masked") was changed by ADR-0270 to steps;
the freeze is what makes the stepped copy one instant. *ADR-0102 item 5* ("The child's one thread") is now the whole
rule. *ADR-0104 item 6* gains the lock.

## Consequences

- **Positive.** A threaded program can `fork` and `exec`; the usual deadlock of DM-001 cannot happen in the library's own
  locks; no descriptor leaks to an unrelated exec.
- **Negative.** The copy stops the siblings; a fork child briefly costs every stack of its parent; one more C file.
- **Follow-up.** The freeze flag in `Process`; the thread table reduction; a test that a fork child's `malloc` works while
  another thread held the heap lock.

## Invariants

- **INV-0306-1.** A `fork` child of a process in which another thread holds `MEM_LOCK`, the library lock or a stdio lock can
  `malloc`, `open`, `printf` and `exec` without waiting. *Check:* an in-QEMU `libposix` test in which a sibling holds each
  lock in turn in a loop while the main thread forks 200 times (the sibling parks at a random point).
- **INV-0306-2.** The child's memory shows a store of a sibling either wholly before or wholly after the fork, never between two
  pages: a sibling increments two counters on different pages in a loop; every child reads them equal. *Check:* an in-QEMU
  test, 500 forks, under HVF and TCG.
- **INV-0306-3.** After a fork the child has exactly one thread and holds no thread handle of the parent, and its usage returns to
  the single-thread figure after item 5's unmap. *Check:* an in-QEMU test with `job_get_usage` of the child's job.
- **INV-0306-4.** After `exec_commit` no thread of the old image completes a library call that changes a descriptor. *Check:* an
  in-QEMU test with a sibling opening files in a loop during 100 `exec`s; the new image's descriptor table has only the
  inherited ones.
- **INV-0306-5.** A failed `snapshot_process` leaves no thread parked: the siblings run again. *Check:* an in-QEMU test with a
  refused charge, then the siblings' counter advancing.

## Open questions

- Whether the freeze should skip threads blocked in a futex wait (they cannot write): an optimization.
- The child's unmap of dead stacks costs a call per thread; a `snapshot_process` flag that omits them would save the copy
  (a new kernel input: deferred).

## Revisit triggers

- A measured stop time that hurts (a large server forking); then the omit-flag.
- SMP: the freeze must wait for siblings running on other CPUs.

## Architect's decision

(Left empty for the human.)
