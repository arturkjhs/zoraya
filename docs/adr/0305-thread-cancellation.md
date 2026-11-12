# ADR-0305: Thread cancellation: deferred at the library's wait points, woken by `thread_interrupt`

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0004, ADR-0100, ADR-0106, ADR-0180, ADR-0300, ADR-0301, ADR-0302, ADR-0304
- **Evidence:** CP-10, DM-059, DM-017

## Context

`pthread_cancel(t)` asks a thread to end at its next cancellation point; the thread's cancel handlers and the
thread-specific-data destructors then run (`pthread_exit(PTHREAD_CANCELED)`). DM-059 records the problems: asynchronous
cancellation is unusable (only three functions are safe to cancel), deferred cancellation depends on a long list of
cancellation points, and cleanup is hard to write. The research's recommendation for Zoraya is that the native ABI defines
how blocking waits are interrupted and that the POSIX layer still emulates `pthread_cancel` for ported C code (DM-059
"Relevance to our OS"; CP-10: "blocking waits are cancelled explicitly").

musl implements cancellation with a signal: `pthread_cancel` stores `t->cancel = 1` and sends `SIGCANCEL` (signal 33)
with `tkill`; the handler, running asynchronously, checks whether the thread's program counter is inside the window of
`__syscall_cp_asm` (`__cp_begin` to `__cp_end`) and, if so, redirects it to `__cp_cancel`; every syscall that is a
cancellation point goes through `__syscall_cp`, which is that assembly. Zoraya has no asynchronous handler (CP-10,
INV-0004-1) and the shim's `__syscall_cp` is a plain `__syscall` with "no cancellation points" (ADR-0100 item 5, written for
one thread). ADR-0180 item 9 made `pthread_setcancelstate` a no-op that changes nothing.

What `thread_interrupt` gives (ADR-0300 item 5): a one-shot flag that makes the target's next or current `futex_wait` or
`sleep_until` return early with `Ok` and the value 1, with nothing running on the target and no `Interrupted` status. A thread
in `port_wait` is woken by a user packet queued to its own port (ADR-0006), which the library sends with the interrupt.

## Decision drivers

1. No asynchronous interruption of user code (CP-10).
2. POSIX's list of cancellation points must be honoured for the functions the library exports: `read`, `write`, `open`
   (not `close`'s race), `waitpid`, `pselect`, `poll`, `nanosleep`, `sleep`, `sem_wait`, `pthread_cond_wait`,
   `pthread_join`, `accept`, `connect`, `recv`, `send`, `fsync`, `sigwait`, `sigsuspend`, `pause`, `pthread_testcancel`.
3. A cancelled thread must run its cleanup handlers and destructors (musl's `__pthread_exit`) and must not die holding the
   library lock (ADR-0302 item 8) or with a half-built Rust frame.
4. musl's retained code (`pthread_cond_wait`, `pthread_join`, `sem_wait`) must cancel correctly with as few patches as
   possible.

## Considered options

### Option A: Deferred cancellation by flag plus `thread_interrupt`; async mode is deferred (recommended)

- **Description:** As recommended below.
- **Evidence for:** Drivers 1 to 4; no async upcall; musl's cancellation logic (`__cancel`, `__testcancel`,
  `pthread_exit`) is kept.
- **Evidence against:** A thread in a compute loop with no library call is not cancelled (the same limit as ADR-0106's
  handlers); `PTHREAD_CANCEL_ASYNCHRONOUS` is accepted but acts as deferred, a stated deviation (POSIX leaves the time
  unspecified, so it conforms).
- **Cost:** *(estimate)* a 60-line patch to `pthread_cancel.c`, 80 lines of Rust at the wrappers.
- **Precedent:** musl's own deferred path; DM-059's alternatives list a cancel token.

### Option B: Asynchronous cancellation by a kernel upcall (musl's SIGCANCEL model)

- **Description:** The kernel runs a handler on the target thread, as Linux does.
- **Evidence for:** Exact musl behaviour, including cancellation in a compute loop.
- **Evidence against:** Rejected by CP-10 and ADR-0004 (DM-017); it needs the asynchronous delivery that does not exist.
- **Cost:** *(estimate)* the hardest kernel primitive in ADR-0004's words.
- **Precedent:** Linux, glibc, musl.

### Option C: No cancellation: `pthread_cancel` returns `ENOSYS`; `pthread_exit` and the cleanup stack still work

- **Description:** Programs that cancel get an error.
- **Evidence for:** Nothing to build; many programs never cancel (DM-059 calls the feature a mistake).
- **Evidence against:** Programs that call it for shutdown (some thread pools, old libraries) would fail or hang; POSIX
  has no `ENOSYS` for it.
- **Cost:** *(estimate)* 10 lines; a conformance gap the pthread subset would show.
- **Precedent:** Some small libcs `[outside evidence]`.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **`pthread_cancel(t)`** (a patch to musl's `pthread_cancel.c`, `0003-deferred-cancel.patch`): `a_store(&t->cancel, 1)`; if
   `t` is the caller and cancellation is enabled and asynchronous, `__cancel()` at once, else nothing; if `t` is another
   thread, `zoraya_posix_tkill(t->tid, SIGCANCEL)`, which the library does not treat as a signal: it
   sets `t`'s "cancel pending" bit, calls `thread_interrupt` on `t`'s handle and queues a user packet to `t`'s port. The signal handler setup (`init_cancellation`,
   `cancel_handler`) is deleted by the patch. An ended thread returns `ESRCH` as now.
2. **Cancellation points are the wrappers.** Every `ffi` wrapper that is a POSIX cancellation point does, **outside the
   library lock**, before its work: `__testcancel()` (musl's: if `cancel` is set and the thread's state is enabled it
   calls `__cancel`, which calls `pthread_exit(PTHREAD_CANCELED)`); and after each long wait (ADR-0302 item 9) that
   returned early on the interrupt (a user packet on the thread's port, or the value 1 of ADR-0300 item 5), the same
   test. A wait that returns early with no handler to run and no cancel is resumed (ADR-0304 item 11 case (c)). The retained musl functions reach their futex waits through `__timedwait_cp`, which uses `__syscall_cp`; the
   shim defines `__syscall_cp(...)` as musl's `__syscall_cp_c` with the assembly window replaced by a plain call to the
   routed function: it tests the thread's `cancel` flag before (if enabled), and after an early return of a futex wait it follows
   ADR-0304 item 11: with `cancel` set and the state enabled, deferred, it calls `__cancel`; with `cancel` set under
   `PTHREAD_CANCEL_MASKED` it returns `-ECANCELED`, which is what musl's `pthread_cond_wait` expects from
   `__syscall_cp_c` (a `-EINTR` there makes musl re-wait with the one-shot flag spent and the cancel is lost); it never
   turns a cancel into `-EINTR`.
3. **`pthread_setcancelstate`, `pthread_setcanceltype`, `pthread_testcancel`, `pthread_cleanup_push` and `_pop`** are musl's
   (the stubs of ADR-0180 item 9 are deleted). `PTHREAD_CANCEL_ASYNCHRONOUS` is accepted and sets `cancelasync`, which
   `__cancel` honours at the next cancellation point and which the entry poll (ADR-0106 item 5) also tests, so an
   asynchronous thread is cancelled at its next library call of any kind. The library does not claim more.
4. **No Rust frame owns anything across a cancellation point.** `__cancel` does not return, and Rust's frames above it are
   not unwound (the crates build with `panic = abort`). So a cancellation point is called only at the top of an `ffi`
   wrapper and after its waits, where the wrapper holds no guard, no `Vec`, no lock and no `Pending` spec; a wrapper that
   must hold something calls the test first and again after dropping it.
5. **A cancelled thread** runs musl's `__pthread_exit`: cleanup handlers in order, thread-specific-data destructors, the
   robust list, the join state; then `thread_exit` (ADR-0300 item 3). Its result is `PTHREAD_CANCELED`.
6. **`pthread_join` and `pthread_cond_wait`** are cancellation points by musl's code; a cancelled `pthread_cond_wait`
   re-takes its mutex before the cleanup handlers run, as POSIX requires and musl does.
7. **`close`** is not a cancellation point here (POSIX allows either; musl's `SYS_close` exception).

**Tensions with accepted ADRs.** *ADR-0180 item 9* is replaced (ADR-0308). *ADR-0106 item 6* ("`EINTR` only where asked") holds:
a cancel interrupt is resumed or ends the thread, and is never an `EINTR` to the program.

## Consequences

- **Positive.** `pthread_cancel` works for the programs that use it for shutdown; musl's cleanup stack is the real one.
- **Negative.** A compute-bound thread is not cancellable; `PTHREAD_CANCEL_ASYNCHRONOUS` is weaker than Linux's;
  a musl patch.
- **Follow-up.** The list of wrappers that are cancellation points is a table checked by a test; the patch in
  `ports/musl/zoraya/`.

## Invariants

- **INV-0305-1.** A thread blocked in each of `read` (empty pipe), `futex_wait` (`pthread_cond_wait`), `nanosleep`, `waitpid`,
  `pselect` and `sem_wait` is cancelled by `pthread_cancel`, runs its cleanup handler once, and joins with
  `PTHREAD_CANCELED`. *Check:* an in-QEMU `libposix` test per wait, and the cancel tests of the conformance subset
  (ADR-0309).
- **INV-0305-2.** A cancelled thread never dies holding the library lock or `MEM_LOCK`: after 1000 cancel rounds against threads
  looping on `open` and `close`, a main-thread `open` still completes. *Check:* an in-QEMU stress.
- **INV-0305-3.** `__testcancel` and `__cancel` are called from `ffi` wrappers only at statement level outside `with`. *Check:*
  `tools/check-cancel-points.sh`, a grep over `crates/zoraya-posix/src/ffi*` that fails on a call inside the closure given to `with`.
- **INV-0305-4.** No signal handler is ever run for `SIGCANCEL`, and signal numbers 32 to 34 are refused by `sigaction`. *Check:* a
  `libposix` test.
- **INV-0305-5.** A cancel under `PTHREAD_CANCEL_MASKED` is never lost at a futex wait: a thread in `pthread_cond_wait` with
  masked cancellation, cancelled while blocked, gets `-ECANCELED` from the shim (never `-EINTR`) and `pthread_cond_wait`
  returns with the cancel visible to the next cancellation point. *Check:* an in-QEMU `libposix` test of 1000 rounds,
  cancel before and during the wait.

## Open questions

- Whether `close` should be a cancellation point for `fsync`-like service waits.
- A cancel token for native programs (CP-10) is not built: the POSIX layer only.

## Revisit triggers

- A program that needs cancellation in a compute loop (async cancel): ADR-0004 Option D.

## Architect's decision

(Left empty for the human.)
