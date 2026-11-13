# ADR-0315: `pthread_*` second slice as built: the `Pending` split, the thread table, signals per thread, deferred cancellation

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0100, ADR-0106, ADR-0300, ADR-0301, ADR-0302, ADR-0304, ADR-0305, ADR-0314
- **Evidence:** CP-18, CP-10, DM-004, DM-058

## Context

ADR-0314 left the long waits inside the library lock and had no thread table, so a thread blocked in `read` stopped the
library calls of every other thread, and no signal or cancel could reach a thread. ADR-0302 item 9 and ADR-0304 and
ADR-0305 say what to build. This ADR records how the code does it, where it refines their text, and what is still missing, so
that no choice is silent. Measured under HVF on the development host: `make test-posix-threads` with the cases `waits`,
`blockedexit`, `signals` and `cancel` (see the report for the run).

## Decision drivers

1. INV-0302-2: a blocked reader must not stop a writer. The method that waits holds `&mut Posix`; the lock must be free during
   the wait and the method must not be resumed with a stale view (ADR-0314 rejected releasing inside `RtSys::wait`).
2. The 8,800 lines of host-tested logic stay as they are: a one-thread process pays nothing but the lock's compare-and-swap.
3. ADR-0304's per-thread mask and pending set are used by code written for one (`Signals`); changing every call is a
   regression risk.

## Considered options

### Option A: Re-run the method (the `Pending` protocol), signal state swapped per entry
- **Description:** As built, below.
- **Evidence for:** Drivers 1 to 3; the methods already loop "try, wait, try again" on level-triggered signals, so a second run from the start is
  correct by construction, and the host tests run the same loop (`tests/files.rs`, `deferred`).
- **Evidence against:** A method must have changed nothing it cannot repeat before its wait (a write keeps its progress in the wait); a new
  wait site that breaks that rule is a bug the host tests find only if they exercise it.
- **Cost:** *(estimate)* 250 lines and tests.
- **Precedent:** ADR-0302 item 9 as written; Linux's `restart_syscall`.

### Option B: A continuation object per blocking call
- **Description:** Each method becomes a state machine (`Poll`-like) that the boundary resumes.
- **Evidence for:** No re-run.
- **Evidence against:** Rewrites every wait site of 8,800 lines of tested code.
- **Cost:** *(estimate)* 1,500 lines.
- **Precedent:** async Rust.

### Option C: Release the lock inside `Sys::wait`
- **Description:** Refused in ADR-0314 (aliasing of `&mut Posix`).
- **Evidence against:** as there.
- **Cost:** 10 lines and an unsound `&mut`.
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The protocol** (refines ADR-0302 item 9). `Posix::defer_waits` is set by the first `__clone`. From then `wait_for` does not call the
   kernel: it stores a `PendingWait { sources, own_len, deadline, progress }` and returns `Err(errno::PENDING)`, a value that is not an errno
   (`0x7fff_0001`). Every method passes it up with `?`. The boundary's `state()` (now `FnMut`) runs the method under the lock with `run_deferred`;
   on `PENDING` it takes the wait out, releases the lock, makes the wait on a **port of its own, created for the wait** (one `port_create` and
   `handle_close` per long wait, none for a short one; no per-thread port to track), and runs the method again with a `Resume { sources, own_len, result,
   progress }` which the next `wait_for` of that run returns as if it had made the wait. The resume is cleared after the run, so a method that does
   not reach its wait again (the data came meanwhile) leaves nothing stale. `write_stream` starts at `resume.progress`. A wait is "long" if it
   goes through `wait_for`; the waits for a service's reply (`Sys::wait` in `call`) stay inside the lock as ADR-0302 item 9 allows.
   The audit of waits (ADR-0302 open question): `wait_for` is called from `read_stream`, `write_stream`, `sock` connect/accept/read/write (net.rs),
   `wait_child`, the terminal read, `sigsuspend`/`pause`, `nanosleep` and `pselect`; all of them return through `state()`.
2. **The thread table** is `Posix::threads` (`threads.rs`): tp (thread pointer, the key), tid, the thread handle, mask, thread-pending set, the port the thread
   waits on, an `interrupted` flag. `Signals.blocked` and `Signals.pending` hold the *calling* thread's; `with()` calls `switch_to(tp)` on every entry, which saves
   the previous thread's pair and loads this one's. So the code of ADR-0106 is unchanged. The first entry registers the main thread (tid 1,
   `thread_self`). `__clone` adds the record **before** `thread_create` (the new thread may run first), fills in the handle after, and
   the thread's exit path (`zoraya_posix_thread_exit`, `__unmapself`) removes the record and closes the handle before `thread_exit`
   (refines ADR-0314 item 3, which closed it at `__clone`). A new thread starts with its creator's mask (musl then sets its saved one).
3. **Signals.** `rt_sigprocmask` is the thread's `Signals::mask` (size must be 8; unblocking polls). `tkill` records a thread-directed signal in the
   target's pending set (not for `SIG_IGN`) and interrupts it; the caller itself takes the `raise` route. A process-directed request that the taker blocks is
   `route`d (ADR-0304 item 3 (b)): to the thread of lowest tid that has it unblocked and waits (has a port), else the lowest with it unblocked; if none,
   `Signals.shared` (the process-pending set) which every thread's `next_unblocked` and `should_poll` read. The interrupt of a thread blocked in a library wait
   is a `thread_interrupt` **plus** a packet with key `u64::MAX` queued on the port the thread registered (the kernel's `port_wait` is not woken by
   an interrupt, ADR-0301); the wake is `EINTR` from `wait_for`, which `restartable` turns into a delivery and, if no handler ran, a second run (ADR-0304 item 11).
   musl's `__block_all_sigs` and `__block_app_sigs` (which were no-ops, leaving `pthread_create`'s `sig_mask` uninitialised) store the mask in force in
   their argument and block nothing (a handler runs only at a library entry or wait, never inside musl's critical sections); `__restore_sigs` is a no-op. The
   main thread is first seen before `__init_tls` set its thread pointer (zero): the table adopts the only thread under its new pointer.
   A thread registers its port and checks its `interrupted` flag and deliverable signals under the lock before it waits, so an interrupt is never lost between the
   check and the wait.
4. **The futex shim** (ADR-0304 item 11): `futex_wait` returning interrupted clears the thread's flag, runs delivery, and returns `-EINTR` if a handler ran, else 0
   (spurious). musl's `sem_wait` and `pthread_cond_wait` loops therefore retry, as INV-0304-7/8 want.
5. **Cancellation** (refines ADR-0305). `pthread_cancel` stores the flag and calls `zoraya_posix_interrupt(tid)` (an interrupt with no signal). `__syscall_cp`
   is `__zoraya_cp_before`/`__zoraya_cp_after` around the call (only musl's futex waits use it): before a call, a pending enabled cancel is acted on (masked mode
   gives `-ECANCELED`); after a call that returned `-EINTR` or 0, likewise. A library wait that is woken by another thread (`EINTR` from the user packet) calls
   `__testcancel` with the lock free. `read`, `write`, `nanosleep` etc. do **not** test at entry: a thread that has a cancel pending and enters `read` with data
   available completes the read (POSIX allows this); it is cancelled at its next wait. Asynchronous cancellation is deferred (no signal can stop a running thread, CP-10).
6. **Not in this slice** (kept as known limits; see the report): `kill(getpid(), n)` from a thread that blocks `n` leaves the signal pending in that thread
   rather than routing it; `sigwait`/`sigtimedwait`; `pthread_getattr_np` and thread names; the `check-cancel-points` script; the fork freeze, child reduction, `exec`
   from a thread, `atfork.c`; `waitpid` and `accept` in a thread are covered by the host retry model and not by an in-QEMU case, because `posixrun` has no broker (a
   `posix_spawn` there hangs): they belong in `tests/posix/t_posix.c`'s full session.
   `pthread_cancel` reads `t->tid` of its target as musl's own `pthread_kill` does under `killlock`; a cancel racing the target's join is the caller's error as in musl.

## Consequences

- **Positive.** A reader thread and a writer thread of one pipe work; `exit()` flushes stdio while a sibling is blocked; `pthread_kill` and `pthread_cancel` wake
  blocked threads; the single-thread path is unchanged except one compare per entry.
- **Negative.** A long wait in a threaded process costs a port create and close; a spurious wake of one library call is possible after an interrupt that was
  only a re-route.
- **Follow-up.** Item 6.

## Invariants

- **INV-0315-1.** A method that asks for a deferred wait and is run again with the outcome returns what it would have returned had it waited in place; a write
  resumed after a wait writes every byte exactly once. *Check:* `cargo test -p zoraya-posix --test files` (`deferred`).
- **INV-0315-2.** Each thread has its own mask and thread-pending set; a process-directed signal a thread blocks goes to exactly one thread that does not, or stays
  process-pending until the first thread to unblock takes it, once. *Check:* `cargo test -p zoraya-posix --test threads`.
- **INV-0315-3.** `pthread_kill` of a thread blocked in `read`, `nanosleep` or `sem_wait` wakes it; the handler runs on that thread; `read` and `nanosleep` return
  `EINTR`, `sem_wait` goes on. *Check:* `make test-posix-threads` (case `signals`).
- **INV-0315-4.** `pthread_cancel` of a thread blocked in `read`, `nanosleep`, `sem_wait` or `pthread_cond_wait`, or looping on `pthread_testcancel`, runs the cleanup
  handler once and joins with `PTHREAD_CANCELED`. *Check:* `make test-posix-threads` (case `cancel`).

## Open questions

- Whether the port per wait should be cached per thread once the thread table carries it (a measurement of a read-heavy threaded program decides).
- Whether `read` and the other cancellation points should test at entry (ADR-0305 item 4 says the wrapper polls before taking the lock); the check script comes with that.

## Revisit triggers

- A threaded program where the port per wait shows in a profile.
- A wait site added to `process.rs` or `net.rs` that does work with side effects before `wait_for`.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
