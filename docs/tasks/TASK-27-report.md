# TASK-27 report: stage 9, applications

Running report, updated after each part. Part 9b's design was written by a Sonnet session on branch `work/9b-design`
(reading, ADRs and plan only: nothing was built, no test was run; `make adr-check` and `make check` are the only commands run).

## Status for the architect (2026-11-13, `origin/main` at `a89755b`)

Sources: `git log --first-parent origin/main`, `git tag -l 'stage-9*'`, `docs/adr/index.md` on `main` and on each branch, the
sections of this report (on `main`, and on `work/9c` and `work/9d` for their parts) and the CI runs named. Nothing here was
re-run for this block. **To be updated when 9c, 9d and `work/tls-options` merge.**

### Parts

| Part | State | Tag |
|---|---|---|
| **9b POSIX threads** | **merged.** Design `e4ff190` (ADR-0300 to ADR-0309); kernel step `02d32e5` (calls 56 to 61, futex, the thread object, the `threads` quota; ADR-0313); `libposix` threads `a0cfc1f` (musl's pthreads over the shim, long waits outside the lock, per-thread signals, deferred cancellation, `fork` and `exec` with siblings, `make test-posix-suites`; ADR-0314, ADR-0315); record locks and SQLite's `threadtest3` `936db09` (ADR-0319). Sections "9b ..." below. | `stage-9b` = `936db09` |
| **9a TLS for C, zlib, curl, wget** | **merged** `da73e69`: OpenSSL 3.5.9 (with threads), zlib 1.3.1, curl 8.23.0, wget 1.25.0 built by `zcc` (`make apps`), `getrandom` in `libposix`, the `names` key, `/system/tls` for a POSIX program with the network, `EXTRA_ROOT` (`tools/run-disk.sh`); the per-launch leak fixed by `work/9a-leak` (`a9f0cfe`, merged into `work/9a`). Sections "9a ..." below. | `stage-9a` = `da73e69` |
| 9c CPython | **in progress** on `work/9c` (head `74d33e7`). Its section: `make python` (CPython 3.15.2, static, five patches), `make test-python` `PASS: 55 steps` under HVF, the `regrtest` subset 3,114 tests `Result: SUCCESS`; ADR-0320 `proposed`; not run: TCG, the independent review, CI. `candidate/work-9c-a` (`6ccd7e2`, run 43457392275) failed in the build job (the host Python's `ensurepip` needs `zlib`); `74d33e7` configures the host Python `--without-ensurepip`; no candidate run of it yet. | |
| 9d NetSurf | **in progress** on `work/9d` (head `0bf5f7a`). Its section: NetSurf 3.11 in a window of its own (`window` line, a client channel from the panel, `zoraya-surface`), `make test-netsurf` passes locally under HVF (HTTP page with CSS, image and form; HTTPS; expired certificate and Proceed with 0 request bytes); review findings fixed (`eba8400`, `454af63`, `bdfd4d9`, `95decc5`, `57a4b55`, `335a68d`); ADR-0317 and ADR-0318 accepted (recorded on the branch, `a58cbe0`). `candidate/work-9d-a` (`a58cbe0`, run 43455266029) failed in the build job (`netsurf-libs.stamp`); no candidate run of the later heads yet. | |
| `work/tls-options` | **in progress** (head `8563ca3`): curl and wget patches refuse every option that turns off, replaces or weakens TLS verification or the roots (curl 23 options by `C_...` code, wget five settings), cases in `tests/shell/apps.script`; ADR-0321 `proposed`. No report section and no candidate run yet. | |

Stage-8 follow-ups merged after the stage-8 full run (`722cb6e`), during stage 9: `work/hand-run` `65b2837` (`make run-net` on a
stale disk, `make test-hand-run`, `ping HOST` with names and an echo-only grant (ADR-0330 `proposed`), the real roots check,
`make run-gui` with the network); `work/first-lookup` `ca2c6b4` (the first lookup after boot no longer waits for a missed wake-up;
ADR-0316 `proposed`); `work/tcg-time` `c1e1b34` (`QEMU_SLOW_TIMEOUT` 420 s); `work/cpu-flake` `62f4ad0` (`t_cpu`'s masked-time limit
off HVF); `5a506a1` (the Makefile's `.PHONY` line); `work/ci-docs` `c4d9f4c` (a push to `main` with only `docs/` changes runs the
document checks only). `work/ci-arm` is not merged (section "CI experiment" below).

Left from 9b (ADR-0315's open questions and the sections below): the checks of INV-0306-3 and INV-0306-5, INV-0308-3 over
fresh logs, the mapping-count measurement of a 128-thread process, `check-cancel-points` (INV-0305-3), the SHA-256 / AES-GCM /
`RAND_bytes` half of ADR-0309 item 4 (`tests/posix/ossl_threads.c`), `accept` in a thread.

### ADRs of stage 9 (and the two proposed follow-ups of stage 8)

| ADR | Title (short) | Status | Where | Recommendation (for the proposed ones; not a decision) |
|---|---|---|---|---|
| 0300 to 0309 | Threads: kernel object, futex, pthreads, TLS, signals, cancellation, fork/exec, quotas, tier list, tests | accepted | `main` | |
| [0310](../adr/0310-surface-for-tls-curl-and-wget.md) | TLS, curl and wget as ports | accepted | `main` | |
| [0311](../adr/0311-url-file-names-and-the-roots-directory-for-posix-programs.md) | The `names` key and the roots directory | accepted | `main` | |
| [0312](../adr/0312-openssl-for-the-posix-side-of-tls.md) | OpenSSL 3 for the POSIX side of TLS | accepted | `main` | |
| [0313](../adr/0313-thread-kernel-as-built.md) | Threads in the kernel as built | accepted (items 6 to 8 confirmed) | `main` | |
| [0314](../adr/0314-pthreads-first-slice-as-built.md) | `pthread_*`, first slice as built | accepted | `main` | |
| [0315](../adr/0315-pthreads-waits-signals-cancel-as-built.md) | `pthread_*`, second slice as built | accepted | `main` | |
| [0316](../adr/0316-netd-answers-every-lookup.md) | `netd` answers every lookup | proposed | `main` | Accept: it records what `ca2c6b4` merged and `make test-net-lookup` tests. |
| 0317 | The window of a POSIX program | accepted | `work/9d` | |
| 0318 | NetSurf: its libraries, its own fonts, the source set | accepted | `work/9d` | |
| [0319](../adr/0319-advisory-record-locks-in-libposix.md) | Advisory record locks in `libposix`, in the process | accepted | `main` | |
| 0320 | CPython 3.15 as a port | proposed | `work/9c` | Decide after the independent review and a green candidate; the choices to look at are patch 0004 (the interpreter on an 8 MiB thread instead of a larger first stack for every process), the 170 names added to `libc.a`, and `test_threading` left out (out of memory). |
| 0321 | curl and wget refuse the options that weaken TLS | proposed | `work/tls-options` | Accept Option C: it makes ADR-0228 item 3 and INV-0312-1 hold in the programs, so a line that matches no description cannot pass `-k`. |
| [0330](../adr/0330-ping-command-and-host-operand.md) | The `ping` command and the `host` operand | proposed | `main` | Accept: it records what `65b2837` merged. |

### Decided by the architect (2026-11-13)

- ADR-0300 to ADR-0309 accepted as recommended (`e4ff190`); ADR-0308 supersedes ADR-0100's "no threads" and the other sentences it lists.
- ADR-0312, ADR-0313 (items 1 to 5), ADR-0314 and ADR-0315 accepted.
- `2c10246`: ADR-0310 (Option A), ADR-0311 (Option A, `cwd none`; `cwd read` refused) and ADR-0319 (Option A) accepted; ADR-0313 items
  6 to 8 confirmed; **ADR-0005 superseded in part by ADR-0319** (item 6 and the row for `fcntl` record locks; `flock` stays excluded).
  Follow-ups in `docs/backlog.md`: `getrandom` for programs `dash` or `make` start, record locks shared across processes in `fsd`,
  a wheel event in the compositor protocol, the `boot-tcg-smp4-gicv3` flake.
- ADR-0317 (Option C, no wheel) and ADR-0318 (Option B) accepted; recorded on `work/9d` (`a58cbe0`), they reach `main` with 9d. 9d
  merges only after the review's must-fix findings are fixed and checked.
- The arm64 runner experiment: result accepted, nothing moves (section "CI experiment" below).
- ADR-0321's rule (the refusal belongs in the programs, not only in the descriptions): decided, per ADR-0321's context; the ADR itself is `proposed`.

### Items for the architect

1. **ADR-0316, ADR-0320, ADR-0321, ADR-0330 are `proposed`** (table above).
2. **wget is GPL-3.0-or-later** (section "Status for the architect (9a)", item 2): a separate program in the image, as GNU make
   (ADR-0109). No decision is recorded; confirm.
3. **The `libc-test` fetch is pinned by git commit**, not by a release (same section, item 5): no decision is recorded.
4. **Candidates:** 9c and 9d each had one candidate run, both red in the build job; neither current head has a candidate run.
   `work/tls-options` has none. Merge order and a `candidate-full/` run before `stage-9c`/`stage-9d`: to be updated when they merge.
5. **The `Makefile` comment above `RUN_NET_CMD`** still gives the 8.5 MiB per-launch leak as the reason for 512 MiB; the leak is fixed
   (`a9f0cfe`; `apps-launches` runs 16 launches on 128 MiB). Whether the run machines go back to less memory is open.

### Manual checks for the architect

- **9b, 9a:** none named in their sections (`make test-hand-run` covers the production images with `curl` and `wget`).
- **9d (on `work/9d` until it merges):** build with `make netsurf`, start `make run-gui` (or `make run-net`), open a terminal from the
  desktop (room `home`, which has the network) and run `netsurf https://example.org/`, `netsurf http://example.com/`,
  `netsurf https://www.wikipedia.org/` (images from other hosts stay missing unless the line has `+net upload.wikimedia.org`), and
  `netsurf` alone for the welcome page.
- **9c:** none named in its section. To be updated when 9c merges.

### Measurements known

- **CI timing.** The whole-suite TCG runs (128 tests): 168 s to 267 s in green runs, one failure at the old 300 s limit on a slow
  runner; `QEMU_SLOW_TIMEOUT` is 420 s (section "CI timing" below). Locally under HVF the suite took about 160 s, `t_thread` 21 s of it.
- **CI on `main`.** The full set: 65 jobs, all green, 27.5 minutes (`c4d9f4c`, run 43452400389). The first docs-only push
  (`a89755b`, run 43458056319): 20 jobs green and 2 skipped, about one minute.
- **arm64 runners** (run 43447464141): no KVM; arm TCG 1.4 to 1.7 times slower than x86 TCG (whole job 45:44 against 30:54).
- **Test counts.** `make test-posix-suites`: 13 libc-test thread tests; `make test-posix-threadtest3`: 7 scenarios, HVF and TCG;
  `make test-apps`: 235 steps (HVF, after the 9a review); `t_threads`: 17 cases at the second slice. On `work/9c`: `make test-python`
  55 steps, `regrtest` 3,114 tests. `docs/adr/invariants.md` Total on `main`: 618.

### Time and model per part

Not measured as session time. From the commits' `Co-Authored-By` lines and dates (local time): 9b design Sonnet 5.5, 2026-11-12
14:22 to 11-13 04:05; 9b kernel Sonnet 5.5, 11-13 06:02 to 08:17; 9b `libposix` Sonnet 5.5 (2 commits Opus 5.5), 06:44 to 12:06;
9b record locks Sonnet 5.5, 14:06 to 14:25; 9a Sonnet 5.5 (1 commit Opus 5.5), 04:41 to 13:17; 9c Sonnet 5.5, 15:28 to 17:07 (open);
9d Sonnet 5.5, 14:03 to 17:02 (open); `work/tls-options` Sonnet 5.5, 17:04 to 17:07 (open).

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

## 9d: NetSurf, step 1 (branch `work/9d`, 2026-11-13, in progress)

Done: proposed ADR-0317 (a POSIX program's window: a `window` line in the description, a client channel from the panel
through the shell, `zoraya-surface` as a static library speaking `zoraya-proto-gui`, `libnsfb` surface in C over five
functions) and ADR-0318 (NetSurf 3.11 with its own bitmap font for 9d, `zoraya-text` later; the source set). Pinned in
`ports/sources.lock`, fetched and verified by `make fetch-ports`: netsurf-all 3.11, libpng 1.6.50, IJG libjpeg 9f,
pkg-config 0.29.2. `make check` passed.

## 9d: NetSurf, steps 1b and 2 (branch `work/9d`, 2026-11-13)

Done and run on the Mac (`make check` passes; no QEMU step was run):
- `make netsurf` (`ports/Makefile`, top `Makefile` forwards): libpng 1.6.50, libjpeg 9f, libcurl installed into `build/ports/deps`,
  NetSurf's libraries (`buildsystem libnslog libwapcaplet libparserutils libcss libhubbub libdom libnsbmp libnsgif libnsutils
  libutf8proc libnspsl libnsfb`), then `netsurf` (framebuffer front end, internal font, curl + OpenSSL from 9a) linked by `zcc`;
  `build/rootfs-apps/bin/netsurf` (12.5 MB, `zoraya-elf-check` ok) and `build/rootfs-apps/share/netsurf/` (resources). Host
  tools built into `build/ports/host` only when needed: pkg-config 0.29.2, bison 3.8.2 (macOS has 2.3; pinned in
  `ports/sources.lock`), zlib and libpng (for `convert_image`). The CI image (`.github/ci/Dockerfile`) gains gperf, flex,
  bison. Decisions recorded in ADR-0318 item 5: no JavaScript (so no `nsgenbind`), no SVG (needs expat), `ports/apps/nsshim.c`
  for names `libc.a` lacks (atol, strncasecmp, modf, iconv*, dirfd, scandir), libnslog patch for old bison.
- `crates/zoraya-platform` (the `Platform` trait, re-exported by `zoraya-app`), `crates/zoraya-surface` (`Surface<P>`: open,
  pixels, damage, commit, event, set_title, close; 15 host tests with a fake compositor), `zoraya-posix/src/ffi/window.rs`
  (five `zoraya_window_*` calls, names added to `libc.a` by a block in ADR-0317; `libc.a` builds and `check-libc` passes),
  `EXT_WINDOW = 4` in `ZPBX` (`zoraya-abi`, test updated) and `Posix::window`, taken from the bootstrap; `ports/netsurf/zoraya.c`
  and `zoraya_window.h` (the `libnsfb` surface `zoraya`, key and button maps) added to the unpacked tree by a patch of
  `libnsfb.h` and its `surface/Makefile`; `netsurf` links with them. ADR-0317 item 7 records the answers to its two open
  questions (rlib into libposix, one allocator; the `ZPBX` flag).

Not done, in order (nothing below exists yet):
1. Step 3 of the list: `window TITLE` key in `zoraya-describe`; the panel's request (a client channel in the shell's room,
   pills of the line's grants, refused in an untrusted room, with a test); `zosh`'s plan passing the channel as the `EXT_WINDOW`
   handle of `ZPBX` (`user/src/posix_child.rs` line ~500 is where `EXT_RANDOM` is added); `user/describe/posix/netsurf.cmd`
   (`+net` from the URL, `window`, `+r /share/netsurf`, download directory through the chooser); host tests for each piece.
2. Run `netsurf` under QEMU: whether `libnsfb`'s constructor registration of the `zoraya` surface runs under musl's start
   (if not, call `zoraya_register_surface` explicitly); a first window on a test page; a GUI script and `make test-netsurf`
   (Makefile `TEST_STEPS`, `tools/ci-tests.txt`, `tools/test-map.txt`, `tools/plan-lib.sh`, workflow timeout); HTTPS, a form,
   downloads; screens under `docs/`; the real-site commands for the architect.
3. `make test-libposix`, `test-posix` and `test-apps` were not run after `libc.a` gained five names and `Posix` a field.
Known limits: no wheel events (the protocol has none), no NetSurf cursor shape, timed waits are a 2 ms poll loop, SVG is not shown,
Cyrillic text shows placeholders (ADR-0318).

## 9d: NetSurf, window, gate, review fixes and `make test-netsurf` (branch `work/9d`, 2026-11-13)

Done, and run on the Mac under HVF (one QEMU at a time):
- `window TITLE` in `zoraya-describe` (once, at most 48 characters, command only; 1 test). `user/describe/posix/netsurf.cmd`
  (`window NetSurf`, optional `URL` operand of kind `url` so `+net` comes from it as for curl, `-v`).
- The request: `OpenWindow`/`WindowOpened`/`WindowRefused` in `zoraya-proto-chooser` (ordinals 38, 39); `zoraya_panel::window_record`
  refuses an untrusted room and malformed requests (2 host tests), a trusted shell's terminal gets the channel at slot 14
  (`WINDOW_SLOT`; slots 9 to 13 are the shell's network, random, keeper and services); `init` gives its first shell one too
  (via the panel, slot 10 of the panel). The panel adds the client with `AddRoomClient` (room's record, pills from the line),
  at most 8 windows a shell. `zosh` asks (`posix_window`), refuses in an untrusted room, and `posix_child` moves the
  client end as `EXT_WINDOW`. `tools/check-untrusted-gate.sh` requires one `untrusted-gate: window-refusal` marker in the shell
  and one in the panel (it fails if either is gone; verified by removing one). The gate itself stays closed: the shell's
  refusal is not reachable in QEMU while an untrusted room cannot be entered; the panel's is tested on the host.
- Review of the window code (opus-review): W1 (fork child forgets the window, no close or unmap; test of `forget_taken`),
  W2 (open size fitted to the display's logical size; test), W3 (checked buffer sizes, cap 16384 per side and 64 Mi pixels in
  `Surface` and `Gui::buffer`; test, found a real 2^60-pixel allocation in `Configure`), W4 (one receive buffer), W5 (modifier
  releases on focus loss, unshifted layout text for main-block keys; C, checked only by the build and the GUI run), W6 (INV-0317-1 to 3
  rows and check texts), N1 (`Drop` closes the channel), N2 (a re-entrant or concurrent window call gets -1/0, no deadlock; documented),
  N3 (ADR wording), N4 (nsshim comment, `modf` sign), N6 (tests: overflowing `Configure`, resize with a frame in flight, damage after
  resize). Not done: none of the list; N5 was not in my message.
- Running it: the surface constructors of libnsfb **do** run under musl's start (the window opens), the open question is closed.
  Three failures found and fixed: libcurl is built without cookies and proxies, so NetSurf's `CURLOPT_COOKIE`/`PROXY` failed every fetch
  (patch 0003: ignore the unknown-option error; consequence: no cookies, no proxy); libposix maps no file, so NetSurf's file fetcher
  (its own CSS) failed and looped on its error page (patch 0004: read the file); NetSurf's "Proceed" button on the privacy error page
  turned TLS verification off (patch 0005: never; INV-0312-1). Resources now live in `/usr/share/netsurf` (the toolchain set reads
  `/usr`; no extra grant).
- `make test-netsurf` (Makefile, `tests/gui/netsurf.script`, pages `/ns/page`, `/ns/style.css`, `/ns/green.png`, `/ns/result` in
  `tools/test-httpd`, which also logs each `/ns/` request): the page's blue block (CSS), green block (PNG) and the form result (yellow), by
  colour regions; the window's pills (`+r /system/tls`, `+net HOST:PORT`) are in the screens; HTTPS to `test.zoraya`; the expired
  certificate gives NetSurf's privacy error page, and after a click on Proceed the same page and 0 request bytes at the server
  (log check as in test-apps). Wired into TEST_STEPS, `tools/ci-tests.txt`, `plan-lib.sh`, `tools/test-map.txt`; `ci-build` builds
  netsurf; the build job's timeout is 45 minutes. Screens: `docs/netsurf/netsurf-{http,result,https,refused,refused-again}.png`.
- Ran after `libc.a` gained names: `make test-libposix`, `make test-posix` (hello, smoke, invariants; auto), `make test-apps` (all passed);
  `make check` passes. Not run: TCG variants, `make test-gui`, `make test-hand-run`, the candidate runs, the full `make test`.

Not done / limits (stated, not hidden):
- Recording the architect's acceptance in ADR-0317/0318 (status and "Architect's decision") and `docs/adr/index.md`: my attempt was refused by the
  permission check (the instruction reached me through the coordinator, not the architect), so both ADRs are still `proposed` in the files. The wheel
  contradiction in ADR-0317 item 4 is fixed. The architect or the user should record the two decisions.
- Downloads through the chooser: not done (needs a second `ZPBX` slot for a chooser channel); NetSurf has no current directory and no download directory.
- The window channel exists for the home terminal (init's first shell) and shells the panel launches; a shell that `init` restarts gets none until
  the panel is restarted too. A page's links to other hosts are refused by the network unless the line has `+net HOST` words (limit of the `url` operand).
- No wheel, cursor shape, clipboard, JavaScript, SVG; cookies and proxy are off (libcurl is built without them, ADR-0310); the title text is Latin only.
- INV-0317-1's status says `checked` for the panel's side and the describe key; the shell's refusal is checked by the marker script only.
- CI: not run (`make test-netsurf` on the x86 runner: the pages' coordinates were taken at scale 1 on HVF; TCG timing may need longer pauses).

For the architect's manual check of real sites (a terminal from the desktop, home, which has the network; the QEMU run with `make run-gui`
or `make run-net` shows the dock): `netsurf https://example.org/`, `netsurf http://example.com/`, `netsurf https://www.wikipedia.org/` (a page that
links to other hosts will show their images missing: add `+net upload.wikimedia.org` words to the line), and `netsurf` alone for the welcome page.

## 9e: the wheel, 32 compositor clients, late window answers (branch `work/9e`, 2026-11-13)

Done (ADRs proposed: ADR-0322, ADR-0323; notes in ADR-0262 and ADR-0317's open questions):
- **Wheel (ADR-0322).** `zoraya-protocol` takes `i32`; `zoraya-proto-gui` `Wheel` (ordinal 46, a client event: window, logical place,
  `dx`/`dy` in 120ths of a notch, `dy` > 0 toward the person, at most 64 notches, modifiers to the focused client only);
  `zoraya-proto-input` record kind `WHEEL` (`Below<3>`); `zoraya-virtio-input` takes `REL_WHEEL`/`REL_HWHEEL` (times 120, the
  vertical sign turned) or their `HI_RES` axes when declared (the notches then skipped); it dropped every `EV_REL` before. The
  compositor sums a frame's turns and sends one `Wheel` to the body under the pointer (rules of `WindowPointerMods`: no focus
  change; nothing during a move, drag or the switcher, under a sheet, to a guarded sheet or a legacy surface). `zoraya-surface`
  `Event::Wheel` (a `Move` first), `zoraya_window_poll` kind 8 (`ZW_WHEEL`), NetSurf's libnsfb surface presses mouse 5/4 per
  whole notch (NetSurf scrolls 100 px; no horizontal button in libnsfb); the terminal scrolls three lines a notch
  (`zoraya_term::WheelLines`). `zoraya-app` ignores it (backlog). The runner's GUI step `wheel X Y up|down [N]` (QMP
  `wheel-up`/`wheel-down`). Measured: QEMU 11.1.2's virtio tablet delivers them as `REL_WHEEL` (the NetSurf step passes).
- **Clients (ADR-0323).** `MAX_CLIENTS` 8 -> 32 (= `MAX_WINDOWS`; a client's record is about 1.5 KiB, the windows' copies
  stay the memory bound); in `zoraya-proto-gui`: `ROOM_CLIENTS` 3, `UNTRUSTED_CLIENTS` 12, `SESSION_CLIENTS` 20. An untrusted
  room's `AddRoomClient` past its account is `OVER_QUOTA`, counted as the room's refusal, `ClientGone` for its tag. The
  panel's `OpenWindows` counts the session's clients apart from untrusted rooms' launches and keeps `RESERVED_CLIENTS` (3)
  out of `SESSION_CLIENTS`. An untrusted client's toplevels were already held to its account's 3 surfaces (`OVER_QUOTA`
  before `MAX_TOPLEVELS`); unchanged.
- **Late answers.** `zosh` subscribes the panel's window channel at its prompt and closes any answer that arrives after
  `windowclient::ask` gave up (`drain_stale`), so its client end no longer holds the compositor's client and the panel's
  count until the next request.
- Tests: host tests in `zoraya-proto-gui`, `-proto-input`, `-virtio-input`, `-compositor` (`tests_wheel.rs`, client
  accounts in `tests_usage.rs`), `-surface`, `-term`, `-panel`, `zoraya-test-runner`; `tests/gui/terminal.script` (wheel
  through the scrollback), `tests/gui/windows.script` (nine more terminals, eleven clients, the tenth takes keys),
  `tests/gui/netsurf.script` (`/ns/tall` scrolled 15 notches down and back).

Ran on the Mac (HVF, one QEMU at a time): `make check`; host tests of the ten touched crates (533 passed); `cd fuzz && cargo
+nightly-2026-11-12 check --bins`; `make test-gui GUI_RUNS="terminal:auto:1 windows:auto:1"` (79 and 61 steps; the first
windows run failed at `poweroff` typed into a panel terminal, which has no power: the script now returns to init's
terminal); `make netsurf` and `make test-netsurf` (72 steps, also after steps 3 and 4); `make test-hand-run` (all three
runs). Not run: TCG and scale-2 GUI runs, the fuzzers themselves, `make test`, any candidate run (none tagged).
Limits: the wheel reaches no `zoraya-app` program (file manager, editor, settings); NetSurf takes no horizontal turn; the
session's 20 clients are not carved (a session of 32 clients leaves untrusted rooms none, as for windows, ADR-0262).

Review fixes (opus-review of 9e): E1 `t_gui`'s flood opened 4 clients in one untrusted room, past the new `ROOM_CLIENTS`
(3), and the fourth create panicked; it now opens 3, checks that a fourth is refused (its end closed), and asks one client
for 4 toplevels (3 admitted, 1 `OVER_QUOTA`); another client of the room is refused too. E2 `tools/test-map.txt`: the
compositor's, the GUI, input and display protocols' and the virtio GPU and input crates also run `test-qemu`. W3
`tests_wheel.rs`: a test for each early return of `send_wheel` (a sheet over the window, a sheet in its guard, a legacy
surface, a carried entry; each fails with its return deleted, checked by hand once) and for overlapping bodies. W4 a
refusal for a room not yet listed forgets the refusals of rooms without a client (the list is at most the untrusted
clients plus one); the test checks the other clients and accounts are unchanged. N5 ADR-0323's open question names the
deviation from ADR-0262's four accounts. N6 `terminal.script` checks the view back at the prompt after the wheel turns
down; `windows.script` drags the tenth window against the bar before reading it, since its cascade place differs at
scale 2 (the scale-2 run failed there before). Ran: `make check`; `cargo test -p zoraya-compositor -p zoraya-proto-gui
-p zoraya-panel` (235 passed); the fuzz crate's `check --bins`; `make test-qemu-auto-default` (`t_gui: all passed`;
`make test-qemu-one` has no GPU and skips `t_gui`); `make test-gui` for `terminal:auto:1` (82 steps), `windows:auto:1`
and `windows:auto:2` (62 steps each).

## CI experiment: GitHub's free arm64 runners (2026-11-13, branch `work/ci-arm`, not merged)

Result accepted by the architect: the `ubuntu-24.04-arm` runner has no KVM (`/dev/kvm` missing; `-accel kvm` exits 1 with
"failed to initialize kvm"), and arm TCG is 1.4 to 1.7 times slower than x86 TCG for the four QEMU steps measured
(`make test-qemu-auto-default` 3:13 against 4:48, `make test-qemu-tcg-smp4-gicv3` 3:44 against 5:54, `make test-net` 8:32
against 12:15, the `threads.script` posix job 8:34 against 14:55; whole job 30:54 against 45:44; one run each, cold
caches, run 43447464141). Decision: move nothing; `work/ci-arm` is not merged and stays as the branch for a later KVM
probe (its workflow `ci-arm.yml` runs on pushes to that branch and by hand). The details are in
`docs/research/ci-arm.md` on that branch.

