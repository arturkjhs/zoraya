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

Evidence: DM-058 records that a lock held across a wait is legitimate only for short paths (here: not); DM-004 that a library cannot own a process-directed
signal, so the routing is by rule; CP-18 that lock scopes are documented, which the retry protocol keeps; CP-10 that no thread is interrupted asynchronously.

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
   Known limitation of that rule: a short `RtSys::wait` inside `call()` still holds the library lock across a service's whole reply, and a few
   replies are not short: `stop_self` (the broker replies only when the process is continued) and the terminal's `TCSADRAIN`/drain reply.
   A sibling thread's library calls wait for the lock meanwhile (the process is stopped as a whole in the first case, so it is harmless there;
   in the second a sibling can wait for the drain). Moving those two to `Pending` waits is follow-up work.
   The audit of waits (ADR-0302 open question): `wait_for` is called from `read_stream`, `write_stream`, `sock` connect/accept/read/write (net.rs),
   `wait_child`, the terminal read, `sigsuspend`/`pause`, `nanosleep` and `pselect`; all of them return through `state()`.
2. **The thread table** is `Posix::threads` (`threads.rs`): tp (thread pointer, the key), tid, the thread handle, mask, thread-pending set, the port the thread
   waits on, an `interrupted` flag. `Signals.blocked` and `Signals.pending` hold the *calling* thread's; `with()` calls `switch_to(tp)` on every entry, which saves
   the previous thread's pair and loads this one's. So the code of ADR-0106 is unchanged. The first entry registers the main thread (tid 1,
   `thread_self`). `__clone` adds the record **before** `thread_create` (the new thread may run first), fills in the handle after, and
   the thread's exit path (`zoraya_posix_thread_exit`, `__unmapself`) removes the record and closes the handle before `thread_exit`
   (refines ADR-0314 item 3, which closed it at `__clone`). A new thread starts with its creator's mask (musl then sets its saved one).
3. **Signals.** `rt_sigprocmask` is the thread's `Signals::mask` (size 8, or 16 as musl passes it, the second word is always empty; unblocking polls). `tkill` records a thread-directed signal in the
   target's pending set (not for `SIG_IGN`) and interrupts it; a `tkill` of the caller itself only marks it pending in the caller (see the musl hooks below). A process-directed request that the taker blocks is
   `route`d (ADR-0304 item 3 (b)): it goes into `Signals.shared` (the process-pending set), which every thread's `next_unblocked` and `should_poll` read, and the
   thread of lowest tid that has it unblocked and waits (has a port), else the lowest with it unblocked, is interrupted to look; the taker's own thread-pending
   bits (an earlier `pthread_kill` of it) are not touched. Whichever thread with the signal unblocked takes it first handles it, once; one that blocks it again or
   ends before looking leaves it for the others. The interrupt of a thread blocked in a library wait
   is a `thread_interrupt` **plus** a packet with key `u64::MAX` queued on the port the thread registered (the kernel's `port_wait` is not woken by
   an interrupt, ADR-0301); the wake is `EINTR` from `wait_for`, which `restartable` turns into a delivery and, if no handler ran, a second run (ADR-0304 item 11).
   musl's `__block_all_sigs` and `__block_app_sigs` store the mask in force in their argument (what `pthread_create` gives the new thread as `sig_mask`) **and
   really block everything but `SIGKILL`/`SIGSTOP`** in the calling thread; `__restore_sigs` puts the mask back and polls. So a handler never runs inside
   musl's `killlock` or thread-list lock (neither from the `tkill` of oneself, which is only marked pending, nor from a delivery at a futex wait inside
   `pthread_exit`), and `pthread_kill(pthread_self(), n)` and `raise` run the handler after the lock is released. (The first version of this slice blocked nothing
   and ran the handler inside the lock: a handler that called `pthread_kill` on itself or `pthread_exit` deadlocked; found in review.) The
   main thread is first seen before `__init_tls` set its thread pointer (zero): the table adopts the only thread under its new pointer.
   A thread registers its port and checks its `interrupted` flag and deliverable signals under the lock before it waits, so an interrupt is never lost between the
   check and the wait.
4. **The futex shim** (ADR-0304 item 11): `futex_wait` returning interrupted clears the thread's flag, runs delivery, and returns `-EINTR` if a handler ran, else 0
   (spurious). The flag is one-shot and the futex wait of a non-cancellation point (a mutex, `__tl_lock`) spends it, so the cancel does not depend on it
   (item 5). musl's `sem_wait` and `pthread_cond_wait` loops therefore retry, as INV-0304-7/8 want.
5. **Cancellation** (refines ADR-0305). `pthread_cancel` stores the flag and calls `zoraya_posix_interrupt(tid)` (an interrupt with no signal). `__syscall_cp`
   is `__zoraya_cp_before`/`__zoraya_cp_after` around the call (only musl's futex waits use it): before a call, a pending enabled cancel is acted on (masked mode
   gives `-ECANCELED`); after a call that returned `-EINTR` or 0, likewise. The cancel does **not** depend on the interrupt flag, which a non-cancellation-point wait
   (`pthread_mutex_lock`, `__tl_lock`, `__lock`) or a wait that ended for data may have spent: musl's own `cancel` flag stays set, and the next cancellation point
   acts on it. The library's cancellation points call `__testcancel` (ADR-0305 item 2) at the **top** of `read`, `write` (and the vector and socket forms),
   `connect`, `recv*`/`send*`, `waitpid`/`wait4`, `nanosleep`/`clock_nanosleep`, `pselect`/`select`, `sigsuspend`/`pause`, outside the lock and before any buffer is built; and
   `wait_unlocked` calls it before and after every long wait, **whatever ended the wait**, with no lock held. So a thread cancelled while it waited for a mutex and then
   calls `read` on an empty pipe is cancelled there. (An earlier text said `read` does not test at entry; with the flag spent that lost the cancel.) Asynchronous
   cancellation is deferred (no signal can stop a running thread, CP-10).
   **Deviation from ADR-0305 item 4** ("no Rust frame owns anything across a cancellation point"): the two tests in `wait_unlocked` run below `state()`, which owns the
   wait's `sources` vector, and below the wrapper's own temporaries (`pselect`'s four fd-set vectors, `connect`'s address copy); with `panic = abort` and no
   unwinding a cancel in the wait leaks them. Bound: a cancel ends the thread, so this happens once per cancelled thread, and the blocks are at most about 32 KiB
   (`pselect` with 1024 descriptors: 16 KiB of sources, 4 x 128 bytes of sets), usually under 300 bytes. A cancel that arrives before the call, or between calls, leaks nothing.
   Restructuring so that the wait returns to the top of the wrapper before the test is possible (a `CANCELLED` marker through `state()`), and is follow-up work if a
   program cancels threads in a loop.
6. **Not in this slice** (kept as known limits; see the report): the `check-cancel-points` script;
   `waitpid` and `accept` in a thread are covered by the host retry model and not by an in-QEMU case, because `posixrun` has no broker (a
   `posix_spawn` there hangs): they belong in `tests/posix/t_posix.c`'s full session.
   (The temporary refusal of `fork` with siblings, `EAGAIN`, was removed by item 9.)
   `pthread_cancel` reads `t->tid` of its target as musl's own `pthread_kill` does under `killlock`; a cancel racing the target's join is the caller's error as in musl.
7. **The exit path** (`exit`, `_exit`, the statistics line of `ZORAYA_POSIX_STATS`, the ADR-0110 report) waits for the library lock for at most 500 ms (a 1 ms sleep between tries)
   instead of trying once, so a sibling's short call does not drop the report; after that the report is skipped and the exit goes on. The statistics line is written with the
   lock free (`restartable`), so a full stderr pipe waits instead of losing the line.
8. **The tests.** The waits in `t_threads` are ordered by an announcement and a real 150 ms sleep (`settle`), because a program cannot see that a sibling is blocked; the
   result of a case does not depend on the order (the interrupt and the cancel flag are sticky), the sleep only makes the interesting order the likely one. Bounded
   loops replace yields where a count is awaited. `overflow` writes a 1 MiB frame's lowest byte below the stack in one jump: it tests the layout (the unmapped gap),
   not a guard page; musl's guard pages are made with `mprotect`, which is `ENOSYS`, so they stay writable and a smaller overrun is not caught.
9. **`fork` and `exec` with threads as built** (implements ADR-0306; ADR-0306's open question on a sibling blocked in the kernel is decided here as "accept").
   `fork()` (`ffi/procs.rs`) calls `__zoraya_fork_prepare` (`ports/musl/zoraya/atfork.c`, musl's `fork.c` minus `_Fork`): the `pthread_atfork` prepare handlers, the
   signal mask (blocked as `__block_app_sigs`), musl's `atfork_locks` (stdio list, atexit, locale, ...), `__inhibit_ptc`, the thread-list lock; then `state()` takes
   the library lock, and **`MEM` is taken only around `snapshot_process`** (after the snapshot buffer is built, because the encoder allocates; the child resets its copy
   with `Lock::reset`, the parent unlocks). So no sibling is in the heap or the mapping table during the copy, and the kernel's `Freeze` stops the rest at EL0. Order:
   musl's locks, thread-list lock, library lock, `MEM` (ADR-0306 item 1 names the thread-list lock first). The order cannot invert, because **nothing takes a musl lock while it holds the library lock or `MEM`**
   (the library's code does not call into musl's locked code under `LIB`). `fopen` does not hold the open-file list lock while it calls the library (it opens first and
   adds the `FILE` to the list after); the real list-lock-then-`LIB` nesting is `fflush(NULL)` and `__stdio_exit`, which hold the list lock and flush each `FILE`, whose
   write enters the library: list lock, then `LIB`, never the reverse. In the child `Posix::after_fork_in_child` reduces the thread table to the caller
   (`threads_after_fork`: a new `thread_self` handle, no wait port, empty process-pending set; host test `a_fork_child_keeps_the_calling_thread_only`); after the library
   lock is released `__zoraya_fork_child` does musl's `__post_Fork` (`next = prev = self`, `__thread_list_lock = 0`, `threads_minus_1 = 0`, `need_locks = -1`), zeroes
   `atfork_locks` and the two words of `__vmlock_lockptr` (as musl's `fork.c` does), **frees every `FILE` lock (stdin, stdout, stderr and the open-file list) held by a thread that is not the caller** (musl leaves them locked; INV-0306-1 asks that
   `printf` works; a lock of -1, "never locked", stays; the buffer of a `FILE` freed this way can hold a half-written record of the dead thread, as the lock is free
   but the data is what the sibling left), unmaps the stack and thread block of every other thread on the list (`map_base`, `map_size`), restores the mask and runs the child handlers.
   `exec` now calls `process_exit(0)` inside `state()`, so the library lock stays held from the descriptor-table copy to the end of the old image (ADR-0306 item 7);
   a sibling's library call blocks on the lock and the kernel ends the sibling at its next return to EL0. `vfork` is `fork`; `posix_spawn` takes only the library lock.
   Known limits: (a) a sibling blocked in a kernel call that completes during the copy may write its result into a page already copied or about to be (a torn write across
   a page boundary of that sibling's own buffer; the buffer is the sibling's, which does not exist in the child); (b) **deviation from accepted ADR-0306 item 5** ("a joinable zombie's too"; needs the architect's acceptance): a joinable thread that has ended and was not joined
   is not on musl's list, so the child does not unmap its stack; it stays mapped, and charged to the child's job, until the child ends (it can never be joined there;
   the parent's zombies are in the child's copy of the table only, and the child's table is reduced to the caller); (c) `abort`'s lock is not taken (`abort` is the library's); (d) `defer_waits` stays on in the child
   (each long wait costs a port, as in a threaded process); (e) INV-0306-3 (usage returns to the single-thread figure) and INV-0306-5 (a refused charge leaves no thread
   parked) have no in-QEMU check yet.
10. **Third slice** (sigwait, attributes, rounds). `sigwait`, `sigtimedwait` and `sigwaitinfo` (`ffi/procs.rs`, `wait_signals`) block the set for the call, take a signal of it from the
   thread-pending or process-pending set, wait with `wait_for` and the thread's port, and give `EAGAIN` at the deadline, `EINTR` when a handler of another signal ran;
   a thread in the wait is a target of routing (`ThreadRec::sigwait`, host test).
   **Signals from other processes (deviation from accepted ADR-0106 item 2, needs the architect's acceptance).** A process with no handler has `SIG_DFL` dispositions in
   the kernel (`Terminate` for the terminating signals, `Ignore` for the rest), so a `sigwait` for a signal sent by another process would end the process (SIGTERM, SIGINT,
   SIGUSR1; INV-0106-4 lets a blocked `SIG_DFL` terminating signal end the process) or never be seen (SIGCHLD, SIGWINCH), and with no handler the wait had no source and slept
   forever. While a thread is in the wait (`ThreadRec::sigwait` non-zero) the kernel disposition of each `SIG_DFL` signal of the set (but the stop and continue signals,
   which are the broker's) is `Deliver` (`Posix::effective_disposition`, set and restored in `wait_signals` with the union over all threads in the wait, so one thread
   ending does not disarm another), and the wait listens to the process's own request signal as a handler would (`interrupt_sources`). Both are undone on every exit of the
   wait (signal, timeout, `EINTR`, cancel). So a process that is in `sigwait` takes `process_take_request` calls and set-disposition calls that INV-0106-2's list does not name:
   **`sigwait`, `sigwaitinfo` and `sigtimedwait` are on that list** here (INV-0315-8). Cancel: the wait runs with the cancel state disabled (`__zoraya_cancel_begin`), looks at
   the flag itself at the top of each round, ends its state (`sigwait` cleared, mask restored, dispositions restored), and only then calls `testcancel`, so a cancelled
   thread leaves no blocked mask and no routing target behind. Known limit: a signal taken from the pending sets clears the thread-pending and the process-pending bit of the
   number together, so two instances (one `pthread_kill`, one `kill`) become one where Linux returns two. The `siginfo_t` of a wait has `SI_USER` and no sender. They are in `posix-surface-T9.txt` with `pthread_getattr_np`, `pthread_getname_np`, `pthread_setname_np`
   (`ports/musl/zoraya/attr.c`; names live in the thread table; the main thread's stack is the kernel's 64 KiB). `open`/`openat` are cancellation points (item 5's list; the
   1000-round test found they were not). The statistics line has `futex=N` (INV-0302-5). `posix-stats --no-thread-calls` checks INV-0308-3 against the call log's union; it
   was written and unit-tested but not run over fresh logs of all the T1 workloads (`make test-posix-long`), so INV-0308-3 stays "not yet applicable".
   Test layout: cases that need `kill(getpid())` or `fork` cannot run under `posixrun` (no broker: `getpid()` is 0 there and `kill` of oneself goes to the broker and hangs),
   so `treroute`, `tsig`, `tsigdfl`, `twait`, `tfork*`, `texec`, `tspawn` are in `tests/posix/t_posix.c`. **Suites (ADR-0309):** `make test-posix-suites` builds and runs 13 thread tests of musl's libc-test
   (pinned in `ports/sources.lock`, `libctest`; `tests/libc-test/threads-excluded.txt` names the 12 left out with reasons: asynchronous cancel, `pthread_cancel-points` (does not link:
   `shm_open` is not exported; its six cancel scenarios pass on a copy without it), robust mutexes (`ENOSYS`), priority inheritance, named semaphores, dynamic loading, tests that
   fork or fill memory). SQLite's `threadtest3` is pinned (`sqlitesrc`) and builds, but every scenario fails at its first write with
   `SQLITE_IOERR_LOCK` because `fcntl` record locks (`F_SETLK`) are `EINVAL`: it is `make posix-threadtest3`, not a step, until record locks have an ADR. The 3850 is not a threads defect and is out of 9b's scope: record locks need an ADR of their own (see the open questions).

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

- **INV-0315-5.** A `fork` child of a process whose siblings are running (in the heap, in `open`/`close`, holding stdout's lock, incrementing counters on three pages, mapping and
  unmapping 64 KiB and 1 MiB) has one thread, can `malloc`, `printf`, `open`, make and join a thread of its own, sees the counters of one instant and any region the sibling had
  published; the `pthread_atfork` handlers run in musl's order; also when the forking thread is not the main thread. *Check:* `make test-posix` with `tests/posix/threads.script`
  (`tfork`, `tforkthread`: 500 forks, every fourth child then execs, HVF and TCG in CI).
- **INV-0315-6.** `exec` from a non-main thread while siblings open files and write to a pipe ends the old image, runs the new one once, leaves it no descriptor above 2 but the
  pipe's, and no write of the old image follows the new image's first. *Check:* the same script (`texec`, 100 execs).
- **INV-0315-7.** `posix_spawn` and `vfork` from four threads at once all complete with the child's status. *Check:* the same script (`tspawn`).
- **INV-0315-8.** `sigwait`, `sigwaitinfo` and `sigtimedwait` take a signal of the set sent by another process while its action is `SIG_DFL` (the process survives, also for a
  default-ignored signal); a cancelled `sigwait` leaves the mask and the wait undone; the kernel dispositions are back when the wait ends. *Check:* `cargo test -p zoraya-posix
  --test threads` (the `sigwait` cases) and `make test-posix` with `tests/posix/threads.script` (`tsigdfl`).

## Open questions

- Whether the port per wait should be cached per thread once the thread table carries it (a measurement of a read-heavy threaded program decides).
- `fcntl` record locks (`F_SETLK`, `F_GETLK`): needed by SQLite's unix VFS; a per-process emulation (success when no other process holds a conflicting lock) would unblock `threadtest3`; a decision for the architect.
- Robust mutexes (`pthread_mutexattr_setrobust`) are `ENOSYS`; whether a thread that ends holding a mutex should be visible to the others is open.
- Not checked yet: INV-0306-3 (usage returns to one thread's figure) and INV-0306-5 (a refused charge leaves no thread parked); INV-0308-3 as above; the mapping-count measurement of a 128-thread process (risk 10) was not made.
- `fcntl` record locks need their own ADR (SQLite's unix VFS; out of 9b's scope).
- ADR-0106 item 2 / INV-0106-2 and ADR-0306 item 5 are deviated from as items 9 and 10 say; the architect accepts the deviations or asks for a superseding ADR.
- The `check-cancel-points` script (INV-0305-3) is not written; the points are the calls named in item 5.
- Whether the wait should hand the cancel back to the top of the wrapper (item 5, the bounded leak).

## Revisit triggers

- A threaded program where the port per wait shows in a profile.
- A wait site added to `process.rs` or `net.rs` that does work with side effects before `wait_for`.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
