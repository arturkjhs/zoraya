# ADR-0314: `pthread_*` as built in the first slice of part 9b step 2: what is in, what waits for signals and cancellation

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0100, ADR-0141, ADR-0300, ADR-0301, ADR-0302, ADR-0303, ADR-0304, ADR-0305, ADR-0308, ADR-0313
- **Evidence:** CP-18, DM-058

## Context

The library half of part 9b (ADR-0302, ADR-0303, ADR-0308, accepted 2026-11-13) was begun in slices. The first slice is
musl's `src/thread` over the shim with the library lock, the memory lock, `__clone`, `__unmapself` and the futex route:
create, join, detach, mutexes of all types, condition variables with time-outs, `pthread_once`, keys, `_Thread_local`,
rwlocks, barriers, spin locks and semaphores, tested by `tests/posix-core/t_threads.c` (`make test-posix-threads`).
Signals per thread, deferred cancellation of a blocked thread, `fork` with siblings, `exec` and `exit` from a non-main
thread, and the split of the long waits are the next slices. This ADR records where the code differs from the accepted text
or fixes a choice the text left open, so that no choice is silent (CLAUDE.md "Decisions"). Each item names the sentence it
refines. Measured under HVF on the development host: the eight cases of `t_threads` pass (`make test-posix-threads`).

## Decision drivers

1. Reuse musl's algorithms unchanged (ADR-0302 driver 1); a program with no thread pays one compare-and-swap per library call
   (ADR-0302 driver 5).
2. A thread is never stranded with a handle, a mapping slot or a memory object it cannot give back (ADR-0300 item 3, INV-0300-7).
3. Every behaviour that this slice does not have is refused or documented, not faked (CP-18; ADR-0304 and ADR-0305 are not met
   yet).

## Considered options

### Option A: The long waits stay inside the library lock in this slice; `Pending` comes with the signals slice
- **Description:** `with()` takes the library lock and holds it across `Sys::wait` as it held the cell before. A thread blocked
  in `read` on an empty pipe, in `waitpid`, `pselect` or a socket call keeps every other thread out of the library calls
  that need `Posix` state (not out of `pthread_*`, which use no library state: musl's futex code reaches the kernel through the shim).
- **Evidence for:** Driver 1: no change to the 8,800 lines of host-tested logic in the same commit as the thread core; the
  thread algorithms can be tested alone first. The cases of `t_threads` do not block in a library call.
- **Evidence against:** INV-0302-2 is not met: a reader thread and a writer thread of one pipe deadlock. ADR-0302 option D says this.
- **Cost:** *(estimate)* none now; the split is the 250 lines of ADR-0302's negative consequences, later.
- **Precedent:** Linux's BKL on its way out (DM-058).

### Option B: Release the library lock inside `RtSys::wait` and `wait_any`
- **Description:** The `Sys` implementation unlocks around the kernel wait and locks again.
- **Evidence for:** Two lines; no `Pending` plumbing.
- **Evidence against:** The method that called `wait` holds `&mut Posix` across the release, and another thread may change the
  descriptor table under it: aliasing and logic errors that the host tests (one thread) cannot see.
- **Cost:** *(estimate)* 10 lines and an unsound `&mut`.
- **Precedent:** none we trust.

### Option C: The `Pending` split first, then the thread core
- **Description:** ADR-0302 item 9 as written, before any thread runs.
- **Evidence for:** INV-0302-2 from the first commit.
- **Evidence against:** The split cannot be tested with real threads until the core exists; a failure in either hides the other.
- **Cost:** *(estimate)* the same total, one larger commit.
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option A for this slice, then C's split in the next, with B refused.** The items below are the
code of the first slice.

1. **Two locks** (ADR-0302 item 8). `LIB` guards `Posix`; `MEM` guards the heap and the mapping table; the order is `LIB` then
   `MEM`. Both are `zoraya_posix::lock::Lock`, Drepper's three-state futex mutex over a `Wait` trait, host-tested. A second entry
   to `with()` by the same thread (its thread pointer is the owner) is the old panic "entered twice", not a deadlock. The table of
   private anonymous mappings grows from 64 to 256 entries, one for each of the kernel's mappings of a process (ADR-0300), so
   that 128 stacks fit.
2. **The waits stay inside `LIB`** (Option A, above). Until the split, `t_threads` and every thread test must not rely on a
   library call that waits while another thread calls the library. INV-0302-2 stays `not yet applicable` with this reason.
3. **No thread table yet** (refines ADR-0302 items 3 and 4). `__clone` makes the thread, stores the new id (2, 3, ...; the main
   thread is 1; never reused; `EAGAIN` at 2^30) at `ptid` before the thread starts, and **closes the thread's handle at once**:
   nothing in this slice uses it, so no slot can outlive the thread. The address the kernel clears at the end of a thread
   (`thread_exit`'s `clear`) is the `ctid` musl gives `__clone`, and `set_tid_address` gives for the main thread; it is
   `&__thread_list_lock` for every thread, so the library keeps the last value given in one atomic instead of one per thread.
   A detached thread's last act, `__unmapself`, gives its memory object's handle back and clears its table entry before
   `thread_exit(clear, base, size)` unmaps the stack. The thread table, the thread's own port and its signal state come with
   ADR-0304, which needs handles for `thread_interrupt`.
4. **Signals and cancellation are not there** (refines ADR-0304 and ADR-0305, which are met by the next slice).
   `SYS_rt_sigprocmask` does nothing (reads give an empty old mask), `SYS_tkill` returns `ENOSYS` (so `pthread_kill` returns
   `ENOSYS`), `pthread_sigmask` is the process-wide Rust function, and `__syscall_cp` is `__syscall`. `ports/musl/zoraya/cancel.c`
   replaces musl's `pthread_cancel.c` and `syscall_cp.c`: `pthread_cancel` sets the flag, `__testcancel` (called by
   `pthread_testcancel`, `pthread_join`, `pthread_cond_wait` through musl's code) acts on it for the calling thread, and a thread
   blocked in a futex is not woken by a cancel. An early return of `futex_wait` on `thread_interrupt` is a spurious wake (0)
   because nothing interrupts yet.
5. **`FUTEX_LOCK_PI` and every other futex op are `-ENOTSUP`, not `-ENOSYS`** (corrects ADR-0302 item 6's remark): musl's
   `pthread_mutexattr_setprotocol(PTHREAD_PRIO_INHERIT)` returns the error of that call, so `ENOTSUP` is what INV-0302-4 asks.
   `FUTEX_REQUEUE` and `FUTEX_CMP_REQUEUE` wake the wake count plus the requeue count (ADR-0301 item 4); `INT_MAX` is a loop
   of wakes of 128. The two `pshared` patches of ADR-0302 item 6 are `ports/musl/zoraya/0002-no-pshared.patch` (`ENOTSUP` for
   mutex, condition, rwlock and barrier attributes, and for `sem_init`).
6. **The tier list** (ADR-0308 item 2, which asks the first step to write it). `docs/adr/posix-surface-T9.txt` lists the 127 names
   of musl's `src/thread` and `<semaphore.h>` that the archive defines, minus those ADR-0308 excludes. The list leaves out
   `pthread_getattr_np`, `pthread_getname_np` and `pthread_setname_np`, which ADR-0308 names but which musl implements with
   `/proc` and `prctl`: their files are dropped (`SHIM_REPLACED` in `ports/musl/musl.mk`) and the three join the list when they
   are written (Rust, for the main thread's stack: ADR-0302 open question). `sem_open` and `sem_unlink` are dropped. `posix-surface-check`
   reads a `.txt` entry of its spec as one more list, and `pthread_create` is off its deny list (`DENIED`, INV-0005-2).
7. **Files of the retained directory that this port does not compile** (`ports/musl/musl.mk`, `SHIM_REPLACED`): the ones that Rust
   (`__clone`, `__unmapself`, `pthread_sigmask`) or `ports/musl/zoraya/cancel.c` replace, and the ones just named. They compile
   on the shim, so they are not in `excluded.txt`, whose rule is that its files fail to compile. `synccall.c` (signals every
   thread with `tkill`, reads `gettid`) is excluded there, as the shim refuses it.
8. **Mappings.** `mprotect` stays `ENOSYS` (musl tolerates it); a thread's stack is writable below its guard area, and the page
   the kernel leaves unmapped under every mapping is the guard (ADR-0302 item 7). INV-0302-6 is not tested in this slice.

## Consequences

- **Positive.** The thread library runs and passes eight cases under HVF; the kernel's calls 56 to 61 are used by real code;
  a single-threaded program's cost is the two atomics.
- **Negative.** A thread blocked in a library wait stops the others' library calls (item 2): a known limit until the next slice.
  No signal can reach a thread, and `pthread_cancel` does not wake a blocked thread (item 4).
- **Follow-up.** The `Pending` split, the thread table, per-thread signal state and `thread_interrupt` (ADR-0304, ADR-0305), the
  fork freeze and the reduction of the child (ADR-0306), the libc-test subset, `threadtest3` and `OpenSSL` (ADR-0309).

## Invariants

- **INV-0314-1.** A detached thread gives back its stack mapping and its memory object's handle: 300 detached threads, 500 joined
  ones and 320 more in batches of 16 leave the mapping table (256 entries) usable; a leak would end in `EAGAIN` for ever.
  *Check:* `make test-posix-threads` (case `cycles`).
- **INV-0314-2.** The names the archive exports are the tier lists' and the T9 list's: `pthread_create` is exported, off the deny
  list, and every name of the T9 list is defined. *Check:* `make check-ports` (`posix-surface-check check`) and `cargo test -p posix-surface-check`.

## Open questions

- Whether `Pending` should be one error value of every method (ADR-0302 item 9) or only of those that wait; the audit of waits
  decides, in the next slice.
- Whether `CLEAR_ON_EXIT` should be per thread when a program calls `set_tid_address` itself (no musl code does).
- The measured cost of `LIB`'s compare-and-swap on the bench of `t_libposix` is not taken yet (ADR-0302 revisit trigger).

## Revisit triggers

- A musl update that gives `__clone` different `ctid` values per thread.
- INV-0302-2's test failing on the first two-thread program with a blocked reader.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
