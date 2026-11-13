# TASK-27 report: stage 9, applications

Running report, updated after each part. Part 9b's design was written by a Sonnet session on branch `work/9b-design`
(reading, ADRs and plan only: nothing was built, no test was run; `make adr-check` and `make check` are the only commands run).

## Status for the architect (2026-11-12, `origin/main` at `6390dc6`)

| Part | State | Tag |
|---|---|---|
| **9a TLS for C, zlib, curl, wget** | implemented on `work/9a` (merged with `origin/main` at `a0cfc1f`, so it has 9b's kernel and libposix threads); ADR-0312 accepted 2026-11-13; ADR-0310 and ADR-0311 `proposed` (they only add decisions); OpenSSL rebuilt with threads; not merged; tests in the section "9a: OpenSSL with threads" below | |
| **9b POSIX threads** | **design accepted 2026-11-13** (ADR-0300 to ADR-0309 accepted by the architect as recommended; ADR-0308 supersedes ADR-0100's "no threads" and the other places it lists); kernel step implemented on `work/9b`; ADR-0313 (the kernel as built) accepted 2026-11-13; libposix step on `work/9b-libc` (ADR-0314, ADR-0315 proposed). Next: 9a, then 9b implementation and 9d in parallel, then 9c. Thread calls stay 56 to 61 (the highest call on main is 55, `device_write`) | |
| 9c CPython | implemented on `work/9c` (ADR-0320 `proposed`; `make python`, `make test-python`); gate closed, no candidate tag; see "9c: CPython" below | |
| 9d NetSurf | not started | |
| image addition (`EXTRA_ROOT`) | done inside 9a (`tools/run-disk.sh`, `make check-run-disk`), without descriptions for unknown programs; already on `work/9a` | |

### The ADRs of part 9b (all accepted 2026-11-13; they were `proposed`; one line each is a recommendation, not a decision)

| ADR | Title | Recommendation in one line |
|---|---|---|
| [0300](../adr/0300-threads-in-a-process.md) | Threads in a process | Thread objects named by handles inside the process: `thread_create(entry, stack, argument, tls)`, `thread_exit(clear, unmap)`, `thread_self`, `thread_interrupt` (calls 56 to 59); at most 128 per process; the end of a process is two-phase (the asker wakes one thread per process, which wakes its siblings in steps); a thread that exits frees its stack and charge and closes its own handles; **no new `Status`** (an interrupt is an early `Ok` of `futex_wait` and `sleep_until`, ADR-0006 rule 5 stands). |
| [0301](../adr/0301-wait-and-wake-on-an-address.md) | Waiting on an address | Two typed calls `futex_wait(address, expected, deadline)` and `futex_wake(address, count)`, private to a process; no requeue, no priority inheritance (musl's requeue becomes a wake); calls 60 and 61; the deadline queue is sized `MAX_THREADS_SYSTEM` + kernel timers + reserved (1024 if it fits) and becomes a heap; a per-process mapping lock, also held by `fork`'s copy. |
| [0302](../adr/0302-pthreads-in-libposix.md) | `pthread_*` in `libposix` | Keep musl's own `src/thread` over a shim; a library lock plus a memory lock; waits that can last are made outside the lock (`Pending` and retry); one port per thread; `PROCESS_SHARED` and priority inheritance refused with `ENOTSUP`. |
| [0303](../adr/0303-thread-local-storage.md) | Thread-local storage | The thread pointer is an argument of `thread_create` (no set-tls call); musl lays TLS out inside the stack mapping, which the creating job pays for. |
| [0304](../adr/0304-signals-with-threads.md) | Signals with threads | Per-thread masks and pending sets, per-process pending set; `pthread_kill` thread-directed; a process's request is taken by any waiting thread and routed to the lowest-tid thread that has it unblocked; no handler under a library lock. |
| [0305](../adr/0305-thread-cancellation.md) | Cancellation | Deferred only: a flag plus `thread_interrupt`, tested in the wrappers outside the lock; asynchronous mode acts as deferred; one patch to musl's `pthread_cancel.c`. |
| [0306](../adr/0306-fork-exec-and-exit-with-threads.md) | `fork`, `exec`, exit | `fork` takes the locks and freezes the other threads for the copy, the child has one thread and unmaps the others' stacks; `exec` holds the library lock until the old image ends; `exit` is the two-phase end. |
| [0307](../adr/0307-thread-quotas.md) | Thread quotas | A `threads` figure of its own (answers ADR-0260's open question), counting every live thread, derived as 3 x `procs` (room 96, untrusted rooms 288, session 600); old records stay valid; `EAGAIN`. |
| [0308](../adr/0308-posix-threads-enter-the-tier-list.md) | Threads enter the tier list | **The superseding ADR**: lists every accepted sentence the nine ADRs change (ADR-0100 items 2 and 5, ADR-0141, ADR-0180 item 9, ADR-0041, ADR-0102 item 5, ADR-0104 item 6, ADR-0106 items 2 and 5, ADR-0260, ADR-0261, ADR-0263, ADR-0285, INV-0006-2's lint, `pthread_create` on INV-0005-2's deny list); tier T9 with a list file; ADR-0100 itself stays `accepted`. |
| [0309](../adr/0309-how-9b-is-tested.md) | How 9b is tested | musl's libc-test thread tests as the conformance subset (Open POSIX Test Suite rejected), SQLite `threadtest3`, our `t_threads.c` stress program, OpenSSL rebuilt with threads. |

### Items for the architect

1. **Accept ADR-0300 to ADR-0308 or change them.** TASK-27's rule is that a part merges only if none of its proposed ADRs
   changes an accepted decision. Each of ADR-0300 to ADR-0308 does (ADR-0308 item 1 lists every sentence: ADR-0100 items 2
   and 5, ADR-0141, ADR-0180 item 9, ADR-0005's deny-list row, ADR-0041, ADR-0105 item 8, ADR-0102 items 2 and 5,
   ADR-0104 item 6, ADR-0106 items 2 and 5, ADR-0260's table rows, ADR-0261 point 1, ADR-0263 point 4, ADR-0285 point 4,
   INV-0006-2's lint). So the 9b implementation can be written on its branch, but **it does not merge until all of
   ADR-0300 to ADR-0308 are accepted** (ADR-0309 changes no accepted text). The task's done criterion ("ADR-0100's 'no
   threads' is superseded by an accepted ADR") is met by your accepting ADR-0308; agents did not and will not set any
   status. ADR-0100 is not edited. Not superseded: ADR-0006 rule 5 and INV-0006-6 (no `Interrupted` status was added).
2. **Choices made without you that are cheap to reverse now and dear later:** the thread is a kernel object with a
   handle (not a `clone`-style call); futex without requeue and PI; musl's pthread code kept (a patch to
   `pthread_cancel.c` and one for `pshared`); the `threads` quota counts the first thread of a process too and has no
   typed word yet (`--threads` deferred); `fork` freezes the other threads for the copy; `PTHREAD_PROCESS_SHARED` is refused;
   `MAX_THREADS` 128 per process.
3. **The plan needs part 9a for one item only** (OpenSSL rebuilt with threads, step 4.2); steps 1 to 3 and the rest of step 4
   do not. 9a has not started (curl was deferred from stage 8 on 2026-11-12), so step 4.2 is **outside 9b's done
   criterion until 9a is merged**; 9b merges without it and the step is recorded as pending.
4. **Confirm ADR-0313 items 6 to 8.** They (`Members::shrink` with `try_reserve_exact`, the hold on a `Move`d handle,
   the room kept for the spawn's child) were added by the step-1 reviews after you accepted items 1 to 5; they refine the
   implementation and change no accepted guarantee. Recorded in the ADR's open questions; its text and status are untouched.
5. Merge conflict note for the orchestrator: this file is new on `work/9b-design`; `work/9-docs` may create the same
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
   the two-phase end state machine (one wake per process under `TREE`, the woken thread wakes its siblings in steps) and the
   FIFO wait list keyed by address with the 64-per-step wake; host tests including
   an exhaustive interleaving model (INV-0300-6, INV-0301-2's model half). Add to `Cargo.toml` workspace and `tools/test-map.txt`.
2. `crates/zoraya-abi/src/lib.rs`: calls 56 `thread_create`, 57 `thread_exit`, 58 `thread_self`, 59 `thread_interrupt`,
   60 `futex_wait`, 61 `futex_wake` (the numbers follow the highest call on `main`, 52, plus `work/n6`'s 53 to 55; the
   step takes the next free numbers if another branch has merged first); **no new `Status`** (17 stays free, INV-0006-6's
   `Status::ALL.len() == 17` is untouched); argument types `Word32` and `Count`; the `thread`
   object type (signal `TERMINATED`, rights `WAIT | SIGNAL`, not duplicable); `quota.rs` (threads word at bytes 16 to 20,
   `QuotaField::Threads = 3`, usage record 152 bytes, decoders total); `crates/zoraya-charge/src/lib.rs` (`Field::Threads`);
   `crates/zoraya-rt/src/lib.rs` stubs. Tests: the table lints (INV-0300-1) and **the amended INV-0006-2 lint in
   `crates/zoraya-abi/src/tests.rs`** (`Address` and `Word32` allowed for `futex_wait` only, three blocking calls instead
   of two; INV-0308-5, INV-0301-5), decoder tests (INV-0307-2).
   `crates/zoraya-time`: `DeadlineQueue` becomes a binary min-heap with a back-index (INV-0301-7).
3. Kernel: `kernel/src/sched.rs` (thread record gains `interrupted`, the futex node, the process slot; `spawn_user` for
   `thread_create`), `process.rs` (thread list, `ending` flag and owner, park and wake of all threads, `snapshot_freeze` used
   by `snapshot_process`, mapping lock), `syscall.rs` (the six calls; the aligned 32-bit user read through the fault-resume
   path), `sleep_until` and `futex_wait` (consume `interrupted`; early `Ok` with value 1; `port_wait` is not touched, the library wakes
   it with a user packet), `object.rs` and `job.rs`/`charge.rs` (the thread type and
   the `threads` charge in the order processes, threads, memory), `timer.rs` (`QUEUE_SIZE` derived from `MAX_THREADS_SYSTEM`, with a `const` assertion; `on_interrupt` pops at most 64 per pass),
   `job.rs` (`request_exit`/`cont` wake one thread per process, never all, under `TREE`), `thread_exit` (refunds record, stack
   and `threads` at the call, reaper frees the kernel stack, the object keeps only `TERMINATED`), `memory/user.rs`
   (the lock around `map_reserve` to `map_commit` and unmap, an RAII guard released on every `Cancelled` path, also taken by
   `snapshot_process` for its whole call), `steps.rs` (units for wake and thread end).
4. Plumbing: `job_set_limits`/`job_get_usage` for the new field; `crates/zoraya-rooms` derives `threads = 3 x procs`; `init`
   sets the session job to 600 and the keeper the untrusted job to 288 (INV-0307-3); `grants` prints the derived figure.
5. In-QEMU tests in `kernel/src/testing.rs` with user programs `user/src/bin/t_thread.rs` (raw ABI, no libposix): INV-0300-2
   to 0300-5, INV-0301-1 to 0301-4 and 0301-6, INV-0307-1 and 0307-4, the freeze (INV-0306-5's kernel half), `t_bound`
   extended with 128 waiters **and a `job_kill`/`job_resume` of 4 processes x 128 threads** (INV-0301-4), a capped job with a
   parked sibling (INV-0300-3), 2,000 detached threads checking slots and usage (INV-0300-7), a sibling looping on
   `mmap`/`munmap` during a `snapshot_process` (INV-0306-6, kernel half), 1000 timed waiters filling the deadline queue
   (INV-0301-7). `tools/test-map.txt` lines for `kernel/src/*`, `crates/zoraya-thread/`.
Exit: `make check`, `make test-changed`, the candidate run on HVF and TCG, `opus-review` with no open error.

**Step 2: the thread core of `libposix`** (about 1,500 lines plus tests; ADR-0302, 0303, 0308; INV-0302-*, 0303-*, 0308-1/2).
1. Audit first (a table in this report): every call site of `Sys::wait`, `wait_any`, `sleep_until`, the stream retries and
   the service reply waits, each marked long or short (ADR-0302 item 9).
1b. **Measure the mapping count:** run a CPython-sized program (or a stand-in with its image, heap and library mappings) and
   count its mappings of the 256; with 128 thread stacks on top the sum must fit. If it does not, `MAX_THREADS` falls or
   the library merges mappings; record the figure in this report (ADR-0300 open question).
2. `crates/zoraya-posix/src/lock.rs` (futex mutex over a `Wait` trait, model-tested, INV-0302-3), `MEM_LOCK` around the
   heap, mapping table and counters, `with()` taking the lock; the thread table and tids (main = 1); per-thread port;
   `Pending` split of the long waits in `process.rs`, `net.rs`, `jobs.rs`, `fd.rs` with wrapper retry loops in
   `ffi.rs` and `ffi/procs.rs`; host tests with a fake kernel of two threads (`crates/zoraya-posix/tests/kernel`).
3. musl side: `ports/musl/musl.mk` (retain `src/thread`), `ports/musl/excluded.txt` (the files of ADR-0302 item 1),
   `ports/musl/zoraya/syscall.h` (the new routes), `clone.c`, `unmapself.c`, `0002-no-pshared.patch`; delete the Rust
   stubs of `sem_*`, `pthread_setcancelstate`, `pthread_testcancel` and the cleanup functions; Rust `pthread_getattr_np`,
   the three scheduling calls (`ENOTSUP`), `sysconf` values (ADR-0308 item 4); the errno table (`OverQuota(Threads)` to `EAGAIN`, INV-0302-7);
   **the thread's exit path closes its own thread handle and port** before `thread_exit` (detached and joinable alike).
4. `docs/adr/posix-surface-T9.txt`, `tools/posix-surface-check` (third list, deny list without `pthread_create`),
   `make check-ports`; `make test-libposix` extended.
5. Tests (`tests/posix/t_posix.c` or a new `t_pthread.c` with a script step): create, join, detach, mutex, cond, once,
   keys, TLS and `errno` per thread (INV-0303-1/2/4), the blocked-`read`-and-`write` pair (INV-0302-2), stack guard
   (INV-0302-6), quota `EAGAIN` (INV-0303-3), no futex call without threads (INV-0302-5).
Exit: as step 1; `make test-posix`, `make test-libposix` unchanged results for the single-threaded workloads.

**Step 3: signals, cancellation, `fork`, `exec`, exit** (about 1,000 lines plus tests; ADR-0304, 0305, 0306).
1. `crates/zoraya-posix/src/signals.rs` per-thread mask and pending sets, the routing rule, `sigwait`, `pthread_kill`,
   `tkill` route; **the futex shim's three outcomes after an early return** (`-EINTR` only if a handler ran, 0 as a spurious
   wake otherwise, `-ECANCELED` for a masked cancel; ADR-0304 item 11); host model (INV-0304-1, 0304-5, 0304-7, 0304-8,
   0305-5).
2. `ports/musl/zoraya/0003-deferred-cancel.patch`, the `__syscall_cp` definition in the shim, `__testcancel` in the wrappers
   outside the lock, `tools/check-cancel-points.sh` and a `check-cancel-points` entry in the Makefile's `CHECK_STEPS`
   (INV-0305-3), `sigaction` refusing 32 to 34.
3. `ports/musl/zoraya/atfork.c`, the `fork` wrapper changes in `ffi/procs.rs` (locks, freeze via `snapshot_process`, child
   reduction and unmap of dead stacks), `exec` holding the library lock, `exit` from any thread.
4. In-QEMU `libposix` tests for INV-0304-2/3/4/6/7/8, INV-0305-1/2/4/5, INV-0306-1 to 0306-4 and 0306-6.
Exit: as step 1, plus `opus-review` on the fork and signal paths.

**Step 4: the suites, OpenSSL, CI and the docs** (about 800 lines of scripts and C plus pinned inputs; ADR-0309).
1. `ports/sources.lock` lines for the libc-test commit and `sqlite-src-3540100.zip` (hashes recorded at first fetch, trust on
   first use as ADR-0109 accepts; libc-test pinned to an archive whose bytes do not change, an immutable mirror asset or the
   file as first fetched, kept in `build/ports/src`),
   `ports/Makefile` targets, `tools/fetch-ports.sh`; `tests/posix/threads.script`, `threads-excluded.txt`, `t_threads.c`;
   `make test-posix-threads`; lines in `tools/ci-tests.txt` and `tools/test-map.txt` (`make check-ci-jobs`).
2. **After 9a (outside 9b's done criterion until 9a is merged):** OpenSSL rebuilt with `threads` (the 9a port's configure line), `tests/posix/ossl_threads.c`, 9a's `curl`
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
   path and held by `fork`'s whole copy (ADR-0306 item 2); a `job_kill` between steps must still free everything
   (INV-0270-2) while a second thread waits on the lock, and every `Cancelled` path must drop it (RAII guard).
4. **musl details found only by compiling:** the exact file exclusions, whether `pthread_create` needs `madvise`,
   `__vm_wait`'s use of futex on the shim, `pthread_getattr_np`. The step records each in `excluded.txt` as ADR-0141 did.
5. **Memory:** 64 KiB of kernel stack plus a 128 KiB default user stack per thread, all committed (no demand paging): a
   128-thread test uses about 25 MiB; room limits in tests must allow it. A fork child copies every stack until it unmaps
   them (ADR-0306 item 5).
6. **The deadline queue**: sized from `MAX_THREADS_SYSTEM` (services and trusted jobs have no thread figure), about 40 bytes an
   entry; decided as a heap so insert, cancel and pop are 10 moves, not a linear scan and rotate under a masked spinlock
   (ADR-0301 item 5, INV-0301-7). The measure of the heap under TCG is still open (INV-0270-1's 2 ms bound).
7. **Thundering herd** on `REQUEST_PENDING` and on broadcast without requeue (ADR-0304 item 4, ADR-0301 item 4); measured
   by the stress cases, with a signal thread and `futex_requeue` as the recorded upgrades.
8. **Pinned inputs:** libc-test is a git snapshot (hash stability of the archive is not guaranteed: pin an immutable mirror
   asset or keep the file first fetched in the ports cache, `build/ports/src`); the hash is recorded at first fetch (trust on
   first use, ADR-0109); `make test` must not need the network, so both go through `tools/fetch-ports.sh`.
9. **SQLite's `threadtest3`** needs record locks and, for WAL, shared memory; scenarios may fail for reasons that are not
   threads. The exclusion rule keeps that honest.
10. **Mappings:** 128 stacks, the heap and the image's mappings may not fit in 256 mappings; a CPython-sized program is
    measured in step 2 (item 1b).
11. **Scheduling fairness** with 128 threads of one job under a CPU ceiling (ADR-0285's park cap of 16 per pick): covered by
    the existing `t_cpu`, to be extended with a multi-threaded spinner.

### Open points left for the implementation sessions (decided as stated unless a measurement says otherwise)

`MAX_THREADS` 128; the 64 KiB `PT_TLS` bound; `--threads` as a word; `futex_requeue`; a signal thread; `close` as a
cancellation point; `pthread_getattr_np` for the main thread; omitting dead stacks from the snapshot copy.

### CI timing (2026-11-13): the whole-suite TCG runs and `QEMU_SLOW_TIMEOUT`

CI on `main` (`02d32e5`) failed `test-qemu-tcg-smp4-gicv3` at the 300 s limit (115 of 128 tests done, running
`room_keeper_from_a_process`). The CI logs keep only the head and tail of a run, so per-test times are not in them. What the
logs show: the same suite (128 tests) took 201 s (candidate `work/9b-d`, gicv3), 267 s (candidate, gicv2) and 168 s (`main`,
gicv2) in green runs, and the failed run had every timed figure about 2x the green ones (`t_bound` free_source 2365 ms against
1215 ms, `t_storage` 100 syncs 1172 ms against 662 ms; the first frame tests 7-11 s against 4-6 s): a slow runner, not a hang.
Locally (HVF host, `make test-qemu-tcg-default`, one run) the suite passed in about 160 s; `t_thread` took 21 s of it (hand-off
rounds 11.3 s, interrupts 5.4 s, joined/detached rounds 1.4 s each). `t_thread` has no TCG branch and its round counts are
invariant figures (INV-0300), so none was reduced. `QEMU_SLOW_TIMEOUT` went from 300 s to 420 s (57% over the slowest green
run, 267 s); the CI job limit of 10 minutes still holds (about 1 minute of setup plus 7).

### CI flake (2026-11-13): `t_cpu`'s masked-time limit off HVF

`cpu_ceiling_from_a_process` failed three times at `t_cpu.rs:181`, twice on TCG (factor 12, limit 2 ms x 64 = 128 ms): `the exit
of 40 sleepers: 156708320 ns masked` (`main` `c1e1b34`, `test-qemu-auto-default`) and `rooms next to a woken process: 144941856 ns`.
The failing interval held 55 units of work, the same 55 as the same subtest in a green TCG run of the same day (1579 us, job
`tcg-smp4-gicv3` of `main`'s run 43387452404) and under HVF locally: 87 us on `c1e1b34`, 106 us on `120ee31` (before 9b), 55
units on both; every other subtest's units are equal too (58, 64). So the time was the host's (the vCPU stalled with IRQ masked),
not a regression of 9b's teardown or the deadline heap. Off HVF `t_cpu` now takes `t_bound`'s limit of 1 s for the time and keeps
`MAX_UNITS` (ADR-0270) as the check; under HVF the 2 ms limit is unchanged. The one HVF failure (2696 us on a loaded host, the deep
job) is left as is (`docs/backlog.md`).

## 9b step 2, second slice of `libposix` threads (branch `work/9b-libc`)

What exists (ADR-0315, proposed; ADR-0314 items 2 to 4 are refined by it):

- **The `Pending` split** (INV-0302-2): `Posix::wait_for` returns `errno::PENDING` once the process has a thread; the boundary's `state()` waits on a
  port of its own with the library lock free and runs the method again with the outcome (`Resume`); a write keeps its progress. Host tests: `tests/files.rs`
  (`deferred`). In QEMU: `t_threads waits` (a reader in `read`, a sleeper, a `pselect`, while the main thread calls `getpid`, `open`, `write`, `stat`, `printf` 20 times
  within 0.3 s) and `blockedexit` (`exit()` flushes stdio while a sibling is blocked in `read`).
- **The thread table, per-thread signal state, `tkill`, `rt_sigprocmask`, routing of process signals** (`threads.rs`, host tests `tests/threads.rs`); musl's
  `__block_*_sigs` now save the mask (they were no-ops and left `pthread_create`'s `sig_mask` uninitialised: found by the test).
  `t_threads signals`: `pthread_kill` wakes a thread in `read` and in `nanosleep` (`EINTR`, handler on that thread) and in `sem_wait` (goes on); masks are per thread.
- **Deferred cancellation** (`cancel.c`): `pthread_cancel` interrupts the target; futex waits act on the flag before and after; library waits call `__testcancel` once woken.
  `t_threads cancel`: `read`, `nanosleep`, `sem_wait`, `pthread_cond_wait` and a `pthread_testcancel` loop, cleanup handler once, `PTHREAD_CANCELED`.

Ran in this session (HVF, after merging `origin/work/9b` at its head of 2026-11-13): `make check` (16 of 16), `cargo test -p zoraya-posix` (all),
`make test-posix-threads` (all 17 cases), `make test-libposix`, `make ports` with `check-ports` (dash, busybox, make, pdpmake, tcc and toybox link the threaded `libc.a`;
tcc builds a hello world for Zoraya), and `make test-posix` limited to `hello`, `smoke` and `invariants` (signals, EINTR, raise, execmask, blocked: the single-thread paths
through the thread table). Not run: the rest of `make test-posix`, TCG, `make test-changed`.

Not done (the next session; each is in ADR-0315 item 6 or below):

- Fork freeze and child reduction (ADR-0306: the kernel `Freeze` is in `work/9b`, the library side is not), `exec` holding the lock, `atfork.c`; tests for fork with siblings running and exec from a thread.
- `sigwait`/`sigtimedwait`, `kill(getpid())` from a thread that blocks the signal (stays in that thread), `pthread_getattr_np`, thread names, `check-cancel-points`, async-signal tests of INV-0304-3/-5/-7/-8 (1000-round forms), INV-0305-2/-4/-5.
- In-QEMU cases for `waitpid`, `accept` and `sem_wait` (as a library wait) in a thread: `posixrun` has no broker, so `posix_spawn` and `fork` hang there; they belong in `tests/posix/t_posix.c`.
- The suites of step 4: libc-test functional subset, SQLite `threadtest3` (SQLite is a pinned port, the amalgamation only: `threadtest3.c` needs the source zip), `threads-excluded.txt`, the mapping-count measurement, INV-0302-5 and INV-0308-3 checks, a TCG run of `test-posix-threads`.

## 9b step 2, third slice of `libposix` threads (branch `work/9b-libc`)

What exists (ADR-0315 items 9 and 10, INV-0315-5 to -7):

- **Fork with siblings and `exec` from a thread** (ADR-0306): `ports/musl/zoraya/atfork.c` (prepare, parent, child: musl's `fork.c` and `__post_Fork` around the snapshot, stdio locks of dead
  threads freed, other threads' stacks unmapped in the child), `MEM` taken around `snapshot_process` only, the thread table reduced in the child (`threads_after_fork`), `exec` holds
  the library lock to the end of the old image. The `EAGAIN` guard is gone. `tests/posix/t_posix.c`: `tfork` (500 forks, five kinds of sibling), `tforkthread`, `texec` (100 execs), `tspawn`.
- **`sigwait`, `sigtimedwait`, `sigwaitinfo`**, `pthread_getattr_np`, `pthread_getname_np`, `pthread_setname_np`; `open` is a cancellation point.
- **1000-round cases** in `t_threads`: `handlerlock`, `condsig`, `cancelrounds`, `maskedcancel`; in `t_posix`: `treroute`, `tsig`, `twait`.
- **Suites:** `make test-posix-suites` (13 libc-test thread tests; exclusions with reasons in `tests/libc-test/threads-excluded.txt`), wired into `TEST_STEPS`, `tools/ci-tests.txt`, `tools/test-map.txt`,
  `tools/plan-lib.sh`. `tests/posix/threads.script` runs in `make test-posix` (both accelerators in CI). `make posix-threadtest3` is not a step (no `fcntl` record locks).

Not done: INV-0306-3 and -5 checks; INV-0308-3 over fresh logs of all workloads; the mapping-count measurement; a TCG run of `test-posix-threads` and the full `make test-posix` (see the final report of the session); `accept` in a thread (no loopback server in the session).

---

## 9b step 4, record locks and SQLite's `threadtest3` (branch `work/9b-locks`)

SQLite's default unix VFS takes `fcntl` record locks; without them every scenario of `threadtest3` failed at its first write with
`SQLITE_IOERR_LOCK` (3850). ADR-0319 (`proposed`; it was numbered 0317 and renumbered because `work/9d` uses 0317 and 0318) decides Option A:
advisory record locks in `libposix`, in the process (`F_SETLK`, `F_SETLKW`, `F_GETLK`, `F_OFD_SETLK`, `F_OFD_SETLKW`, `F_OFD_GETLK`;
`crates/zoraya-posix/src/records.rs`, `fcntl_locks.rs`, `ffi.rs`), released on `close` and `dup3`. **Documented limit: a second process does
not see the lock** (the file service has no owner per process). `flock` stays absent. ADR-0005 item 6 says record locks are absent; ADR-0319
item 8 and an open-question note in ADR-0005 record the tension (architect's to resolve).

What holds now for 9b's "done when" (ADR-0309): (1) musl's libc-test thread subset (`make test-posix-suites`, exclusions in
`tests/libc-test/threads-excluded.txt`); (2) SQLite `threadtest3`, 7 rollback-journal scenarios (`make test-posix-threadtest3`, a step of
`make test` and of CI, HVF and TCG; the WAL scenarios are excluded: no shared mapping); (3) the stress program `t_threads.c`
(`make test-posix-threads`); (4) OpenSSL with threads (9a's section); ADR-0308 supersedes ADR-0100's "no threads".

Ran in this session (M4, `make test-posix-threadtest3`, `FULL=1`): all 7 scenarios (`cgt_pager_1 dynamic_triggers create_drop_index_1 lookaside1 vacuum1 stress1 shared1`) pass under HVF and under TCG, "0 errors out of 7 tests", `zoraya-fsck` clean after each; no scenario needed a fix or an exclusion. Not run: a CI run (the candidate is the orchestrator's).

## 9a: TLS for C programs, zlib, curl, wget

Branch `work/9a` from `origin/main` at `972bbf0`; one Sonnet 5.5 session, 2026-11-13.

### What exists

- `ports/sources.lock`: OpenSSL 3.5.9, zlib 1.3.1, curl 8.23.0, wget 1.25.0 (version, SHA-256, URLs; `make fetch-ports`
  downloaded and verified them; `docs/dependencies.md` and `ports/README.md` list them with licences and patches).
- `make apps` (`ports/Makefile`): zlib, OpenSSL (static, `no-threads`), curl and wget with `zcc`, 6.7 MiB each without debug
  information; the end of the target checks the logs for `-D__linux` and that every OpenSSL patch is under `__zoraya__`.
  Patches: `ports/patches/openssl/` (3), `curl/` (config.sub), `wget/` (plain-fd BIO, no `MSG_PEEK`).
- `getrandom` in libposix (`crates/zoraya-posix/src/random.rs`), the `ZPBX` bootstrap form (`zoraya-abi`), the share in `zosh`
  (`user/src/bin/zosh.rs`, `user/src/posix_child.rs`).
- Descriptions `user/describe/posix/curl.cmd` and `wget.cmd`; the key `names` (`zoraya-describe`); `grants` and the run give a
  POSIX program the roots directory and show the `net:` line; `posix_child::load` reads the program into its image directly.
- `tools/run-disk.sh` `EXTRA_ROOT`; `ports/apps/zshim.c`.
- 22 names in `libc.a` (now 337 exports); ADR-0310 to ADR-0312 with 6 INV rows (the Total is 538).

### Tests (what ran in this session, on the M4, HVF)

| Command | Result |
|---|---|
| `cargo test -p zoraya-posix --test random` | 7 passed |
| `cargo test -p zoraya-describe -p zoraya-shell -p zoraya-command` | passed (the `names` key, `url_file_name`, 4 `curl_o_*` plans) |
| `cargo test -p zoraya-abi` | 69 passed (one new: the `ZPBX` flags) |
| `make check` | 17 of 17 passed (adr-check 0 errors; `check-run-disk` is new) |
| `make test-apps` (new) | passed: both programs over HTTP and TLS, expired and wrong-name certificates refused by both with no request byte at the server, `grants`, refused `-O` names, `-k` as a usage mistake |
| `make test-apps FULL=1` | passed under HVF and TCG |
| `make test-net-posix`, `test-libposix`, `test-boot` | passed (they cover what the bootstrap and `grants` changes touch) |
| `make test-files` | passed (an earlier `rootfs/etc/wgetrc` had made `/etc` appear in the root listing; it is gone) |
| `make test-hand-run` | passed: the production `run-net` image with `curl` and `wget`: HTTP works, HTTPS reaches the certificate and is refused (curl exit 60, wget "cannot verify") |

Not run here: `make test-changed` as a whole, `test-net`, `test-posix*`, `test-tour-gui`, `test-gui`, `test-crash`, `test-qemu` (CI's candidate run
covers them), TCG for anything but `test-apps`, mutation testing. Disk: 44 GB free at the start, 42 GB at the end; the worktree
is `zoraya-9a` with its own `target/` and `build/`. The CI workflow changed: the build job also builds `make apps` (cache paths and `ports-build-v3`; timeout 30 minutes).

### Decisions

See ADR-0310 to ADR-0312 and Items 2 and 4 above. Not done in 9a: `getrandom` for programs a POSIX parent starts, `tls.roots`
for C programs, threads (9b), a hand-written mutation list for the new code (`tools/mutants/`).

### For the independent review (security and trust boundaries first)

1. `crates/zoraya-posix/src/random.rs`, `ffi/sockets.rs` (`getrandom`), `ffi.rs` (`bootstrap`'s `ZPBX` parse: handle counts,
   closing what is not a channel), `user/src/posix_child.rs` (the message and every close path on failure), the share in `zosh`.
2. The description files (`curl.cmd`, `wget.cmd`): the options list is the policy (no `-k`, no `--cacert`, no config file); the `names` key
   (`crates/zoraya-describe`, `crates/zoraya-shell/src/grants.rs`): can the name the shell approves differ from the file curl
   makes? (`url_file_name` is deliberately stricter than curl.)
3. TLS root handling in C programs: `zosh.rs` `ready_from_volume` (the roots directory added with the network),
   `ports/patches/openssl/0003`, `ports/patches/wget/0001` (a plain-fd BIO).
4. `tools/run-disk.sh` `EXTRA_ROOT`, and `grants` for POSIX programs (`grants_report`, `Host::grants`).
5. The wget patches and the shim (`ports/apps/zshim.c`) are C that runs with the program's authority only; skim.

## 9a: fixes after the independent review (opus-review), same branch

Decisions made without the architect, all in ADR-0310 to ADR-0312 (still `proposed`):

- **Finding 2 (the one to look at).** `-o`/`-O` gave the program the whole current directory (the shell folded the one
  `create NAME` with the directory's read). Chosen: the preferred fix. `curl.cmd` and `wget.cmd` say `cwd none`; the one
  name reaches the program as a directory handle narrowed to it at the current directory's prefix, and `grants` says
  `create or replace NAME (option -O)` and nothing else for the directory (ADR-0311 item 5, INV-0311-1). A line with no
  output file starts the program in `/bin`, read only; a path below the directory gets a placeholder entry with a dead connection as a
  starting point (nothing readable; review W1). Alternative for the architect: `cwd read` as before, and say that `-o`/`-O` write the whole directory.
  Test: `make test-apps` (`grants` lines; `-d @secret.txt` fails with exit 26 for a file the line does not give, and works
  with `+r .`). wget's `.wget-hsts` is gone with HSTS off.
- **Finding 1.** The patches (`ports/patches/{openssl/0004,curl/0002,wget/0003}`, ADR-0310 item 6, INV-0310-4): no
  environment variable and no configuration file decides trust. Tested with `+r .` on the line so that a file could be
  read: `.curlrc` `insecure`, `.wgetrc` `check_certificate = off`, `WGETRC`/`SYSTEM_WGETRC`, and `CURL_CA_BUNDLE`,
  `SSL_CERT_FILE`, `SSL_CERT_DIR`, `OPENSSL_CONF` naming nothing (a valid server still works, an expired and a wrong-name one
  still fail; the server's log shows 0 request bytes).
- **Findings 3 and 4.** `--with-rand-seed=getrandom no-tls1 no-tls1_1 no-dtls` (and the `-method` forms); the end of `make apps`
  checks that neither program names `/dev/urandom` or `/dev/random` or links `TLSv1_method` and the like (INV-0310-5). Not tested:
  a run without the random connection (the shell always gives one); `cargo test -p zoraya-posix --test random` covers the
  library side (`ENOSYS`). A TLS 1.0 server is not in the test PKI.
- **Finding 5.** `rootfs/etc/wgetrc` removed (patch 0003 of OpenSSL gives the roots; wget reads no rc file); `files-write.script` back.
- **Finding 6.** The descriptions now say that `-d @FILE` and `-H @FILE` read only what a `+r`/`+w` of the line gives, and that no
  configuration file is read; the tests above.
- **Findings 7 to 11.** `url_file_name` cuts `?` and `#` before the path (host tests); `/system/tls` by the same `REACH` test
  in the run and in `grants`; a `ZPBX` message with an unknown flag keeps `SELF` (code now agrees with the abi doc);
  `getrandom` keeps `EINTR` and `EINVAL` (its wait is a signal wait point; no host test of the `EINTR` path: the fake kernel
  has no signal source), the limit of 64 connections per shell is documented in `random.rs` and ADR-0310; the SAFETY
  comment of the image mapping states what it relies on.

Ran in this session: `make check`; `cargo test -p zoraya-describe -p zoraya-posix -p zoraya-shell`; `make apps`; `make test-apps`
(one run, HVF, 235 steps); `make test-hand-run`; `make test-files`. Not run: `FULL=1` (TCG), `make test-changed`, `test-net-posix`.

## 9a: OpenSSL with threads, and the merge with `main` (2026-11-13)

Done on `work/9a` after ADR-0312 was accepted and 9b's `libposix` pthreads (ADR-0314, ADR-0315) reached `main`.

- **Merge.** `origin/main` (`a0cfc1f`) merged into `work/9a`; conflicts in the Makefile step lists, `ports/Makefile`, `ports/sources.lock`,
  `tools/plan-lib.sh`, `tools/test-map.txt`, the fake kernel of `crates/zoraya-posix/tests/kernel/mod.rs`, `docs/adr/invariants.md`
  (Total recomputed: 615) and this file; both sides kept. ADR numbers do not collide (0310 to 0312 are 9a's; 0313 to 0316 are 9b's and
  first-lookup's). `work/9a-leak`'s fix (`posix_child::load` attaches the image's object itself) was merged into `work/9a` earlier
  (`git merge-base --is-ancestor origin/work/9a-leak HEAD` holds); nothing to fold in.
- **OpenSSL with threads.** `ports/Makefile` `OPENSSL_OPTS` has `threads` in place of `no-threads`; the rest (`no-tls1 no-tls1_1 no-dtls no-ssl3
  no-engine no-dso no-shared`, `--with-rand-seed=getrandom`, the four patches) is unchanged, and `getrandom` is still the only seed source (the end
  of `make apps` checks it as before). Its locks are OpenSSL's own `threads_pthread.c` (`CRYPTO_THREAD_*` over `pthread_rwlock_*`,
  `pthread_once`, `pthread_key_*`) on musl's pthreads in `libc.a`; the build and the test link and run without a shim. curl and wget stay
  single-threaded: curl's options already say `--disable-threaded-resolver` (synchronous resolver; chosen, recorded in ADR-0310 item 2)
  and wget's `--disable-threads`. `curl --version` now lists `threadsafe` (a feature of the build with OpenSSL's threads), which
  `tests/shell/apps-launches.script` expects.
- **The test of ADR-0309 item 4 (TLS half).** `tests/apps/tlsthreads.c` (built by `ports/Makefile`, copied to the test volume by `make test-apps` only;
  described by `user/describe/posix/tlsthreads.cmd`, so the shell gives it a connection and `/system/tls`): four threads, one shared `SSL_CTX`, three
  handshakes each per URL. Against the valid server all 12 succeed and return the page; against the expired and the wrong-name server all 12
  are refused (and the Makefile's check of the server logs shows 0 request bytes). The shell's connection account for a POSIX program holds two
  connections (`POSIX_NET_BUDGET`), so a thread whose `connect` fails waits 50 ms and tries again: at most two handshakes are in flight at once.
- **Not done:** the SHA-256 / AES-GCM / `RAND_bytes` check of ADR-0309 item 4 (`tests/posix/ossl_threads.c`).
- **A stale build trap found:** `build/ramdisk.bin` is rebuilt only when missing, so a new `user/describe/posix/*.cmd` (or any user-program change)
  does not reach `make test-apps`' kernel until `build/ramdisk.bin` and `build/zoraya-kernel.elf` are removed (CI starts clean).

## Status for the architect (9a)

1. **ADR-0310 and ADR-0311 are `proposed`.** They only add decisions (names for `libc.a`, the `names` description key, the roots directory). Accept,
   change or reject them; 9a does not merge on their account alone, but the descriptions and the shell code depend on them.
2. **wget is GPL-3.0-or-later.** It is a separate program in the image, built from the pinned tarball, not linked into anything; the precedent is
   ADR-0109 (GNU make, GPL-3.0, in the image). `docs/dependencies.md` lists it. Confirm that this is acceptable.
3. **SQLite's source is public domain** and is used as a test input only (`threadtest3` in 9b, W3's source); it is not linked into the kernel or a
   `no_std` crate.
4. **Linux CI build of the ports** (`make apps` in the build job): verified by the candidate run 43371864611 (green on `11d77f2`, as the orchestrator reported; not rerun here). The
   candidate for this head is the orchestrator's to tag.
5. **The `libc-test` fetch is pinned by git commit** (`ports/sources.lock`: `libctest 63e36e9...`, with the SHA-256 of the tarball made from it), not
   by a release; there is no upstream release to pin. A new commit means a new line and a new hash.
6. **Open from the earlier 9a report:** the ~8.5 MiB leak per `curl`/`wget` launch was fixed by `work/9a-leak` (merged; the `apps-launches` step
   runs 16 launches on 128 MiB); the choices of Item 4 of the earlier status (wget host triple, `rand`/`srand` exported, no `-k`) stand.

## 9c: CPython (branch `work/9c`, 2026-11-13)

### What exists
- `ports/sources.lock` pins Python-3.15.2.tar.xz by the SHA-256 python.org publishes (`8b5536d2...797de`); python.org also publishes a
  Sigstore bundle, and checking it is optional and not done (an earlier note here and in the session said python.org published no SHA-256: wrong, corrected on the architect's word).
- `make python` (`ports/Makefile`): host Python 3.15.2 (`build/ports/host`), SQLite for the target, `libpyshim.a` (`ports/apps/pyshim.c`), the static
  cross build with `zcc`, the install, byte code compiled by the host Python (`unchecked-hash`), and `build/rootfs-python`: `bin/python3` 15.5 MB and
  `usr/lib/python3.15` 37 MB, 1226 files, 52 MB in all (`du` 52,364 KiB). The recipe ran end to end three times in this session (each patch change rebuilds it from the unpacked source).
- Five patches in `ports/patches/python/` (`_ssl` fd BIO; environment, byte code and user site off; `asyncio` self-pipe on `os.pipe`; `main` on an 8 MiB thread stack; no `pyvenv.cfg` search in `getpath.py` and `site.py`), `ac_cv_header_sys_ioctl_h=no`
  (so `set_blocking` uses `fcntl`: libposix's `ioctl` serves terminals only).
- `user/describe/posix/python3.cmd`; `tools/run-disk.sh` with several colon-separated `EXTRA_ROOT`s and `DISK_SIZE`; `tests/python/` (`smoke.py`, `https_client.py`, `regr.py`, `regr-modules.txt`, `regr-ignore.txt`), `tests/shell/python.script`, `make test-python`;
  CI wiring (`TEST_STEPS`, `tools/ci-tests.txt`, `tools/test-map.txt`, `tools/plan-lib.sh`, the workflow's cache paths and 30-minute timeout, `ci-build` builds `python`).
- ADR-0320 (`proposed`, INV-0320-1 to -5), `docs/design.md` section "CPython (stage 9, 9c)".

### What was run (HVF, M4, this session)
- `make python`: ok (3 times). `make adr-check` and `make check`: ok after the ADR and the index (before the CI wiring; `make check-ci-jobs` ok after it).
- Found by running, fixed: a process starts on a 64 KiB stack and CPython's overflow guard keeps 32 KiB, so the first `import` gave `RecursionError` (patch 0004); `os.set_blocking` failed with `ENOTTY` (ioctl `FIONBIO`: configure without `<sys/ioctl.h>`);
  the shell hinted about `/pyvenv.cfg` (patch 0005); `ssl.create_default_context()` refuses the test PKI's certificates (no Authority Key Identifier, `VERIFY_X509_STRICT`): the test client clears that flag alone; `regrtest` needs `--tempdir` below `/tmp` (the program is not given a `stat` of `/`).
- In the guest, one run of each part (by hand, in a harness with the same disk and script lines; the full run is the last item): `python3 -V`; `-c`; exit statuses; `smoke.py` (flags `1 True 1`, `sqlite3` 3.54.1, `ssl` OpenSSL 3.5.9, `asyncio`, a thread, `fork`); `grants python3 ...` with and without `+net`;
  the `asyncio` HTTPS client: `HTTP/1.1 200`, the page; an expired certificate refused ("certificate has expired"), a wrong-name one refused ("Hostname mismatch"), the server's log showing `ended after 0 request bytes` for both; `regr.py`: 17 modules, 3,114 tests, 51 skipped, `Result: SUCCESS`, 31 s.
- `make test-python` end to end: `PASS: 55 steps`, the server's log shows 0 request bytes for both refused certificates, `zoraya-fsck` clean (HVF; ~10 minutes with the build).
- Not run: TCG; CI (no candidate tag, as told); `make test-changed`; the independent review (opus-review).

### Limits found (recorded in `docs/design.md`)
- A program that Python starts has no random connection (ADR-0310, the same as under `dash`): another `python3` fails at start. Nine test cases of the listed modules start interpreters and are left out (`tests/python/regr-ignore.txt`, each with its reason).
- `test_threading` (CPython's) ran 8.5 minutes and failed at thread creation with `Out of memory` (10 errors, 42 failures); not fixed, not in the subset; `test_thread`, `test_threading_local` and `test_queue` pass. A very large thread stack gives `MemoryError`.
- No `socketpair`, `AF_UNIX`, server sockets, `posix_spawn`, shared modules, `ctypes`.
- The shell prints hints for `/dev/urandom` and `/system/tls/roots` after a script that touches them without the grants.

### Decisions (taken without asking, as told)
Patch 0004 (the thread) over a larger initial stack in the kernel (an ABI constant for every process); no weakening of Python's hash randomization for a child with no random connection (the test cases are left out instead);
no `-E`-style environment variable honoured; `test_threading` out of the subset; the client test clears `VERIFY_X509_STRICT` and not the test PKI's certificates (shared with other steps).

### For the independent review (opus-review)
Patch 0001 (`_ssl` BIO) and 0004 (the thread and the exit path: `exit()` from a non-main thread); the shims in `pyshim.c` (`dirfd`, `gethostbyname`, `execv`); the surface block of ADR-0320 (170 names added to `libc.a`);
the environment policy (patch 0002) as a security boundary; `tools/run-disk.sh`'s colon-separated roots and the key; the `test_threading` out-of-memory finding.
