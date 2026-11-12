# ADR-0301: Waiting on an address: `futex_wait` and `futex_wake`, without requeue, private to a process

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0006, ADR-0023, ADR-0032, ADR-0051, ADR-0141, ADR-0260, ADR-0261, ADR-0264, ADR-0270, ADR-0300
- **Evidence:** CP-11, CP-18, DM-120, GR-011, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

Every mutex, condition variable, read-write lock, barrier, semaphore, `pthread_once` and thread join of musl's retained
`src/thread` waits through `__wait`, `__timedwait` and `__wake`, which call `futex(addr, op, val, ...)`. Today
`ports/musl/zoraya/syscall.h` answers `futex` with `-ENOSYS` at compile time, because a process has one thread
(ADR-0141 item 2). With ADR-0300 a second thread can exist, so the shim needs a real route and the kernel needs a way to
put a thread to sleep until a word in the process's memory changes and to wake it.

What the kernel has: the port (`port_wait`) is a wait on kernel objects' signals and user packets, with a deadline; it
cannot wait on "this word is still 1". Using it for locks means a kernel packet per unlock and a port per waiter, the
cost the futex design was made to avoid (GR-011). A port also cannot be made to give back a lost wake: the wake would have
to be a packet sent to the right waiter's port, and the waiter does not exist as an object the waker can name.

What the research says: a futex is a 32-bit word in user memory; the kernel puts a thread to sleep "if the word still has
this value" and wakes waiters on the address (GR-011). The Linux call grew into one multiplexed syscall whose requeue
operation "proved to be broken and unusable" and was the cause of CVE-2014-3153 (DM-120), and GR-011's own lesson for this
project is to keep wait and wake as separate small operations. Zircon has separate `zx_futex_wait`, `zx_futex_wake` and
`zx_futex_requeue`, and Starnix lists futex semantics among its hard parts
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). What is measured about musl's use: it needs wait, wake and
the requeue of one waiter in `pthread_cond_timedwait` (`unlock_requeue`); it probes `FUTEX_PRIVATE` and falls back to
the shared form when the first returns `ENOSYS`; and it uses priority-inheritance operations only for mutexes with
`PTHREAD_PRIO_INHERIT`.

## Decision drivers

1. Small typed calls, not one multiplexer (GR-011, DM-120, CP-11).
2. No lost wake: the check of the word and the sleeping are one step with respect to a waker (CP-18, "documented lock
   scopes").
3. A wait must not fail for lack of memory after the check (a waiter's node is made at thread creation and charged then,
   ADR-0300 item 2).
4. Every blocking call takes a `Deadline` (ADR-0006 rule 2); the lint of INV-0006-2 is amended for it (ADR-0308).
5. Bounded work with interrupts masked (ADR-0270).
6. Private to a process: no cross-process wake (ADR-0141 item 2 and ADR-0005 exclude shared mappings).

## Considered options

### Option A: Linux's `futex` call, one entry point with op codes

- **Description:** `futex(addr, op, val, timeout, addr2, val3)` with wait, wake, requeue, compare-requeue, wake-op, PI.
- **Evidence for:** musl would need no shim beyond a number; every libc and runtime already speaks it.
- **Evidence against:** The multiplexer is the attack surface DM-120 and GR-011 describe; it takes a relative timeout,
  against ADR-0006's absolute `Deadline`; the PI operations need an owner protocol the scheduler has no use for (no
  priorities, ADR-0030).
- **Cost:** *(estimate)* 700 lines with PI and requeue.
- **Precedent:** Linux.

### Option B: Two typed calls on an address, wait and wake (this ADR), with musl's requeue answered by a wake

- **Description:** `futex_wait(address, expected, deadline)` and `futex_wake(address, count)`; a shim in `libposix` maps
  musl's `FUTEX_WAIT`, `FUTEX_WAKE`, `FUTEX_REQUEUE` (as a wake of the count it asked to requeue) and answers `ENOSYS`
  to the PI operations.
- **Evidence for:** Drivers 1 to 6. A woken waiter that finds the lock taken sleeps again, which every futex user already
  does because spurious wakeups are part of the contract (GR-011 point 4); so a wake in place of a requeue is correct and
  costs a contended re-wait, which only matters for a broadcast to many waiters.
- **Evidence against:** `pthread_cond_broadcast` with many waiters wakes them all at the mutex's contention point instead of
  moving them; a measured cost is unknown.
- **Cost:** *(estimate)* 250 lines in the kernel, 60 in the shim.
- **Precedent:** Windows `WaitOnAddress` and `WakeByAddress*` are the same two operations, documented for threads of one
  process only (GR-011).

### Option C: Kernel mutex and condition objects with handles (NT's events and mutexes)

- **Description:** `mutex_create`, `mutex_lock`, ... as objects waited on through ports.
- **Evidence for:** No word in user memory to get wrong; lifetime is the handle's.
- **Evidence against:** A system call on every lock and unlock, the cost futexes were made to remove (GR-011); a handle slot
  per mutex, of 1024; musl's own algorithms would be replaced.
- **Cost:** *(estimate)* 600 lines and a rewrite of `src/thread`.
- **Precedent:** Windows NT objects, System V semaphores.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`futex_wait(address: Address, expected: Word32, deadline: Deadline)`** (call 60). `address` must be 4-aligned and in a
   readable mapping of the caller's space (`BadAddress` otherwise). **Interrupts stay masked from the read of the word to
   the moment the thread is `BLOCKED`**: this is the contract of `block_until` (as `Process::wait` relies on it), because
   `sched::wake` does nothing to a thread that is not yet `BLOCKED` (`kernel/src/sched.rs`), so a wake that came between
   the check and the block, with interrupts open, would be lost; on the one CPU (ADR-0030) no waker runs in that
   interval. Within it, with the process's futex lock held, the kernel reads the 32-bit word with one aligned load; if it
   differs from `expected` the call returns `WouldBlock` at once; otherwise, **in this order**: (a) a finite deadline
   takes its entry in the deadline queue, and a full queue returns `NoMemory` here, with the node not yet on any list,
   so nothing has to be undone (if the implementation arms the deadline inside `block_until` after the enqueue, it must
   remove the node under the same masked interval on `NoMemory`: no path leaves a node on the wait list after a failed
   call); (b) the thread's own node goes on the process's wait list, keyed by the address, in arrival order; (c) the lock
   is released as part of the block and the thread blocks. It returns `Ok` with the value register 0 when woken,
   `DeadlineExceeded` at the deadline, `Ok` with the value register 1 after `thread_interrupt` (ADR-0300 item 5: a
   spurious wake the caller must re-check; there is no `Interrupted` status) and `Cancelled` when the process is ending
   (ADR-0300 item 6). Any return takes the node and the deadline entry off again. A wake and a time-out that race
   return `Ok`: a wake is never dropped, because the waker took the node off the list under the same lock before it
   woke the thread.
2. **`futex_wake(address: Address, count: Count)`** (call 61): wakes up to `count` threads of the caller's process waiting
   on `address`, oldest first, and returns how many (in the value register). `count` is 1 to 128. It names no object and
   needs no right: it acts only inside the caller's own process, so there is nothing to leak. An address that is not mapped
   wakes none.
3. **Rights and scope.** The key is `(process, address)`; there is no wake across processes. A word in a memory object two
   processes map does not connect them. This is the same limit as ADR-0141 item 2 states for the library, and
   `PTHREAD_PROCESS_SHARED` is refused (ADR-0302 item 6).
4. **No requeue, no PI, no wake-op, no wait-multiple.** `futex_wake` is the whole of the kernel's side. The shim's mapping
   of `FUTEX_REQUEUE(l, 0, 1, r)` is `futex_wake(l, 1)`: the one waiter wakes, finds the mutex taken by the signalling
   thread, and waits on the mutex word, which is what the requeue would have done in one step less. `FUTEX_LOCK_PI` and
   `FUTEX_UNLOCK_PI` return `-ENOSYS`; musl then returns `ENOTSUP` from `pthread_mutexattr_setprotocol` and from a
   `PTHREAD_PRIO_INHERIT` mutex.
5. **Time.** `deadline` is absolute on `clock_monotonic`; the shim turns musl's relative timeout into `now + timeout`,
   and `pthread_cond_timedwait`'s absolute `CLOCK_REALTIME` or `CLOCK_MONOTONIC` time (a `__timedwait` argument) into the
   same. A finite deadline uses one entry of the kernel's deadline queue while the thread sleeps. **The queue's size is a
   derived constant**, `QUEUE_SIZE >= MAX_THREADS_SYSTEM + KERNEL_TIMERS + RESERVED` (`kernel/src/timer.rs`, with a
   `const` assertion): every thread in the system, services and the trusted jobs outside the session included, may
   hold one entry, and only `MAX_THREADS_SYSTEM` (1000) bounds those (ADR-0307's per-job figures bound only the session
   and the untrusted rooms), plus the kernel's own timers and the two reserved entries (ADR-0264, ADR-0285). The
   implementation sets it to that sum (1024 if the kernel's timers and the reserved entries are 24 or fewer, which the
   step measures; today's 288 is the one-thread-per-process figure). A full queue still returns `NoMemory`, which
   the shim returns as `-EAGAIN`. **The queue's operations are O(log n):** `DeadlineQueue` (`zoraya-time`) is a sorted
   array whose insert and cancel are a linear scan and a rotate (`lib.rs`), which at 1024 entries under a masked
   spinlock is about 16 times ADR-0270's step, and `on_interrupt` pops each due entry with a rotate of up to 1024.
   Instead of counting those units in INV-0270-1, this ADR decides on a binary min-heap now: each waiter's deadline
   entry holds its own heap index, so insert, cancel and pop are at most 10 moves at 1024 entries, and `on_interrupt`
   pops at most 64 due entries per pass, re-arming the timer for an immediate second pass if more are due. An entry is
   about 40 bytes (a time, a thread reference and the heap index), so the static queue is about 40 KiB, not 16.
6. **Mapping and the wait list.** The address space's mapping calls take a per-process mapping lock (a kernel mutex whose
   waiters sleep, not a spinlock) for the whole of a call, including its steps, because `map_reserve` and `map_commit`
   assume nothing maps between them (ADR-0270, `memory/user.rs`); a second thread's mapping call sleeps until the first
   ends. The futex lock is a separate spinlock, taken only for the read-and-enqueue, so a long mapping call never delays a
   wake. The mapping lock is also held by `snapshot_process` for the whole of its call (ADR-0306 item 2) and by the
   unmap in `thread_exit` (ADR-0300 item 3), and is released by an RAII guard on every path (ADR-0300 item 6).
7. **Bounded work.** Each thread owns its wait node, made at `thread_create` and charged there, so `futex_wait` allocates
   nothing and cannot fail for memory after its check. `futex_wake` takes at most 64 nodes off the list per step, drops the
   lock and passes a preemption point between steps (ADR-0270 item 2); at most 128 threads wait in a process, so a wake is
   two steps.
8. **The word is read through the user-copy path** (`UserSlice`, with the fault resume of ADR-0036), so a fault in the read
   returns `BadAddress` and never panics.

**Tensions with accepted ADRs.** *ADR-0141 item 2* ("`futex` is answered with `-ENOSYS` in the header") is replaced by the
shim of this item; ADR-0302 states it. *ADR-0006 rule 2* (every blocking call takes a `Deadline`) holds; *ADR-0006 INV-0006-2's* lint (parameters drawn from
`{ Handle, Port, Deadline, &mut [Packet], flags }`, exactly two blocking calls) does not: `futex_wait` takes an `Address`
and a `Word32` and is a third blocking call, so the lint's set and count are superseded by ADR-0308 (INV-0308-5).
*ADR-0006 rule 5 and INV-0006-6* are untouched: this ADR adds no `Status`. *ADR-0260's* table row "Kernel deadlines: 288
in one queue" becomes the derived size of item 5. *ADR-0031:* the mapping lock of item 6.

## Consequences

- **Positive.** Two small calls; no lost wake; no allocation after the check; musl's algorithms run unchanged.
- **Negative.** Broadcast cost without requeue; a bigger deadline queue (about 40 bytes an entry, so about 40 KiB static), now a heap; a new kernel lock for mapping calls.
- **Follow-up.** `zoraya-rt` stubs; the shim; `Word32` and `Count` argument types in `abi`; the lints that every blocking
  call takes a `Deadline`.

## Invariants

- **INV-0301-1.** A wake between a waiter's check and its sleep is never lost: with one thread storing a value and waking, and
  one waiting for it, the waiter returns in every one of 100,000 rounds, under HVF and TCG. *Check:* an in-QEMU test.
- **INV-0301-2.** `futex_wait` returns `WouldBlock` and does not sleep when the word differs from `expected`, `BadAddress` for an
  unmapped or unaligned address, and never blocks a thread whose process is ending. *Check:* an in-QEMU test of each,
  and a host test of the wait list model in `zoraya-thread`.
- **INV-0301-3.** `futex_wait` and `futex_wake` allocate nothing: the heap statistics are equal before and after 10,000 rounds, and
  the job's usage does not change. *Check:* an in-QEMU test with the test kernel's heap and usage calls.
- **INV-0301-4.** No futex or thread-ending path keeps interrupts masked longer than ADR-0270's bound with 128 waiters. *Check:*
  `t_bound` extended with three cases and the masked-time counter (INV-0270-1): 128 threads waiting on one address and
  one `futex_wake(128)`; a `job_kill` of one process of 128 threads; and a `job_kill` of a job of **4 processes x 128
  threads** (and the `job_resume` of the same after a `job_suspend`), which must show one wake per process under `TREE`
  (ADR-0300 item 6).
- **INV-0301-5.** Every call in the `abi` table that blocks takes a `Deadline`, and the two futex calls are in the table with their
  argument types. *Check:* INV-0006-2's lint over `abi` **as amended by ADR-0308 (INV-0308-5)**: the lint as it stands
  (`crates/zoraya-abi/src/tests.rs`) rejects `futex_wait`'s `Address` and `Word32` and expects two blocking calls, so it
  does not cover the futex calls until it is amended; the amended lint expects three.
- **INV-0301-6.** A timed `futex_wait` never returns `DeadlineExceeded` before its deadline. *Check:* an in-QEMU test with 64 threads
  and staggered deadlines, comparing `clock_monotonic` at return.
- **INV-0301-7.** The deadline queue holds `MAX_THREADS_SYSTEM` timed waiters plus the kernel's timers and the reserved
  entries at once, never returns `NoMemory` to a `futex_wait` below that, and no insert, cancel or pop moves more than 12
  entries. *Check:* a `const` assertion on `QUEUE_SIZE`, `cargo test -p zoraya-time` (the move count over 1024 random
  inserts, cancels and expiries) and an in-QEMU test with 1000 threads each in a timed `futex_wait`.

## Open questions

- The cost of a heap insert into the queue with interrupts masked, measured under TCG (the heap is decided; this is the
  number that confirms the 64-pop pass).
- Whether a broadcast to 100 waiters needs a requeue after all: measure with the stress test of ADR-0309.
- `futex_wait` on several words (GR-011's lesson on multi-wait): not needed by musl; deferred.

## Revisit triggers

- A measured broadcast cost that matters, or a program that needs a wait across processes (a shared-memory lock): a new
  call with a handle to a memory object as its key.
- SMP: the futex lock becomes a real lock and the word read an atomic.

## Architect's decision

(Left empty for the human.)
