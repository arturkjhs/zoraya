# ADR-0302: `pthread_*` in `libposix`: musl's own thread code over a shim, one library lock, long waits outside it

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0002, ADR-0005, ADR-0006, ADR-0100, ADR-0101, ADR-0102, ADR-0103, ADR-0104, ADR-0106, ADR-0141, ADR-0180, ADR-0300, ADR-0301, ADR-0303, ADR-0304, ADR-0305, ADR-0306, ADR-0307, ADR-0308
- **Evidence:** CP-18, DM-058, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

ADR-0100 item 5 made a POSIX process single-threaded: musl's `src/thread` directory is replaced by Rust (only
`__set_thread_area.s` and `__init_tls.c` are kept), `sem_*` are counters, `pthread_setcancelstate` and the cleanup
macros' functions are stubs (ADR-0180 item 9), and `crates/zoraya-posix/src/ffi.rs` keeps its state in one
`UnsafeCell<Option<Posix>>` guarded by a flag that panics on a second entry ("zoraya-posix entered twice"). The same file
keeps the heap (`ProcessHeap`), the mapping table (`MAPPINGS`) and the `__zoraya_calls` counters in statics with no lock.
Blocking sites are `Sys::wait` and `Sys::wait_any` (`net.rs`, `process.rs`, `jobs.rs`, `ffi.rs`), `nanosleep`, and
the retries of `read` and `write` on streams.

Stage 9b adds kernel threads (ADR-0300) and a wait on an address (ADR-0301). What remains is the library: where
`pthread_create`, mutexes, condition variables, `pthread_once`, join and thread-specific data come from, and how the
library's own state survives several threads.

What musl's `src/thread` is: a complete, small (about 3,200 lines of C and assembly in musl 1.2.5, counted), long-used implementation whose only contact with the
kernel is `__syscall(SYS_futex ...)`, `SYS_exit`, `SYS_set_tid_address`, `SYS_rt_sigprocmask`, `SYS_tkill`,
`SYS_set_robust_list`, `SYS_sched_*`, `SYS_mprotect` (its failure with `ENOSYS` is tolerated, `pthread_create.c`), plus
`__clone`, `__unmapself` and `__syscall_cp_asm` in assembly. ADR-0100 item 2's shim already routes `SYS_*` names to
`zoraya_posix_*` functions and fails the build on a name it does not route, so a retained file cannot reach Linux
(INV-0100-2). The stdio, locale and malloc-adjacent code that ADR-0100 retains already uses `struct pthread` (`errno`, the
`FILE` lock owner's tid, the locale pointer), so the layout of that structure is in the library whatever is chosen.

## Decision drivers

1. Reuse code that is already correct for threads: musl's mutex, cond, rwlock, barrier, once, tsd and join algorithms
   (CP-18: documented lock scopes; the algorithms are documented in musl's source and by the futex papers).
2. INV-0100-2 stays: no `svc` and no Linux system call outside `zoraya-rt`.
3. `libposix` state that is shared (descriptor table, namespace table, signal state, children, the heap) must be protected,
   and **no thread may block for long while holding the lock that protects it**: a thread in `read` on an empty pipe must
   not stop another from writing to it.
4. The change to `zoraya-posix` should be small enough to review: 8,783 lines of host-tested logic exist.
5. A program not using threads pays almost nothing: tcc's 5,294 header opens (ADR-0002 driver 6).

## Considered options

### Option A: musl's `src/thread` retained over a shim; one library lock; long waits outside it

- **Description:** As recommended below.
- **Evidence for:** Drivers 1, 2, 4 and 5: the new Rust is the shim (about 400 lines), the lock and the split of the
  waits; the algorithms are musl's. The same route ADR-0141 took for the rest of musl.
- **Evidence against:** Two small patches to musl are needed (ADR-0305 for cancellation, `pshared` below), and the musl
  update path (ADR-0141 revisit trigger) must re-check them.
- **Cost:** *(estimate)* 500 lines Rust and C new, one patch of 60 lines, 3 files excluded.
- **Precedent:** musl on every platform; Starnix does not do this but emulates `clone` and `futex` the same way
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option B: `pthread_*` written in Rust in `zoraya-posix`, host-tested

- **Description:** Mutex, condvar and the rest are Rust types over `futex_wait`/`futex_wake`; the `struct pthread` fields
  that retained musl code reads are kept by Rust.
- **Evidence for:** Host tests of the algorithms (INV-0100-3 spirit); no musl patch.
- **Evidence against:** About 3,200 lines of musl's algorithms rewritten and a second copy of `struct pthread`'s layout
  to keep in step with the retained stdio and locale code; ADR-0100 item 2's reason for retaining musl is the opposite.
- **Cost:** *(estimate)* 3,500 lines and the bugs of a new condvar.
- **Precedent:** Rust's `std` has its own futex-based locks, over a libc that has its own.

### Option C: Emulate Linux's `clone`, `futex` and `tkill` in the kernel (a Starnix-style personality call set)

- **Description:** The kernel gets Linux-shaped calls for `libposix`'s sake only.
- **Evidence for:** musl's code runs with no shim.
- **Evidence against:** ADR-0002 rule 3: kernel facilities for the personality are `#[personality_only]` and numbers are
  not identities; ADR-0300 option A's reasons; Starnix itself names futexes and signals as its hard parts.
- **Cost:** *(estimate)* 900 lines in the kernel and rules.
- **Precedent:** Starnix.

### Option D: One library lock held across every wait (a big lock, as Linux's BKL was)

- **Description:** The shared state is under one lock which a thread holds for the whole of each library call, including
  its waits.
- **Evidence for:** It is the cheapest way to make the existing code safe; DM-058 notes that a big lock is legitimate
  when the protected paths are short.
- **Evidence against:** The paths here are not short: `read` on a pipe, `waitpid`, `pselect` and `nanosleep` wait for as
  long as the program lets them, and a program with a reader thread and a writer thread deadlocks at its first byte.
- **Cost:** *(estimate)* 100 lines; unusable.
- **Precedent:** Linux's BKL (DM-058); seL4's kernel big lock works because its calls are short (DM-058's alternatives).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **What is retained from `src/thread`.** All of it except the files below (found by compiling, as ADR-0141 item 1
   does, and listed in `ports/musl/excluded.txt` with the reason): `__set_thread_area.c` (the `.s` stays), `synccall.c`
   and `membarrier.c` (signals and `tkill`, used only by the set-uid family and `dlopen`), `pthread_getattr_np.c`
   (reads `/proc/self/maps`; replaced by Rust: the main thread's stack from the startup block, a thread's from its
   `struct pthread`), `pthread_setschedparam.c`, `pthread_getschedparam.c`, `pthread_setschedprio.c` (replaced: they
   return `ENOTSUP`; Zoraya has no priorities, ADR-0030), `__timedwait.c`'s `FUTEX_CLOCK_REALTIME` form is used as is.
   `sem_*`, `call_once`, `mtx_*`, `cnd_*`, `thrd_*`, `tss_*` and `pthread_atfork` come from the retained files, and the
   Rust stubs for `sem_init`, `sem_post`, `sem_wait`, `pthread_setcancelstate`, `pthread_testcancel` and the cleanup
   functions (`ffi.rs`) are deleted.
2. **The shim** (`ports/musl/zoraya/syscall.h`) gains these routes; each is a `zoraya_posix_*` function that returns a
   value or `-errno`:

   | musl's call | Route |
   |---|---|
   | `SYS_futex` | `zoraya_posix_futex(uaddr, op, val, timeout, uaddr2, val3)`: `WAIT` and `WAKE` with or without `PRIVATE` are `futex_wait` and `futex_wake` (a relative timeout becomes `now + timeout`); `REQUEUE` is a wake of its requeue count; everything else `-ENOSYS` (ADR-0301 item 4); a `futex_wait` that returns early with the interrupt value is not passed to musl as `-EINTR`: ADR-0304 item 11 gives the three outcomes |
   | `SYS_exit` | `zoraya_posix_thread_exit(code)`: closes the thread's own handle and port (item 4), then ends the calling thread with `thread_exit(clear, 0, 0)`, `clear` being the `ctid` its `__clone` was given |
   | `SYS_rt_sigprocmask` | the calling thread's mask (ADR-0304) |
   | `SYS_tkill` | `zoraya_posix_tkill(tid, sig)`: a thread-directed signal (ADR-0304 item 2) |
   | `SYS_gettid` | the calling thread's tid |
   | `SYS_sched_yield` | `thread_yield` |
   | `SYS_set_robust_list`, `SYS_get_robust_list`, `SYS_sched_setscheduler`, `SYS_sched_getparam`, `SYS_sched_setparam`, `SYS_sched_getscheduler`, `SYS_mprotect` | `-ENOSYS` (musl tolerates it for the robust list, for `mprotect` and for the scheduling calls reports `ENOSYS`) |

   `SYS_set_tid_address` keeps its route but the main thread's tid is now **1**, not the pid; new tids are 2, 3, ...
   (a per-process counter in the library, never reused, refused with `EAGAIN` at 2^30).
3. **`__clone` and `__unmapself`** are replaced by two small files in `ports/musl/zoraya/` (`clone.c` with an
   assembly trampoline, `unmapself.c`). `__clone(fn, stack, flags, arg, ptid, tls, ctid)` ignores `flags` (musl always
   passes the same set), stores the new tid at `*ptid` before the thread starts, records `ctid` in the library's thread
   table, calls `thread_create(trampoline, stack, {fn, arg}, tls)` and returns the tid. The trampoline sets nothing
   (the kernel gave `TPIDR_EL0`) and calls `fn(arg)`; `fn` is musl's `start`, which never returns. `__unmapself(base,
   size)` closes the thread's own handle and port, then calls `thread_exit(ctid, base, size)`. The musl file `src/thread/aarch64/clone.s` and `__unmapself.s` are
   excluded.
4. **The library's thread table** is a `Vec` in `Posix`, one record per live thread: tid, the thread handle (from
   `thread_create`, or `thread_self` for the main thread, called once at start-up and after a `fork`), the thread's own
   port (created at its start, closed at its end), its signal mask and thread-pending set (ADR-0304), its `ctid`, and
   a wait flag. A thread finds its record through `__pthread_self()->tid`. Two handle slots per thread, so with 128 threads
   256 of the 1024 slots. **The thread closes both itself**, in its exit path (`zoraya_posix_thread_exit` and
   `__unmapself`, which take the library lock briefly and remove the record) before it calls `thread_exit`, whether it
   is joinable or detached; joining waits on the `ctid` word, not on the handle (ADR-0300 item 3). So no slot outlives
   its thread, and 1024 slots are never used up by detached threads (INV-0300-7). The main thread's handle and port are
   closed by the process's end.
5. **One port per thread.** `Sys::wait` and `wait_any` use `self.port`, a single port per process; with threads two
   waiters on one port take each other's packets. The port moves into the thread's record, and a thread subscribes the
   same handles on its own port for each wait, as it does now.
6. **Refused attributes.** `PTHREAD_PROCESS_SHARED` for a mutex, condvar, rwlock, barrier or semaphore returns `ENOTSUP`
   (a patch to the four `*attr_setpshared.c` files and `sem_init`'s `pshared` test, `ports/musl/zoraya/0002-no-pshared.patch`),
   because ADR-0301 item 3 has no wake across processes; `PTHREAD_PRIO_INHERIT` and `PTHREAD_PRIO_PROTECT` return `ENOTSUP`
   (musl's own result when `FUTEX_LOCK_PI` is `ENOSYS`); `pthread_attr_setschedpolicy`, `setscope` and `inheritsched` accept
   only the default; a robust mutex works for non-shared mutexes as musl implements it in user space (`pthread_create.c`
   walks the list at exit).
7. **Stacks.** `pthread_create` maps its stack and TLS block with `mmap`, which `libposix` serves from a memory object
   (ADR-0100 item 3, ADR-0141 item 2). musl's default stack is 128 KiB and guard 8 KiB (`DEFAULT_STACK_SIZE`), kept. `mprotect`
   stays `ENOSYS`, so the guard region of musl's mapping is writable, but the kernel's gap above every mapping
   (`find_gap`) is an unmapped page below the next mapping, so an overflow past the bottom of a stack faults
   (INV-0030-2's spirit, tested by INV-0302-6). `pthread_attr_setguardsize` is accepted and ignored. A stack of less than
   `PTHREAD_STACK_MIN` (musl: 2048 plus TLS) is refused as musl does. Each stack is one of the process's 256 mappings.
8. **The library lock.** A new type `Lock` (`zoraya-posix/src/lock.rs`, `#![forbid(unsafe_code)]`, Drepper's three-state
   futex mutex over a `Wait` trait so it is host-tested with a model) is taken by `with()` in place of the "entered twice"
   panic. It guards `Posix`. A second lock, `MEM_LOCK`, guards the heap, the mapping table and the `__zoraya_calls`
   counters; the order is library lock first, memory lock second, never the other way, and the heap never calls the library.
   Both are unlocked in the one-thread case at the cost of one compare-and-swap (INV-0302-5). `errno` is musl's thread-local.
9. **Long waits happen outside the lock.** A wait that can last as long as the program lets it (`read` or `write` on a
   stream, a terminal, a socket or a pipe; `accept`, `connect`, `recv`, `send`; `waitpid`, `pselect`, `poll`, `sigsuspend`,
   `sigwait`, `pause`, `nanosleep`, `sleep`, `sem_wait`) is never made inside a `Posix` method that holds the lock.
   The method that would wait returns a new error value `Pending(WaitSpec { sources, deadline })` having changed nothing it
   cannot repeat; the `ffi` wrapper releases the lock, waits on the thread's port, and calls the method again. For
   `write` of several bytes a partial write is kept and the wrapper resumes at the offset. Waits for a service's
   reply (a request to `fsd`, `netd`, the broker or the terminal: bounded by the service answering) stay inside the
   lock, as they are short; a service never needs this process's other threads to answer. The one exception in
   start-up (`ffi.rs` `sys.wait(slot::BOOTSTRAP ...)`) runs before a second thread exists.
10. **Interrupt and cancellation points** are the same places as the waits above (ADR-0304, ADR-0305): the wrapper
    polls its thread's flags before it takes the lock and after each wait, never while holding it.

**Tensions with accepted ADRs.** *ADR-0100 item 5* ("No `pthread_*` is exported", `sem_*` no-ops) and *ADR-0141 item 2*
(`futex` answered `-ENOSYS`) are superseded by ADR-0308 once accepted. *ADR-0100 item 2* (the replaced list of directories, which names `src/thread`) gains
`src/thread` as retained, minus the files in item 1; *ADR-0141* (item 2's `futex` answer and its revisit trigger) is
replaced by item 2's table. *ADR-0180 item 9* (`pthread_setcancelstate` changes nothing) is
replaced by musl's. *ADR-0005 INV-0005-2* names `pthread_create` on the deny list "unless an accepted ADR adds it": ADR-0308
does.

## Consequences

- **Positive.** The thread library is the one musl's users run; the Rust change is a lock, a table and a split of
  eight waits.
- **Negative.** The split of waits touches `process.rs`, `net.rs`, `jobs.rs` and `fd.rs` (about 250 lines) and their host
  tests; two musl patches; every library call takes two atomic operations.
- **Follow-up.** The `Pending` protocol and its host model (a fake kernel with two threads); the tier list for the new
  symbols (ADR-0308); `docs/design.md`.

## Invariants

- **INV-0302-1.** `libc.a` contains no `svc` outside `zoraya-rt`, and every `SYS_*` the retained `src/thread` files use is a row of
  the shim's table. *Check:* INV-0100-2's scan of the archive and the build failing on an unrouted name.
- **INV-0302-2.** No thread blocks for longer than a service round trip while holding the library lock: with one thread blocked in
  `read` on an empty pipe, a second thread's `write` to it, `getpid`, `open` and `stat` each complete. *Check:* an in-QEMU
  `libposix` test with that pair, repeated for `waitpid`, `pselect`, `nanosleep`, `accept` and `sem_wait`.
- **INV-0302-3.** The `Lock` never lets two threads in at once and never loses a wake: a host model of two to four threads over
  the `Wait` trait, every interleaving to depth 12. *Check:* `cargo test -p zoraya-posix` (`lock`).
- **INV-0302-4.** `PTHREAD_PROCESS_SHARED`, `PTHREAD_PRIO_INHERIT` and `PTHREAD_PRIO_PROTECT` return `ENOTSUP`. *Check:* the
  conformance subset of ADR-0309 and a `libposix` test.
- **INV-0302-5.** A program that creates no thread makes no `futex_wait` or `futex_wake` call at all: the lock's fast path is an
  atomic exchange. *Check:* the call counters of `__zoraya_calls` over the existing `make test-posix` workloads show zero futex calls.
- **INV-0302-6.** A write below the lowest address of a thread's stack mapping faults the process (the mapping gap). *Check:* an
  in-QEMU test with an expected `Fault` exit.
- **INV-0302-7.** Every `Status` maps to one `errno` per call, with `OverQuota(Threads)` to `EAGAIN` (there is no `Interrupted` status; `EINTR` comes from
  ADR-0304 item 11 only).
  *Check:* INV-0100-4's host test over `Status::ALL`.

## Open questions

- Which waits are "long": the list of item 9 is from reading; the audit in the first implementation step (ADR-0309's plan)
  counts the call sites of `Sys::wait`, `wait_any` and `sleep_until`.
- Whether the library lock can later be split by subsystem: not needed for one CPU.
- `pthread_getattr_np` for the main thread (its stack is the 64 KiB kernel-made stack): Python's `threading` asks.

## Revisit triggers

- A measured cost of the two atomics on a single-threaded program, or contention on the library lock in the stress test.
- A musl update that changes `pthread_create.c`'s use of `mprotect` or the `__clone` arguments.

## Architect's decision

Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.
