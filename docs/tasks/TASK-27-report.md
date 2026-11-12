# TASK-27 report: stage 9, applications

Running report, updated after each part. Part 9b's design was written by a Sonnet session on branch `work/9b-design`
(reading, ADRs and plan only: nothing was built, no test was run; `make adr-check` and `make check` are the only commands run).

## Status for the architect (2026-11-12, `origin/main` at `6390dc6`)

| Part | State | Tag |
|---|---|---|
| 9a TLS for C, zlib, curl, wget | not started | |
| **9b POSIX threads** | **design done** on `work/9b-design` (ADR-0300 to ADR-0309, `proposed`, and the plan below); implementation not started | |
| 9c CPython | not started | |
| 9d NetSurf | not started | |
| image addition (`EXTRA_ROOT`) | not started | |

### The proposed ADRs of part 9b (all `proposed`; one line each is a recommendation, not a decision)

| ADR | Title | Recommendation in one line |
|---|---|---|
| [0300](../adr/0300-threads-in-a-process.md) | Threads in a process | Thread objects named by handles inside the process: `thread_create(entry, stack, argument, tls)`, `thread_exit(clear, unmap)`, `thread_self`, `thread_interrupt`; at most 128 per process; the end of a process is two-phase; new `Status::Interrupted`. |
| [0301](../adr/0301-wait-and-wake-on-an-address.md) | Waiting on an address | Two typed calls `futex_wait(address, expected, deadline)` and `futex_wake(address, count)`, private to a process; no requeue, no priority inheritance (musl's requeue becomes a wake); the deadline queue grows from 288 to 1024; a per-process mapping lock. |
| [0302](../adr/0302-pthreads-in-libposix.md) | `pthread_*` in `libposix` | Keep musl's own `src/thread` over a shim; a library lock plus a memory lock; waits that can last are made outside the lock (`Pending` and retry); one port per thread; `PROCESS_SHARED` and priority inheritance refused with `ENOTSUP`. |
| [0303](../adr/0303-thread-local-storage.md) | Thread-local storage | The thread pointer is an argument of `thread_create` (no set-tls call); musl lays TLS out inside the stack mapping, which the creating job pays for. |
| [0304](../adr/0304-signals-with-threads.md) | Signals with threads | Per-thread masks and pending sets, per-process pending set; `pthread_kill` thread-directed; a process's request is taken by any waiting thread and routed to the lowest-tid thread that has it unblocked; no handler under a library lock. |
| [0305](../adr/0305-thread-cancellation.md) | Cancellation | Deferred only: a flag plus `thread_interrupt`, tested in the wrappers outside the lock; asynchronous mode acts as deferred; one patch to musl's `pthread_cancel.c`. |
| [0306](../adr/0306-fork-exec-and-exit-with-threads.md) | `fork`, `exec`, exit | `fork` takes the locks and freezes the other threads for the copy, the child has one thread and unmaps the others' stacks; `exec` holds the library lock until the old image ends; `exit` is the two-phase end. |
| [0307](../adr/0307-thread-quotas.md) | Thread quotas | A `threads` figure of its own (answers ADR-0260's open question), counting every live thread, derived as 3 x `procs` (room 96, untrusted rooms 288, session 600); old records stay valid; `EAGAIN`. |
| [0308](../adr/0308-posix-threads-enter-the-tier-list.md) | Threads enter the tier list | **The superseding ADR**: replaces ADR-0100 item 5, ADR-0141 item 2's `futex` answer, ADR-0180 item 9 and `pthread_create`'s place on INV-0005-2's deny list; tier T9 with a list file; ADR-0100 itself stays `accepted`. |
| [0309](../adr/0309-how-9b-is-tested.md) | How 9b is tested | musl's libc-test thread tests as the conformance subset (Open POSIX Test Suite rejected), SQLite `threadtest3`, our `t_threads.c` stress program, OpenSSL rebuilt with threads. |

### Items for the architect

1. **Accept ADR-0308 (and ADR-0300 to ADR-0307) or change them.** TASK-27's rule is that a part merges only if none of its
   proposed ADRs changes an accepted decision. ADR-0308 does (ADR-0100 item 5, ADR-0141 item 2, ADR-0180 item 9, a row of
   ADR-0005's exclusion table), and ADR-0301 and ADR-0307 change numbers of ADR-0260/0261 (deadline queue, the limits record).
   So the 9b implementation can be written on its branch, but **it does not merge until you accept at least ADR-0300,
   0301, 0307 and 0308**. The task's done criterion ("ADR-0100's 'no threads' is superseded by an accepted ADR") is met by
   your accepting ADR-0308; agents did not and will not set any status. Partial supersession is written in ADR-0308's text,
   as ADR-0225 did; ADR-0100 is not edited.
2. **Choices made without you that are cheap to reverse now and dear later:** the thread is a kernel object with a
   handle (not a `clone`-style call); futex without requeue and PI; musl's pthread code kept (a patch to
   `pthread_cancel.c` and one for `pshared`); the `threads` quota counts the first thread of a process too and has no
   typed word yet (`--threads` deferred); `fork` freezes the other threads for the copy; `PTHREAD_PROCESS_SHARED` is refused;
   `MAX_THREADS` 128 per process.
3. **The plan needs part 9a for one item only** (OpenSSL rebuilt with threads, step 4b); steps 1 to 3 and the rest of step 4
   do not.
4. Merge conflict note for the orchestrator: this file is new on `work/9b-design`; `work/9-docs` may create the same
   path. Keep the Status block of whichever is newer and this file's "9b" section below.

Manual checks the architect must do for 9b: none. Measurements: none yet (design only). Time and model: one Sonnet session
for the design.

## 9b: POSIX threads

### What exists today (read, not measured), and what the design rests on

- A process has one thread (`Process::thread`, `kernel/src/process.rs`); the only user-thread call is `thread_yield`;
  `Param::EntryPoint` exists in `zoraya-abi` for a `thread_create` that does not exist (INV-0004-1 names it).
- A user thread blocks in two places only: `port_wait` (`kernel/src/port.rs`, whose waiter list already holds several
  threads) and `sleep_until`. `TPIDR_EL0` is saved at each switch and a snapshot child gets the caller's value.
- Every mapping has an unmapped page below the next one (`AddressSpace::find_gap`), so thread stacks get a guard for free;
  there are 256 mappings and 1024 handle slots per process, hence 128 threads at most.
- `map_reserve`/`map_commit` assume one thread in the call (ADR-0270 steps): a mapping lock is needed (ADR-0301 item 6).
- `zoraya-posix` keeps all state in one cell with a panic on re-entry (`ffi.rs` `with`), plus an unlocked heap, mapping
  table and call counters; its waits are `Sys::wait`, `Sys::wait_any`, `nanosleep` and the stream retries.
- musl 1.2.5's `src/thread` (about 3,200 lines) reaches the system only through `__syscall(SYS_futex|exit|tkill|
  rt_sigprocmask|set_robust_list|sched_*|mprotect ...)` plus `__clone`, `__unmapself`, `__syscall_cp_asm`; the existing shim
  already makes an unrouted `SYS_*` a compile error. Read from the unpacked tree in a sibling worktree's build directory.
- Not verified because nothing was built: that the retained files compile as listed, the names in libc-test's tree, the
  cost of a 1024-entry deadline queue insert with interrupts masked, the exact list of long waits.

### Implementation plan: four large steps

Each step goes to a fresh `general-purpose` Sonnet subagent in its own worktree off the branch `work/9b` (made from `main`
after the architect's acceptance, or earlier on the proposed ADRs if the orchestrator wants to start; merge waits for item 1),
`CARGO_BUILD_JOBS=3`, one QEMU test at a time, `make check` before every commit, `make test-changed` under HVF while working.
Kernel code, `unsafe` and the security boundaries (the futex's user read, the two-phase end, the freeze) go to
`opus-review` (CLAUDE.md "Models and loops"). The first step that works in an ADR's area implements its invariants' checks
(ADR-0001 rule 6) and flips the status column of `docs/adr/invariants.md` from `not yet applicable (stage 9)` to
`checked: <command>`. Line counts are estimates.

**Step 1: kernel, ABI and quotas** (about 1,800 lines plus 1,200 of tests; ADR-0300, 0301, 0307; INV-0300-*, 0301-*, 0307-*).
1. `crates/zoraya-thread` (new, `no_std`, `zoraya_test_alloc::cap_memory!()` in lib and tests): the bounded thread list,
   the two-phase end state machine and the FIFO wait list keyed by address with the 64-per-step wake; host tests including
   an exhaustive interleaving model (INV-0300-6, INV-0301-2's model half). Add to `Cargo.toml` workspace and `tools/test-map.txt`.
2. `crates/zoraya-abi/src/lib.rs`: calls 53 `thread_create`, 54 `thread_exit`, 55 `thread_self`, 56 `thread_interrupt`,
   57 `futex_wait`, 58 `futex_wake`; `Status::Interrupted = 17`; argument types `Word32` and `Count`; the `thread`
   object type (signal `TERMINATED`, rights `WAIT | SIGNAL`, not duplicable); `quota.rs` (threads word at bytes 16 to 20,
   `QuotaField::Threads = 3`, usage record 152 bytes, decoders total); `crates/zoraya-charge/src/lib.rs` (`Field::Threads`);
   `crates/zoraya-rt/src/lib.rs` stubs. Tests: the table lints (INV-0300-1, INV-0301-5), decoder tests (INV-0307-2).
3. Kernel: `kernel/src/sched.rs` (thread record gains `interrupted`, the futex node, the process slot; `spawn_user` for
   `thread_create`), `process.rs` (thread list, `ending` flag and owner, park and wake of all threads, `snapshot_freeze` used
   by `snapshot_process`, mapping lock), `syscall.rs` (the six calls; the aligned 32-bit user read through the fault-resume
   path), `port.rs` and `sleep_until` (consume `interrupted`), `object.rs` and `job.rs`/`charge.rs` (the thread type and
   the `threads` charge in the order processes, threads, memory), `timer.rs` (`QUEUE_SIZE` 1024), `memory/user.rs`
   (the lock around `map_reserve` to `map_commit` and unmap), `steps.rs` (units for wake and thread end).
4. Plumbing: `job_set_limits`/`job_get_usage` for the new field; `crates/zoraya-rooms` derives `threads = 3 x procs`; `init`
   sets the session job to 600 and the keeper the untrusted job to 288 (INV-0307-3); `grants` prints the derived figure.
5. In-QEMU tests in `kernel/src/testing.rs` with user programs `user/src/bin/t_thread.rs` (raw ABI, no libposix): INV-0300-2
   to 0300-5, INV-0301-1 to 0301-4 and 0301-6, INV-0307-1 and 0307-4, the freeze (INV-0306-5's kernel half), `t_bound`
   extended with 128 waiters. `tools/test-map.txt` lines for `kernel/src/*`, `crates/zoraya-thread/`.
Exit: `make check`, `make test-changed`, the candidate run on HVF and TCG, `opus-review` with no open error.

**Step 2: the thread core of `libposix`** (about 1,500 lines plus tests; ADR-0302, 0303, 0308; INV-0302-*, 0303-*, 0308-1/2).
1. Audit first (a table in this report): every call site of `Sys::wait`, `wait_any`, `sleep_until`, the stream retries and
   the service reply waits, each marked long or short (ADR-0302 item 9).
2. `crates/zoraya-posix/src/lock.rs` (futex mutex over a `Wait` trait, model-tested, INV-0302-3), `MEM_LOCK` around the
   heap, mapping table and counters, `with()` taking the lock; the thread table and tids (main = 1); per-thread port;
   `Pending` split of the long waits in `process.rs`, `net.rs`, `jobs.rs`, `fd.rs` with wrapper retry loops in
   `ffi.rs` and `ffi/procs.rs`; host tests with a fake kernel of two threads (`crates/zoraya-posix/tests/kernel`).
3. musl side: `ports/musl/musl.mk` (retain `src/thread`), `ports/musl/excluded.txt` (the files of ADR-0302 item 1),
   `ports/musl/zoraya/syscall.h` (the new routes), `clone.c`, `unmapself.c`, `0002-no-pshared.patch`; delete the Rust
   stubs of `sem_*`, `pthread_setcancelstate`, `pthread_testcancel` and the cleanup functions; Rust `pthread_getattr_np`,
   the three scheduling calls (`ENOTSUP`), `sysconf` values (ADR-0308 item 4); the errno table (`Interrupted` to `EINTR`,
   `OverQuota(Threads)` to `EAGAIN`, INV-0302-7).
4. `docs/adr/posix-surface-T9.txt`, `tools/posix-surface-check` (third list, deny list without `pthread_create`),
   `make check-ports`; `make test-libposix` extended.
5. Tests (`tests/posix/t_posix.c` or a new `t_pthread.c` with a script step): create, join, detach, mutex, cond, once,
   keys, TLS and `errno` per thread (INV-0303-1/2/4), the blocked-`read`-and-`write` pair (INV-0302-2), stack guard
   (INV-0302-6), quota `EAGAIN` (INV-0303-3), no futex call without threads (INV-0302-5).
Exit: as step 1; `make test-posix`, `make test-libposix` unchanged results for the single-threaded workloads.

**Step 3: signals, cancellation, `fork`, `exec`, exit** (about 1,000 lines plus tests; ADR-0304, 0305, 0306).
1. `crates/zoraya-posix/src/signals.rs` per-thread mask and pending sets, the routing rule, `sigwait`, `pthread_kill`,
   `tkill` route; host model (INV-0304-1, 0304-5).
2. `ports/musl/zoraya/0003-deferred-cancel.patch`, the `__syscall_cp` definition in the shim, `__testcancel` in the wrappers
   outside the lock, `tools/check-cancel-points.sh` and a `check-cancel-points` entry in the Makefile's `CHECK_STEPS`
   (INV-0305-3), `sigaction` refusing 32 to 34.
3. `ports/musl/zoraya/atfork.c`, the `fork` wrapper changes in `ffi/procs.rs` (locks, freeze via `snapshot_process`, child
   reduction and unmap of dead stacks), `exec` holding the library lock, `exit` from any thread.
4. In-QEMU `libposix` tests for INV-0304-2/3/4/6, INV-0305-1/2/4, INV-0306-1 to 0306-4.
Exit: as step 1, plus `opus-review` on the fork and signal paths.

**Step 4: the suites, OpenSSL, CI and the docs** (about 800 lines of scripts and C plus pinned inputs; ADR-0309).
1. `ports/sources.lock` lines for the libc-test commit and `sqlite-src-3540100.zip` (hashes recorded at fetch),
   `ports/Makefile` targets, `tools/fetch-ports.sh`; `tests/posix/threads.script`, `threads-excluded.txt`, `t_threads.c`;
   `make test-posix-threads`; lines in `tools/ci-tests.txt` and `tools/test-map.txt` (`make check-ci-jobs`).
2. **After 9a:** OpenSSL rebuilt with `threads` (the 9a port's configure line), `tests/posix/ossl_threads.c`, 9a's `curl`
   test repeated. If 9a is not merged, steps 1 to 3 and 4.1 still merge; 4.2 waits and is noted here.
3. `docs/design.md` as-built text (threads, futex, quotas, signals, the quota table), `docs/backlog.md` (known limits:
   async cancel, compute loops, kernel stack size, requeue), `docs/dependencies.md` (no crate added; state it), invariants
   statuses, this report, ADR open-question notes where a check could not be written (ADR-0001 rule 6).
4. Final review by `opus-review`, `candidate-full/` run, HVF and TCG; then `stage-9b` is the architect's tag.

### Test plan and the choices in it

- **Conformance subset: musl's libc-test** (MIT, `src/functional` and `src/regression` programs named `pthread_*`, `sem_*`,
  `tls_*`, `thrd*`, `cnd*` and the fork/spawn ones), pinned by commit and SHA-256. Why: musl's thread code is what ADR-0302
  retains, so its author's tests are the right oracle; the programs are standalone with an exit status; MIT. **Open POSIX
  Test Suite rejected** for the build: GPL, about a thousand assertions of which many test options Zoraya refuses on
  purpose (process-shared, priority, real-time), which would need a second expected-failure source of truth. Exclusions
  live in `tests/posix/threads-excluded.txt`, each with a reason (ADR-0309 item 1).
- **SQLite:** `threadtest3.c` from `sqlite-src-3540100.zip` with the pinned amalgamation; shared-memory (WAL) scenarios
  excluded with the reason or rerun in rollback-journal mode.
- **Stress:** `t_threads.c`, ten cases (mutex counter, condvar ring with timed waits and broadcast, 100,000 `pthread_once`
  races, 2,000 create/join cycles, quota exhaustion and recovery, cancellation in each wait, `fork` with a sibling
  holding each lock, signals, `exit`/`exec` with siblings running, TLS).
- **OpenSSL with threads:** 8 threads of hashes, AES-GCM and `RAND_bytes` against single-threaded results; 4 threads of HTTPS
  to the runner's TLS test server; 9a's `curl -O` unchanged.
- **Both accelerators:** one step `test-posix-threads`; HVF in `make test-changed`; TCG in the candidate and nightly sets
  with TCG-scaled limits. Kernel invariants run in `make test-qemu`.

### Risks (most serious first)

1. **The two-phase process end** (ADR-0300 item 6): a sibling parked by a CPU ceiling, blocked in a step point, or woken
   during teardown is the kind of race that shows once in a hundred TCG runs. Mitigation: the host model first, the
   INV-0300-3 test with spinners, `opus-review`, and a rule that the owner frees nothing until the count is one.
2. **Splitting the long waits out of the library lock** (ADR-0302 item 9): 8,800 lines of host-tested logic, one wait missed
   is a deadlock only a two-thread test finds. Mitigation: the audit table, INV-0302-2 repeated per wait, and a debug
   assertion that a long wait is never entered with the lock held.
3. **The mapping lock and the stepped calls** (ADR-0301 item 6): a new sleeping kernel mutex taken in `memory_create`'s
   path; a `job_kill` between steps must still free everything (INV-0270-2) while a second thread waits on the lock.
4. **musl details found only by compiling:** the exact file exclusions, whether `pthread_create` needs `madvise`,
   `__vm_wait`'s use of futex on the shim, `pthread_getattr_np`. The step records each in `excluded.txt` as ADR-0141 did.
5. **Memory:** 64 KiB of kernel stack plus a 128 KiB default user stack per thread, all committed (no demand paging): a
   128-thread test uses about 25 MiB; room limits in tests must allow it. A fork child copies every stack until it unmaps
   them (ADR-0306 item 5).
6. **The 1024-entry deadline queue** with an ordered insert under masked interrupts (ADR-0301 open question, INV-0270-1's
   2 ms bound under TCG).
7. **Thundering herd** on `REQUEST_PENDING` and on broadcast without requeue (ADR-0304 item 4, ADR-0301 item 4); measured
   by the stress cases, with a signal thread and `futex_requeue` as the recorded upgrades.
8. **Pinned inputs:** libc-test is a git snapshot (hash stability of the archive is not guaranteed: keep the file in the
   ports cache and note a mirror); `make test` must not need the network, so both go through `tools/fetch-ports.sh`.
9. **SQLite's `threadtest3`** needs record locks and, for WAL, shared memory; scenarios may fail for reasons that are not
   threads. The exclusion rule keeps that honest.
10. **Scheduling fairness** with 128 threads of one job under a CPU ceiling (ADR-0285's park cap of 16 per pick): covered by
    the existing `t_cpu`, to be extended with a multi-threaded spinner.

### Open points left for the implementation sessions (decided as stated unless a measurement says otherwise)

`MAX_THREADS` 128; the 64 KiB `PT_TLS` bound; `--threads` as a word; `futex_requeue`; a signal thread; `close` as a
cancellation point; `pthread_getattr_np` for the main thread; omitting dead stacks from the snapshot copy.
