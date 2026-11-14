# TASK-27 report: stage 9, applications

Running report, updated after each part. Part 9b's design was written by a Sonnet session on branch `work/9b-design`
(reading, ADRs and plan only: nothing was built, no test was run; `make adr-check` and `make check` are the only commands run).

## Status for the architect (2026-11-13, stage 9 complete, `origin/main` at `7adf938`)

Sources: `git log --first-parent origin/main`, `git ls-remote --tags origin`, `docs/adr/index.md` on `main` at `7adf938`, the part
sections of this report, the commits' `Co-Authored-By` lines and the CI runs named (`gh run view`). Nothing was re-run for this block.

**Stage 9 is done.** Tags: `stage-9a` = `da73e69`, `stage-9b` = `936db09`, `stage-9c` = `stage-9d` = `stage-9` = `7adf938`. CI on
`main` at `7adf938` (run 43511821485, a push to `main`, so the full set: 69 jobs) is green on its second attempt; the first attempt's
`build` job failed in `make fetch-ports`, which timed out five times on `https://ftp.gnu.org/gnu/make/make-4.4.1.tar.gz` and then on
its `ftpmirror.gnu.org` fallback (curl exit 28, connect timeout; a network flake, not the change; `docs/backlog.md`, "CI downloads").
The runs of the three tags (43517591619, 43517596370, 43517601368, 23 jobs each) are green. No `candidate-full/` tag was pushed for
stage 9: the full set ran as `main`'s push run of the same commit.

### Parts merged

| Part | Merge on `main` | Branch head (green candidate run) | Tag | What |
|---|---|---|---|---|
| 9b design | `e4ff190` | | | ADR-0300 to ADR-0309 (threads: kernel object, futex, pthreads, TLS, signals, cancel, fork/exec, quotas, tier list, tests). |
| 9b kernel | `02d32e5` | | | Calls 56 to 61, futex, the thread object, the `threads` quota; ADR-0313. |
| 9b `libposix` threads | `a0cfc1f` | | | musl's pthreads over the shim, long waits outside the lock, per-thread signals, deferred cancellation, `fork`/`exec` with siblings, `make test-posix-suites`, `check-mutant-lists`; ADR-0314, ADR-0315. |
| 9a TLS for C, zlib, curl, wget | `da73e69` | `0867b1a` | `stage-9a` | OpenSSL 3.5.9 (threads), zlib 1.3.1, curl 8.23.0, wget 1.25.0 by `zcc` (`make apps`), `getrandom`, the `names` key, `/system/tls` with the network, `EXTRA_ROOT`; ADR-0310 to ADR-0312. |
| 9b record locks, `threadtest3` | `936db09` | `495b14f` | `stage-9b` | `fcntl` record locks in the process (ADR-0319), SQLite `threadtest3` (7 scenarios) in CI. |
| 9d NetSurf | `65b1522` | `616fe29` (43465681384) | `stage-9d` (= `7adf938`) | NetSurf 3.11 in a window of its own: the `window` line, a client channel from the panel, `zoraya-surface`, the `libnsfb` surface, `ClientGone`; `make test-netsurf`; ADR-0317, ADR-0318. |
| `work/tls-options` | `f90d29f` | `5b52463` (43497477629) | | curl and wget refuse every option that disables, replaces or weakens TLS verification or the roots (patches `curl/0003`, `wget/0004`, `tests/shell/apps-refusals.script`); ADR-0321. |
| 9e wheel and clients | `dd575e3` | `ea11ae6` (43498404799) | | A `Wheel` event in the compositor protocol (NetSurf and the terminal scroll), 32 compositor clients counted against the room's account, late window answers closed; ADR-0322, ADR-0323. |
| 9c CPython | `7adf938` (`main` fast-forwarded to `work/9c`'s merge of `main`) | `7adf938` (43505870979) | `stage-9c`, `stage-9` | CPython 3.15.2 static by `zcc`, six patches, `make test-python` and `make test-python-regr` in CI, `make test-python-long` by hand; ADR-0320. |

Stage-8 follow-ups and CI changes merged during stage 9: `work/hand-run` `65b2837` (`make run-net` on a stale disk, `make
test-hand-run`, `ping HOST` with names and an echo-only grant, ADR-0330; the real roots check; `make run-gui` with the network);
`work/first-lookup` `ca2c6b4` (the first lookup after boot no longer waits for a missed wake-up, lookup timeouts answered, ARP probes
for the gateway and DNS; ADR-0316); `work/tcg-time` `c1e1b34` (`QEMU_SLOW_TIMEOUT` 420 s); `work/cpu-flake` `62f4ad0` (`t_cpu`'s
masked-time limit off HVF); `5a506a1` (the Makefile's `.PHONY` line); `work/ci-docs` `c4d9f4c` (a push to `main` with only `docs/`
changes runs the document checks only). Documents: `972bbf0` (TASK-27), `2c10246` (the architect's decisions), `a89755b` and
`7f28bb1` (this report). `work/ci-arm` is not merged (section "CI experiment").

### ADRs of stage 9 (and the proposed follow-ups of stage 8), status from `docs/adr/index.md` at `7adf938`

| ADR | Title (short) | Status | Recommendation (for the proposed ones; not a decision) |
|---|---|---|---|
| 0300 to 0309 | Threads: kernel object, futex, pthreads, TLS, signals, cancellation, fork/exec, quotas, tier list, tests | accepted | |
| [0310](../adr/0310-surface-for-tls-curl-and-wget.md) | TLS, curl and wget as ports | accepted | |
| [0311](../adr/0311-url-file-names-and-the-roots-directory-for-posix-programs.md) | The `names` key and the roots directory | accepted | |
| [0312](../adr/0312-openssl-for-the-posix-side-of-tls.md) | OpenSSL 3 for the POSIX side of TLS | accepted | |
| [0313](../adr/0313-thread-kernel-as-built.md) | Threads in the kernel as built | accepted | |
| [0314](../adr/0314-pthreads-first-slice-as-built.md), [0315](../adr/0315-pthreads-waits-signals-cancel-as-built.md) | `pthread_*` as built, two slices | accepted | |
| [0316](../adr/0316-netd-answers-every-lookup.md) | `netd` answers every lookup | **proposed** | Accept Option B: it records what `ca2c6b4` merged and `make test-net-lookup` tests. |
| [0317](../adr/0317-the-window-of-a-posix-program.md) | The window of a POSIX program | accepted | |
| [0318](../adr/0318-netsurf-fonts-and-the-source-set.md) | NetSurf: its libraries, its own fonts, the source set | accepted | |
| [0319](../adr/0319-advisory-record-locks-in-libposix.md) | Advisory record locks in `libposix`, in the process | accepted | |
| [0320](../adr/0320-cpython-port.md) | CPython 3.15 as a port | **proposed** | Accept Option A as merged (green candidate and full run); decide item 10 (below) with it. The index row still says "five patches"; there are six. |
| [0321](../adr/0321-curl-and-wget-refuse-tls-weakening-options.md) | curl and wget refuse the options that weaken TLS | **proposed** | Accept Option C: it makes ADR-0228 item 3 and INV-0312-1 hold in the programs, so no line passes `-k`. |
| [0322](../adr/0322-a-wheel-event-in-the-compositor-protocol.md) | A wheel event in the compositor protocol | **proposed** | Accept Option C as built in 9e (120ths of a notch, to the body under the pointer, no focus change). |
| [0323](../adr/0323-the-compositors-clients-counted-against-the-rooms.md) | 32 compositor clients, an untrusted room's counted in its account | **proposed** | Accept Option C, and rule on its open question: client accounts do not count rooms, a deviation from ADR-0262's four accounts. |
| [0330](../adr/0330-ping-command-and-host-operand.md) | The `ping` command and the `host` operand | **proposed** | Accept Option A: it records what `65b2837` merged, which follows the architect's decision already written in it. |
| 0280, 0281, 0285, 0286 | Stage-8 ADRs (frame slots, netd ping/DHCP, CPU ceiling details, N3 fetch) | proposed | Outside stage 9; still waiting (TASK-26). |

### Decided by the architect (2026-11-13)

- ADR-0300 to ADR-0309 accepted as recommended (`e4ff190`); ADR-0308 supersedes ADR-0100's "no threads" and the other sentences it lists.
- ADR-0312, ADR-0313 (items 1 to 5), ADR-0314 and ADR-0315 accepted.
- `2c10246`: ADR-0310 (Option A), ADR-0311 (Option A, `cwd none`) and ADR-0319 (Option A) accepted; ADR-0313 items 6 to 8 confirmed;
  **ADR-0005 superseded in part by ADR-0319** (`flock` stays excluded). Follow-ups in `docs/backlog.md`.
- ADR-0317 (Option C) and ADR-0318 (Option B) accepted; on `main` with 9d.
- The arm64 runner experiment: result accepted, nothing moves.
- ADR-0321's rule (the refusal belongs in the programs, not only in the descriptions): decided; the ADR itself is `proposed`.

### Items for the architect

1. **Six ADRs are `proposed`:** ADR-0316, ADR-0320, ADR-0321, ADR-0322, ADR-0323, ADR-0330 (table above).
2. **INV-0312-1 for a program that turns verification off** (9c re-review W6): the ADR-0312 note states only the fact (Python is not
   mediated; a script can pass `ssl.CERT_NONE`); the proposed reading (the invariant covers connections made with this OpenSSL's
   defaults) is ADR-0320 item 10. Decide it with ADR-0320, or ask for a superseding ADR of ADR-0312.
3. **wget is GPL-3.0-or-later** (section "Status for the architect (9a)", item 2): a separate program in the image, as GNU make
   (ADR-0109). No decision recorded; confirm.
4. **SQLite's source is public domain** (the rule of `docs/dependencies.md` names MIT, Apache-2.0, BSD, ISC and Zlib): listed there as
   W3's source and as a test input (`threadtest3`, item 3 of the 9a status). CPython's `_sqlite3` also links SQLite into
   `bin/python3` (`make python`), which the list does not say. Confirm public domain as a test input and as a part of a program in the image.
5. **The `libc-test` fetch is pinned by git commit** (`ports/sources.lock`, `libctest 63e36e9...`, with the SHA-256 of the archive
   first fetched), not by a release: upstream has none. No decision recorded.
6. **Record locks across processes** (ADR-0319 Option B, a lock table in `fsd`): in `docs/backlog.md`; two processes on one SQLite
   file do not see each other's locks. Schedule it or leave it.
7. **The wheel in the native applications** (proposed ADR-0322): `zoraya-app` drops `Wheel`, so the file manager, editor and
   settings do not scroll with it; in `docs/backlog.md`.
8. **The `Makefile` comment above `RUN_NET_CMD`** still gives the 8.5 MiB per-launch leak as the reason for 512 MiB; the leak is fixed
   (`a9f0cfe`). Whether the run machines go back to less memory is open.
9. **CI downloads:** `make fetch-ports` reaches the Internet on a cache miss; `main`'s CI went red once on `ftp.gnu.org`
   (`docs/backlog.md`, "CI downloads"). A cache of the pinned archives keyed by SHA-256 is the proposed fix.

### Manual checks for the architect

Build the programs once, then start on a fresh disk (the default disk keeps a person's files; `tools/run-disk.sh` makes a new one,
keeping the old as `build/disk.img.old`, only when its key changes):

```
make apps netsurf python
rm -f build/disk.img build/disk.img.key          # a fresh disk: this deletes the files of the old one
make run-gui                                      # or make run-net (the serial console with the Internet)
```

With Python the volume needs more room: `EXTRA_ROOT=build/rootfs-apps:build/rootfs-python DISK_SIZE=192M make run-gui`.
In a terminal from the desktop (room `home`, which has the network), or at the `run-net` prompt:

- `ping google.com` (by name; four replies, then the summary).
- `netsurf https://example.org/`, `netsurf http://example.com/`, `netsurf https://www.wikipedia.org/` (images from other hosts stay
  missing unless the line has `+net upload.wikimedia.org`), `netsurf` alone for the welcome page; scroll with the wheel.
- `python3 -V`; `python3 -c 'import sqlite3, ssl; print(sqlite3.sqlite_version, ssl.OPENSSL_VERSION)'`;
  `+net www.python.org:443 python3 -c 'import urllib.request; print(urllib.request.urlopen("https://www.python.org/").status)'`.
- `+net example.org:443 curl -O https://example.org/index.html` and `curl -k https://example.org/` (refused as a usage mistake).

### Measurements known

- **CI on `main` at `7adf938`** (run 43511821485, attempt 2): 69 jobs green; from the re-run `build` job's start to the last job 32 min
  (`build` 10 min 56 s on a cache hit; the longest test jobs `make test-apps` 13 min, `make test-qemu-tcg-smp4-gicv3` 6 min 46 s,
  `make test-python` 5 min 48 s, `make test-netsurf` 5 min, `make test-python-regr` 3 min 29 s). The full set on `c4d9f4c` was 65
  jobs in 27.5 min. The first docs-only push (`a89755b`, run 43458056319): 20 jobs green and 2 skipped, about one minute.
- **Whole-suite TCG runs** (128 tests): 168 s to 267 s in green runs; `QEMU_SLOW_TIMEOUT` is 420 s (section "CI timing").
- **arm64 runners** (run 43447464141): no KVM; arm TCG 1.4 to 1.7 times slower than x86 TCG (whole job 45:44 against 30:54).
- **Test counts.** `make test-posix-suites` 13 libc-test thread tests; `make test-posix-threadtest3` 7 scenarios; `make test-apps`
  235 steps (after the 9a review); `t_threads` 17 cases; `make test-netsurf` 72 steps; `make test-python` 104 steps and
  `make test-python-regr` 16 modules, 2,949 tests (HVF, the 9c re-review round). `docs/adr/invariants.md` Total: 634.

### Time and model per part

Not measured as session time. From the commits' author dates (local time, 2026-11-13 unless said) and `Co-Authored-By` lines:

| Part | Commits | From, to | Model |
|---|---|---|---|
| 9b design | 8 | 11-12 14:22 to 11-13 04:05 | Sonnet 5.5 (8 more without a trailer) |
| `work/hand-run` | 17 | 01:41 to 03:39 | Sonnet 5.5 16, Opus 5.5 1 |
| 9a | 23 | 04:41 to 13:17 | Sonnet 5.5 22, Opus 5.5 1 |
| 9b kernel | 31 | 06:02 to 08:17 | Sonnet 5.5 |
| 9b `libposix` | 34 | 06:44 to 12:06 | Sonnet 5.5 32, Opus 5.5 2 |
| `work/first-lookup` | 9 | 08:40 to 08:58 | Sonnet 5.5 5, Opus 5.5 4 |
| `work/tcg-time`, `work/cpu-flake` | 1, 1 | 09:07; 10:25 | Sonnet 5.5; Opus 5.5 |
| 9b record locks | 7 | 14:06 to 14:25 | Sonnet 5.5 |
| `work/ci-docs` | 2 | 15:55 to 15:57 | Sonnet 5.5 |
| 9d | 30 | 14:03 to 17:43 | Sonnet 5.5 12, Opus 5.5 4, 14 without a trailer (model not recorded) |
| `work/tls-options` | 9 | 17:04 to 20:56 | Opus 5.5 5, Sonnet 5.5 4 |
| 9e | 16 | 18:30 to 21:02 | Opus 5.5 |
| 9c | 38 | 15:28 to 21:07 | Sonnet 5.5 27, Opus 5.5 11 |

### Known limitations (each in its section, `docs/design.md` or `docs/backlog.md`)

- **Threads (9b):** no checks yet for INV-0306-3 and INV-0306-5, INV-0308-3 over fresh logs, `check-cancel-points` (INV-0305-3);
  the mapping-count measurement of a 128-thread process; `accept` in a thread untested; service-reply waits (`stop_self`, `TCSADRAIN`)
  inside the library lock; a cancelled long wait leaks up to about 32 KiB; record locks are per process.
- **TLS programs (9a, tls-options):** a program `dash` or `make` starts (or Python starts) has no `getrandom` connection, so OpenSSL
  cannot seed; no `tls.roots` setting for C programs; the SHA-256 / AES-GCM / `RAND_bytes` half of ADR-0309 item 4 is not written;
  ADR-0321's option lists are not diffed against a new curl or wget release.
- **NetSurf (9d):** no downloads (no chooser channel), no JavaScript, SVG, cookies, proxy, clipboard, cursor shape or horizontal
  wheel; Latin titles and placeholders for Cyrillic; a shell `init` restarts gets no window channel until the panel restarts; links
  to other hosts need `+net HOST` words; timed waits are a 2 ms poll.
- **CPython (9c):** no `socketpair`, `AF_UNIX`, server sockets, `posix_spawn`, shared modules or `ctypes`; a `fork` copies the whole
  writable space (`test_threading` runs out of memory in `os.fork` and is only in `make test-python-long`, by hand); recursion ends
  in `MemoryError` near 33,000 frames; the archive's Sigstore bundle is not checked.
- **Compositor (9e):** the session's 20 clients are not carved per room; a client with no window keeps its slot; no scale-2 TCG run.
- **CI:** `make fetch-ports` depends on upstream servers on a cache miss.

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

**9b closed (2026-11-13).** Merged in four parts: design `e4ff190`, kernel `02d32e5`, `libposix` `a0cfc1f`, record locks `936db09`;
tag `stage-9b` = `936db09`. ADR-0300 to ADR-0309, ADR-0313 to ADR-0315 and ADR-0319 accepted. What is left is in the status block's
"Known limitations" and `docs/backlog.md` ("Follow-ups from the 9b libposix review", "Stage 9 parts: what the reports left").

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

**9a closed (2026-11-13).** Merged `da73e69` (head `0867b1a`), tag `stage-9a` = `da73e69`; ADR-0310 to ADR-0312 accepted. Items 2,
3 and 5 above are still open (the status block's "Items for the architect"); the option refusals came later with `work/tls-options`.

## 9c: CPython (branch `work/9c`, 2026-11-13)

### What exists
- `ports/sources.lock` pins Python-3.15.2.tar.xz by the SHA-256 python.org publishes (`8b5536d2...797de`); python.org also publishes a
  Sigstore bundle, and checking it is optional and not done (an earlier note here and in the session said python.org published no SHA-256: wrong, corrected on the architect's word).
- `make python` (`ports/Makefile`): host Python 3.15.2 (`build/ports/hostpython`; `build/ports/host` until the merge of `main` with 9d, whose host tools are there), SQLite for the target, `libpyshim.a` (`ports/apps/pyshim.c`), the static
  cross build with `zcc`, the install, byte code compiled by the host Python (`unchecked-hash`), and `build/rootfs-python`: `bin/python3` 15.5 MB and
  `usr/lib/python3.15` 37 MB, 1226 files, 52 MB in all (`du` 52,364 KiB). The recipe ran end to end three times in this session (each patch change rebuilds it from the unpacked source).
- Six patches in `ports/patches/python/` (the sixth, `urllib` without proxies from the environment, is from the review round) (`_ssl` fd BIO; environment, byte code and user site off; `asyncio` self-pipe on `os.pipe`; `main` on a 2 MiB thread stack (8 MiB at first; see the review round); no `pyvenv.cfg` search in `getpath.py` and `site.py`), `ac_cv_header_sys_ioctl_h=no`
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
- `test_threading` (CPython's) first failed with `Out of memory`; that was `os.fork`, not thread creation (see the review round: it is in the subset now); `test_thread`, `test_threading_local` and `test_queue` pass. A very large thread stack gives `MemoryError`.
- No `socketpair`, `AF_UNIX`, server sockets, `posix_spawn`, shared modules, `ctypes`.
- The shell prints hints for `/dev/urandom` and `/system/tls/roots` after a script that touches them without the grants.

### Decisions (taken without asking, as told)
Patch 0004 (the thread) over a larger initial stack in the kernel (an ABI constant for every process); no weakening of Python's hash randomization for a child with no random connection (the test cases are left out instead);
no `-E`-style environment variable honoured; the client test clears `VERIFY_X509_STRICT` and not the test PKI's certificates (shared with other steps).

### For the independent review (opus-review)
Patch 0001 (`_ssl` BIO) and 0004 (the thread and the exit path: `exit()` from a non-main thread); the shims in `pyshim.c` (`dirfd`, `gethostbyname`, `execv`); the surface block of ADR-0320 (170 names added to `libc.a`);
the environment policy (patch 0002) as a security boundary; `tools/run-disk.sh`'s colon-separated roots and the key; the `test_threading` out-of-memory finding.
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

**9d closed (2026-11-13).** Merged `65b1522` (head `616fe29`, candidate run 43465681384 green); ADR-0317 and ADR-0318 accepted and
on `main` with it; tag `stage-9d` = `7adf938` (with 9c). The wheel came with 9e. Downloads, the window channel of a restarted shell
and the other limits above are in `docs/backlog.md` ("Stage 9 parts: what the reports left").

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

**9e closed (2026-11-13).** Merged `dd575e3` (head `ea11ae6`, candidate run 43498404799 green); ADR-0322 and ADR-0323 are `proposed`.
The wheel in `zoraya-app` programs and the carving of the session's clients are in `docs/backlog.md`.

## 9c: the merge of `main` with 9d (branch `work/9c`, 2026-11-13)

`origin/main` (65b1522, 9d merged) merged into `work/9c`; every conflict keeps both sides (the netsurf and python targets,
`TEST_STEPS`, `ci-build` builds `netsurf` and `python`, `tools/ci-tests.txt`, `plan-lib.sh`, one map line per prefix with the
union of both steps). The host Python's prefix is now `build/ports/hostpython`: `build/ports/host` holds NetSurf's host tools,
which CI's artifact must keep for `test-netsurf` (it left `build/ports/host` out for Python before). The workflow's build job
has 60 minutes (a cold cache builds NetSurf and CPython; not measured), ADR-0317 is in the ports cache's key, and the long-job
timeout names `test-apps` and `test-netsurf` as well. Invariants Total 628. No ADR number collides (0320 here; 0321
`work/tls-options`, 0322 and 0323 `work/9e`).

Run on the Mac (HVF): `make check` ok; `make netsurf` ok (`bin/netsurf` 12.5 MB); `make test-python` (rebuilt with the new
prefix) `PASS: 107 steps`, 0 request bytes to the refused servers; `make apps`; a 192M volume of `rootfs-apps` (curl, wget,
NetSurf) and `rootfs-python` (77.9 MB of files, 20,195 of 49,152 blocks) made by `tools/run-disk.sh`, `zoraya-fsck` clean;
`make test-hand-run` ok (three runs PASS). Not run: `make test-netsurf`, CI.

## CI experiment: GitHub's free arm64 runners (2026-11-13, branch `work/ci-arm`, not merged)

Result accepted by the architect: the `ubuntu-24.04-arm` runner has no KVM (`/dev/kvm` missing; `-accel kvm` exits 1 with
"failed to initialize kvm"), and arm TCG is 1.4 to 1.7 times slower than x86 TCG for the four QEMU steps measured
(`make test-qemu-auto-default` 3:13 against 4:48, `make test-qemu-tcg-smp4-gicv3` 3:44 against 5:54, `make test-net` 8:32
against 12:15, the `threads.script` posix job 8:34 against 14:55; whole job 30:54 against 45:44; one run each, cold
caches, run 43447464141). Decision: move nothing; `work/ci-arm` is not merged and stays as the branch for a later KVM
probe (its workflow `ci-arm.yml` runs on pushes to that branch and by hand). The details are in
`docs/research/ci-arm.md` on that branch.


### 9c review round (opus-review E1 to N14, branch `work/9c`)
- E1 the host python is configured `--without-ensurepip` (the CI container has no zlib headers); a clean host build with no zlib module and `compileall` of the stage ran here. E2 the interpreter thread ends the process with `exit(status)`; `smoke.py` checks a fork child's `sys.exit(5)` (5) and an uncaught exception (1): passed. N12 no fallback to 64 KiB (message, status 1).
- W3 the stack is 2 MiB, measured: fork capacity on 512 MiB 11 (8 MiB), 15 (2 MiB), 16 (1 MiB); 1 and 2 MiB passed the deep-nesting cases (`json`, `ast`, `compile`, `pickle`, `copy`). W4 the `Out of memory` was `os.fork`; `test_threading` runs with no OOM at 2 MiB (not run: `--mem 2G`; not repeated at 8 MiB); 4000 threads leave the mapping table flat; recursion past about 33,000 frames ends in `MemoryError` (one mapping per frame chunk, the table is 256).
- W5 `-c` and `-m` no longer import from the current directory (patch 0002); a script imports from its own directory, said in `python3.cmd`; `cwd none` considered, not taken. W6 the default context finds the roots: text fixed, test added. W7 `http.client` and `urllib` tests. W8 negative `grants` check. W9 INV-0312-1 note, Python not mediated, patch 0006. W10 markers, `build/python-testlib`, the artifact and cache leave out the Python trees (cold `ci-build` time not measured here). W11 `test-run-disk.sh` extended. N13 `pause`, `sched_yield`. N14 text slips.


### 9c re-review round (opus-review E1, W2 to W6, N7 to N10, branch `work/9c`)
- E1 the python stamp's recipe makes the source and host trees again when a marker outlived them; `make check-python-cache` (in
  `make check` and CI's checks job) dry-runs a cache hit and was seen failing on a mutant (host tree check removed). Simulated here
  for real: the trees deleted, the markers kept, `libzshim.a` touched: `make python` printed "unpacking it again" and "building the
  host python again" and ended with the image part (52,332 KiB, 1226 files). CI: the libc ADRs are in the ports cache key and get the
  old time, `build/ports/workloads` is cached, the build job's timeout is 45 min (N7; cold time on CI still not measured).
- W2 patch 0004: the main thread blocks every signal; Ctrl-C to `time.sleep(30)` prints `KeyboardInterrupt` (new script step). N10
  `abort()` after the join. W3 the negative grants check looks between the report's first line and the prompt; the mutant
  "roots without +net" (`tools/mutants/python-9c.toml`) failed it ("/system/tls appeared where it must not"). W4 exactly 3
  zero-byte connections per refused server, none with more. W5 patch 0002: only a script file adds a directory; `python3 <
  fromstdin.py` does not import the planted module. N8 `-D_PYTHREAD_NAME_MAXLEN=15`; `test_set_name` out of the ignore list.
- `make test-python` (HVF): `PASS: 107 steps`, regrtest 3,329 tests, 66 skipped, `Result: SUCCESS`, 15 min 59 s; `test_threading`
  took 15 min 29 s and ended "env changed" (uncaught `OSError` in `do_fork_and_wait` threads, the `fork` out-of-memory of ADR-0320's
  open question); regrtest counts it as passed. Its time is a risk for the 30-min CI test job under TCG: not measured on CI.
- Candidate run 43488132441 (`candidate/work-9c-e`): every job passed but `make test-python`, cancelled at the 30-minute job
  limit under TCG (CI has no HVF): the script's steps before regrtest took 4.5 minutes, regrtest's first 11 modules 1.6 minutes,
  and `test_queue` had not ended after 22 minutes. The recipe also lacked `exit $rc` (a failed run left make with status 0); fixed.
  Split (ADR-0320 item 9): `make test-python` (`python.script`, without regrtest), `make test-python-regr` (`python-regr.script`,
  regr.py over `regr-modules.txt`), each a CI job of 45 minutes, the runner's limits 900 s and 1500 s; `make test-python-long`
  (by hand, not in `make test` nor nightly) over `tests/python/regr-long.txt`. Measured with `--slowest`: the 17 modules under the
  Mac's TCG 5 min 39 s, `test_queue` 4 min 5 s of it (9 s under HVF), so **the CI subset is the 16 other modules** (core types,
  `re`, `json`, `zlib`, `hashlib`, `sqlite3`, `_thread`, `threading.local`, `asyncio` tasks, futures, locks) and **the long list is
  `test_queue` and `test_threading`** (15.5 min under HVF). Runs on the Mac: `make test-python` HVF `PASS: 104 steps` (106 s with
  the build), TCG `PASS: 104 steps` (112 s), INV-0320-3 server check ok, fsck clean; `make test-python-regr` HVF `PASS: 9 steps`,
  16 modules, 2,949 tests, 44 skipped, `Result: SUCCESS`, 21.3 s. `make test-python-long` not run in this round.

**9c closed (2026-11-13).** `work/9c` merged `main` (with 9d, `work/tls-options` and 9e) in `7adf938`, and `main` was moved to it;
candidate run 43505870979 (`candidate/work-9c-g`) green; `main`'s full run 43511821485 green on its second attempt (the first lost a
download, the status block). Tags `stage-9c` = `stage-9d` = `stage-9` = `7adf938`. ADR-0320 is `proposed`, with item 10 (INV-0312-1's
reading) for the architect. Left: `make test-python-long` (by hand), the limits of the status block, `docs/backlog.md`.

## `work/tls-options`: curl and wget refuse the options that weaken TLS (closing note)

This branch wrote no section of its own; what it built is in ADR-0321 and `docs/design.md` ("TLS for C programs, curl and wget"):
patches `ports/patches/curl/0003-refuse-tls-weakening-options.patch` (curl's `getparameter`, by option code) and
`ports/patches/wget/0004-refuse-tls-weakening-options.patch` (wget's `setval_internal`, by setting), `tests/shell/apps-refusals.script`
(every refused option with `+net` to the test servers, no request byte at the expired and wrong-name servers) and the mutants of
`tools/mutants/tls-options.toml`. Merged `f90d29f` (head `5b52463`, candidate run 43497477629 green). ADR-0321 is `proposed`. Open
(ADR-0321's open questions, `docs/backlog.md`): no check diffs the refused lists against a new curl or wget release; wget's
`https` to `http` redirect rule; `--cert-status` by default.

## 9f-text: NetSurf's text through `zoraya-text` (branch `work/9f-text`, 2026-11-14)

The architect's decision of 2026-11-14 is recorded as ADR-0326 (accepted, "Decided by the architect; recorded by an agent at the
architect's request"); it supersedes ADR-0318's fonts item only (ADR-0318's note and index row say so). Model: Opus 5.5, one
session, 01:15 to about 02:30.

Done:
- `crates/zoraya-fontset` (`no_std`, no `unsafe`): four faces of the system's fonts with fallback lists, width, position and split as
  NetSurf documents them, glyphs from `zoraya-text`'s cache; the fonts are asked for only as `/system/fonts/NAME` (14 host tests:
  the paths read, missing and broken fonts, every Russian and Ukrainian letter a non-blank glyph that is not the box at 16 and 32
  pixels (12.8 pt at scale 1 and 2), widths, position, split, invalid UTF-8, zero-width characters, random bytes).
- `libc.a`: `zoraya_text_init`, `_width`, `_position`, `_split`, `_metrics`, `_glyphs` (`crates/zoraya-posix/src/ffi/text.rs`) and
  `zoraya_window_scale` (`ffi/window.rs`), ADR-0326's `posix-surface-added` block, added to `ADDED_ADRS`.
- NetSurf: `ports/netsurf/font_zoraya.c` (`NETSURF_FB_FONTLIB := zoraya`, `-DFB_USE_ZORAYA`), patch `0008` (font.h, the text
  plotter, the toolkit's text fields; their pixel-to-point conversion uses the page's dpi, else the URL field's text was twice its
  field at scale 2); dpi 90 times the window's scale.
- The grant: `/system/fonts` read only with a `window` line (`zoraya_shell::grants::with_window`, `zosh` run and `grants`); the
  window shows a third pill. `tools/run-disk.sh` copies `fonts/` into the disk's `/system/fonts` (in the key; `tools/test-run-disk.sh`
  extended); `make test-netsurf`'s root too.
- Tests: the runner's `ink X Y W H RGB N` step; `/ns/cyrillic` in `tools/test-httpd`; `tests/gui/netsurf.script` checks it at scale 1
  and the new pill gap; `tests/gui/netsurf-text.script` at scale 2 in a second boot of `make test-netsurf`. Screens:
  `docs/netsurf/netsurf-cyrillic.png`, `netsurf-cyrillic-2.png`.
- Found on the way: `zoraya-ar` dropped every symbol of an object whose `.bss` is larger than the file; with seven more names the
  call log's `zoraya-calls.o` (a 648-byte `.bss` in a 704-byte object) lost `__zoraya_calls*` from `libc.a`'s index and
  `make check-ports` failed. Fixed (`SHT_NOBITS` takes no file bytes) with a test.
- Proposed ADR-0327: OFL-1.1 for unmodified font files as data in `CLAUDE.md`'s licence rule (Option A recommended). Until the
  architect rules, no font was added: NetSurf reads the files ADR-0162 already ships, at run time.

Run on the Mac (HVF): `make check`; `cargo test -p zoraya-fontset -p zoraya-ar`, the new `zoraya-shell` and runner tests;
`tools/test-run-disk.sh`; `make netsurf`; `make -f ports/Makefile check-ports` up to its rootfs part (libc.a against the lists
passed; the `rootfs-posix` check needs `make ports`, not built here); `make test-netsurf` (`netsurf-auto` PASS 86 steps,
`netsurf-text-2` PASS 19 steps, the INV-0228-1 log check, fsck clean). One earlier exploratory run lost QMP (broken pipe at a
screendump) once; the rerun and the real run did not.
Also run after `libc.a` changed: `make test-libposix` (PASS 53 steps), `make test-posix` (every script PASS), `make test-apps`
(`apps-auto` and `apps-refusals-auto` PASS 302 steps each, `apps-launches` 12); the host tests of `zoraya-ar`, `zoraya-fontset`,
`zoraya-posix`, `zoraya-shell`, `zoraya-test-httpd` and `zoraya-test-runner` (18 test binaries, all ok). Not run: `make test-changed`
(the change touches `Makefile`, `Cargo.toml` and the runner, so it maps to every step, which an agent session does not run), TCG,
`test-python`, `test-gui`; the candidate CI runs them.

Open for the architect: ADR-0327 (the licence rule); CI's `ports-build` cache key (`.github/workflows/test.yml`) does not name
`docs/adr/0326-*.md` (left to 9f-ci: the task kept CI caching out of this part; `libc.a` is rebuilt on every build anyway, since
`zoraya-posix` is `FORCE`); a person's disk from before 9f-text is replaced once (kept as `.old`), since the fonts are in its key.
Limits: no italic, toolbar not scaled, text cost per page not measured, native GUI programs still embed their fonts.
## 9f-wheel: the wheel in `zoraya-app` (branch `work/9f-wheel`, 2026-11-14)

Decided by the architect 2026-11-14: the file manager, the editor and settings scroll with ADR-0322's `Wheel`, a notch
consistent with the terminal's three lines, smooth with high-resolution axes, clamped, a list keeping its selection and
cursor rules; the other native applications too where they share the toolkit. The new decision is ADR-0329 (`proposed`):
a third widget event and its units. What exists is in `docs/design.md`, "The wheel in the applications (as built)".

Done:
- **Toolkit.** `zoraya_ui::Event::Wheel { x, y, dx, dy }`; `Window` routes it to the widget under the pointer, then up to
  the nearest ancestor that can go that way (a widget at its end answers `None`); no focus moves. `WheelSteps` (3 rows a
  notch, 40/120 a row, the rest kept, forgotten at an end, at most 64 notches); `scrolled_by`, `widgets::wheel_rows`.
  `Widget::scrolled`/`set_scrolled`/`reveal`; `Window::scrolled(id)`, `Window::replace_keeping_scroll`.
- **`ScrollView`** rebuilt: it never moved what it showed (its one child was laid out at the frame's top, the height cut).
  It is now a column of children shown from `first`, children before it laid out empty, `first` clamped so the last child
  is at the bottom, focus (Tab or a key) on a child out of view scrolls it in. `ScrollView::new` takes the children.
- **`zoraya-app`**: ordinal 46 to `Event::Wheel` (saturated to `WHEEL_MAX`); after a turn or a key that changed something
  the window is laid out again at once, so a press before the paint hits what is shown; `App::replace_keeping_scroll`.
- **Applications.** `ListView` (the chooser's sheet, which now reads where the wheel left its list before it rebuilds, and
  follows the selection only when it moved), the file manager's list (over the list only, not with the menu or card open)
  and the editor: the first row or line moves, clamped; the selection and the cursor stay; a key, a press, or a cursor the
  program moves (find, new folder) brings the view back. Settings: the page is always a `ScrollView` and is rebuilt with
  `replace_keeping_scroll`; the editor too.
- **Tests.** Host: `zoraya-ui` `tests_wheel.rs` (10 tests) and the two rewritten `ScrollView` tests, `zoraya-app` (2),
  `zoraya-files` (4), `zoraya-editor` (4), `zoraya-settings-window` (1), `zoraya-chooser` (1). GUI: `tests/gui/wheel.script`
  (new; `GUI_RUNS_ALL` and `tools/ci-tests.txt` gain `wheel:auto:1`, `wheel:tcg:1`, `wheel:auto:2`): the file manager's
  selected row three rows up after a notch and back, a key after a far turn bringing the cursor back; settings (two rooms)
  with its controls 135 pixels higher after a notch and back, then Apply reached at the end (`settings: No change.`); the
  editor's empty lines in the rows that showed `x` lines after a notch away from the person, and at the top.
- `tools/mutants/editor.toml`: three mutants re-anchored (`occurrence = 1`, the follow block's new indentation);
  `tools/mutants/wheel-9f.toml`: seven mutations made by hand (parts of a notch dropped, the stop at both ends, no
  routing up, no reveal, no layout after a turn, the file manager and the editor following the cursor anyway).

Ran on the Mac (HVF, this session): `make check`; `cargo test` in `zoraya-ui`, `-app`, `-files`, `-editor`,
`-settings-window`, `-chooser` (all pass); `make test-gui GUI_RUNS="wheel:auto:1"` and `"wheel:auto:2"` (74 steps
each, PASS); `make mutants MUTANTS_ARGS=tools/mutants/wheel-9f.toml` (7 caught, 0 survived). `make test-changed`:
the checks and the host tests of the 13 affected test binaries passed; since the `Makefile` changed it chose every step,
and its build stopped at the ports (`build/ports/src/dash-0.5.13.5.tar.gz is missing`: this fresh worktree never ran
`make fetch-ports`), so no QEMU step of it ran. Not run locally: `wheel:tcg:1`, the other GUI scripts, `make test`; the
candidate run on CI covers them.

Limits (also `docs/backlog.md`): settings scrolls by whole children, not pixels (`Logical` is unsigned; ADR-0329 Option A
not taken); no scroll bar; `dx` scrolls nothing; no Page Up or Page Down in the file manager and the editor.

## 9f-net: the `web` grant for browsers and NetSurf's refusal page (branch `work/9f-net`, 2026-11-14)

Decided by the architect (2026-11-14) and recorded at the architect's request as **ADR-0324** (the web grant) and **ADR-0325**
(the refusal page), both `accepted`. Six invariants (INV-0324-1 to -4, INV-0325-1, -2); `docs/adr/invariants.md` Total 640.

What exists (details in `docs/design.md`, "NetSurf on the web"):
- **The rule.** `zoraya_abi::net::Target::Public` (reach kind 5): every address outside `NOT_PUBLIC` (0/8, 10/8 with QEMU's
  10.0.2.0/24, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.168/16, 198.18/15, 224/4, 240/4), names only through
  their answers (`name_grant`/`resolve_grant`: `PublicOnly`); it never names an address, so a local one (this machine, the
  gateway, a DNS server) needs a rule that does; only a web rule is within a web rule.
- **`netd`.** A name only the web rule admits uses public, non-local answers, `net.hosts` included; a `resolve` with none left is
  `NotGranted`; a local address the web rule admits by range is `NotGranted` at `connect`.
- **The shell.** `net web` in a description of the image gives `Public`, ports 80 and 443, TCP; `grants netsurf` prints
  `net: web  reach public hosts, ports 80 and 443; not this machine or its network (description)`; the window's pill is `web`
  (the panel accepts that one word); `netgrant::web_refused` refuses it in an untrusted room (markers `untrusted-gate:
  web-refusal` and two `web-refusal-call`, checked by `make check-untrusted-gate`; `tools/untrusted-gate.txt` unchanged, the
  gate stays closed). NetSurf's description takes it; curl and wget keep the per-URL rule.
- **The page.** NetSurf patch 0009 (0008 before the merge with 9f-text, whose patch is 0008) and `ports/netsurf/zoraya_refusal.c`: a fetch that ends with `connect` `EACCES`, or a lookup
  refused (`EAI_SYSTEM`/`EACCES`, asked a second time), gets a 403 page `Not granted: HOST:PORT` with the line `+net HOST:PORT
  netsurf URL` (quoted, escaped); other failures keep NetSurf's error page.
- **The test network.** `tools/run-qemu.sh --net-subnet` and `--net-guestfwd`; `make test-netsurf` runs on 203.0.113.0/24
  (TEST-NET-3 as the Internet) with two "public" hosts `one.web.test` and `two.web.test` relayed to the test server by
  `zoraya-test-httpd --relay` (SLIRP's `guestfwd ... cmd:`, a process per connection).

Ran on the Mac (HVF): `make check` (all 21 steps, with the new `check-netsurf-refusal`); `cargo test -p zoraya-abi -p zoraya-net
-p zoraya-shell -p zoraya-describe -p zoraya-panel`; the 11 mutants of `tools/mutants/net-web.toml` applied by hand, all killed;
`make netsurf`; `make test-netsurf` (all 131 steps passed, with the server-log checks of INV-0228-1 and INV-0324-1; screens `docs/netsurf/netsurf-web.png` and `netsurf-web-refused.png`). Not run: TCG, `make test-changed`'s other steps beyond those named
here, the full set.

Limits: IPv4 only; a name the web rule alone admits cannot use a `net.hosts` entry for a local address; a name rule's local answer
(`Local`, `EAI_NONAME`) gets NetSurf's error page, not the refusal page (ADR-0325's open question); the page is in the window's
body, which a site can imitate (it grants nothing); curl and wget have no such page (ADR-0224 item 5's `REPORT` hint is not built);
the web rule cannot be written as a `+net` or room word (a word `web` on a line is a host named `web`).

### 9f-net: the review's findings (`docs/tasks/TASK-27-review-9f-net.md` on `docs/review-9f-net`)

- **1 (must-fix), fixed.** The page no longer offers `+net any:PORT`: `zoraya_refusal_plain_host` mirrors the shell's
  `parse_word`/`valid_name` and refuses `any`, `web` and a host whose last label is all digits but which is no dotted
  address; such a host gets the page without a line. `make check-netsurf-refusal` tests hosts named `any`, `ANY`, `web`.
- **2, not changed: proposed ADR-0328.** A URL typed on the line under `net web` still gets its own name rule, which
  accepts private answers, so its name could rebind to 192.168.1.1:80 on later fetches. ADR-0324 item 2 says so; the
  options (keep; public-only like the web rule, recommended; no rule) are in ADR-0328 for the architect.
- **3, fixed.** SLIRP relays `two.web.test:8080` to the test server, so a leak through the port check is logged and fails
  the recipe; mutant `W-web-name-any-port-gui` (tests: `make test-netsurf`), run once by hand: the server's log held
  `GET /ns/must-not-reach` and the recipe failed (the page-colour step 122 first, rc 2).
- **Ran** after the fixes and the merge of `main`: `make check`; `make check-netsurf-refusal` and the host tests of
  `zoraya-abi`, `zoraya-net`, `zoraya-shell`; the hand mutants of the fixes, all killed; `make netsurf`; `make
  test-netsurf` (145 steps passed, both server-log checks ok). `make test-changed` failed in `test-libposix`,
  `test-posix`, `test-net` (`test-net-posix`) and `test-tour-gui` (`tcc hello.c` gives no `./hello`): the local
  `libc.a` index lacks `__zoraya_calls`, `__zoraya_calls_count` and `__zoraya_calls_list`, the `zoraya-ar` problem of
  the review's last section, also on `main`, fixed by `548e8f2` on `work/9f-text`; nothing of this branch.
- **4, fixed.** The shown URL is cut at 1024 bytes with a mark, the host at 253; a URL too long to repeat gets the line
  of the host's first page (`+net HOST:PORT netsurf SCHEME://HOST/`); the page always fits 16 KiB (tested at the worst
  bytes). That line is not the refused URL: ADR-0325 item 3 does not foresee it, for the architect to note.
- **5, fixed.** Patch 0009 reads curl's own lookup through `--wrap=getaddrinfo` and asks no second time. ADR-0325 item 1
  and its Consequences still say "a second `getaddrinfo`": the mechanism changed, the rule (`EAI_SYSTEM`/`EACCES`) did
  not.
- **6, fixed.** The decoder refuses a web rule with any protocol but TCP, so `netd` does (host tests in `zoraya-abi` and
  `zoraya-net`, mutant `W-web-rule-any-protocol`).
- **8, 9, 10, fixed.** `grants` of a native command calls the untrusted-room web refusal (a third `web-refusal-call`);
  `grants` shows every port range of a web rule and `any port`; tests for a redirect to a refused host and a refused
  lookup (`make test-netsurf`), and hosts named `web` and `any` (shell and page host tests).
- **7, not changed.** Other hosts of a public on-link subnet are reachable under `web` (ADR-0324 item 2 defines local as
  ADR-0223 does); recorded in `docs/design.md`'s limits, an open question for ADR-0324.

## 9f: a usable browser (branch `docs/9f-final`, 2026-11-14)

The final step of part 9f: the real desktop (`main` at `e8bf184`, NetSurf 3.11 with the `web` grant, ADR-0324/0325/0326) on the real
Wikipedia, through QEMU's user network to the Internet. Nothing is mirrored or faked. Two hand-run scripts, in no CI list:
`make hand-netsurf-wikipedia` (`tests/gui/netsurf-wikipedia.script`: front page and article of ru and uk) and `make
hand-netsurf-wikipedia-2` (`tests/gui/netsurf-wikipedia-2.script`: scrolling, a click on a link, a redirect to another host). The image
is `test-netsurf`'s with no test names and no test PKI: the production roots in `/system/tls/roots`, DNS from QEMU's lease, `--net
--net-open`, scale 1, 1024x768, under HVF. The script types `netsurf URL` into the terminal, waits and takes screendumps through the
runner's QMP (`shot`); nothing is asserted (the sites change). Both runs ended `PASS` (44 and 30 steps). Cyrillic URLs are typed
percent-encoded (the runner types keys, not UTF-8).

Screens (`docs/netsurf/`, 1024x768 PNG, 100 to 150 KB each; the screen 45 s after the start and 105 s after it are identical):

| Screen | What it shows |
|---|---|
| `wikipedia-ru-front.png` | `https://ru.wikipedia.org/` redirected (same host) to `/wiki/Заглавная_страница`; pills `+net ru.wikipedia.org:443`, `web`, fonts, tls |
| `wikipedia-ru-moskva.png` | `/wiki/Москва`, the top of the article ("Done (1.1s)") |
| `wikipedia-ru-moskva-scrolled.png` | the same after 6 wheel notches: a caption, the infobox rows (Страна, Статус, Субъект Федерации) |
| `wikipedia-ru-link.png` | a click on a link of the article (a `Файл:` page of the same host): page, back button live |
| `wikipedia-uk-front.png` | `https://uk.wikipedia.org/` redirected to `/wiki/Головна_сторінка`; the Ukrainian welcome text |
| `wikipedia-uk-kyiv.png` | `/wiki/Київ` (percent-encoded `%D0%9A%D0%B8%D1%97%D0%B2`) |
| `wikipedia-redirect-www.png` | `http://wikipedia.org/` (port 80) redirected to `https://www.wikipedia.org/`, a second host and port, which the web grant admits; the title bar keeps the line's `+net wikipedia.org:80` and `web` |

What works: TLS to the real sites with the production roots (no refusal), DNS through the web rule's public-only answers, HTTP to HTTPS
and cross-host redirects, Russian and Ukrainian text (ґ є і ї included) in the system's fonts, with links in blue, headings, bold, the
navigation row, a page's logo (PNG), the `web` pill, the wheel, the back button, same-host links. Each page was done in 0.3 to 1.3 s
after the fetch by the status bar's count, and was complete within 45 s of the command.

What does not, plainly:
- **Pages are not laid out as a browser would.** The article is a column of left-aligned pieces: the infobox is flattened, and in the
  article views the prose below the lead did not appear in what I scrolled (21 notches on Москва ended on blank space and the
  "Москва" captions, `wp2-ru-scroll-2`, not kept): either NetSurf's CSS 2.1 layout loses Wikipedia's current (flex, grid, `:has`,
  variables) styles, or text is hidden by a rule it does not read. Not diagnosed. Kyiv shows a title, the "other meanings" lines and the
  flag and coat of arms captions (as text) and then blank space.
- **Empty frames are images.** The boxes (the search icon, the menu and language buttons, the flag and coat of arms images) are SVG
  or WebP files, which NetSurf here does not decode (no SVG, ADR-0318); those from `upload.wikimedia.org` are fetched under the web grant
  (a second host), but are not drawn. The logo, a PNG on the page's own host, is.
- **CJK and the other scripts show as boxes** (`wikipedia-redirect-www.png`: the Japanese, Chinese and Korean portal entries): the
  system's fonts have Latin and Cyrillic only (ADR-0326).
- Text overlaps on `www.wikipedia.org` (the portal's absolutely positioned labels), the Ukrainian title row's "Перевірена версія" sits
  over the tab row; the search box is missing (it is a form of the page's script and an SVG button). No JavaScript runs, so a page that
  needs it for content shows what its `noscript` or static HTML gives.
- The runs take about 12 and 6 minutes because of the waits I chose (45 s and 60 to 90 s per page), not because the pages are slow.

What the architect should look at: (1) the flattened article layout and the missing prose below the lead (is it CSS support, or a
bug in `zoraya-text` or layout of tall boxes? a `netsurf -v` run of the article would say which style sheet fails); (2) SVG and WebP,
which decide whether Wikipedia looks like itself; (3) a CJK-capable font, if the world's scripts are in scope; (4) ADR-0328 (a URL on
the line under `net web` gets its own name rule), since a real redirect from the typed host to another one now happens in the field.
Untrusted code never ran, the gate stays closed, and no code of the kernel or the shell changed in this step (a Makefile target and two
scripts).

## 9g-url: a URL's name rule under `net web` is public-only (branch `work/9g-url`, 2026-11-14)

ADR-0328 Option B, accepted by the architect (closes the 9f-net review's finding 2: `netsurf http://evil.example/` kept a
name rule that took a later answer of 192.168.1.1, a GET to the router).

- **The rule.** `zoraya_abi::net::Target::PublicName` (encoding kind 6: a length byte and the name), TCP only in the
  decoder. It admits its one name on its ports through the public, non-local answers (`name_grant`/`resolve_grant` give
  `PublicOnly`, `Target::admits_public_answers`), so `netd`'s checks of a web rule's answers apply with no change in
  `netd`; it covers and names no address. Subset: within a web rule on its ports, the same public-only name, a name or
  wildcard rule that names the name, and `any`; only the same public-only name is within it.
- **The shell.** Under the description's `net web` a literal URL whose host is a name adds no rule on 80 and 443 and a
  public-only name rule on another port; a dotted address keeps its address rule; a `+net` word that covers the URL keeps
  ADR-0223's rule; curl and wget (no `net web`) are unchanged. Pill `+net HOST:PORT public` (the panel takes `web` only
  alone, ADR-0324 item 6, so the mark follows the target; a panel test admits it), `grants` line `reach, public answers
  only (operand URL)`.
- **Decisions taken without asking.** A new kind, not a flag bit (a decoder that ignored a flag would read a name rule,
  which takes every answer); a wildcard rule over the name counts as a parent; the pill's form above. Recorded in
  ADR-0328's open questions (rule-6 note); notes in ADR-0324 (item 2's words "and the URL operand's rule", INV-0324-2
  with the new kind) and ADR-0325 (a private-only URL name now gets the page). INV-0328-1: `checked: make test-unit,
  make test-netsurf`.
- **`make test-netsurf`.** The `one.web.test` window has no pill for its URL; `netsurf http://two.web.test:8080/ns/web-port`
  (public answer, relayed) shows the server's lime page; `netsurf http://lan.web.test:8080/...` (10.0.2.2 in
  `net.hosts`) gets ADR-0325's page with `+net lan.web.test:8080`. The HTTPS steps on `test.zoraya` and `expired.zoraya`
  (the gateway 203.0.113.2 in `net.hosts`, on ports that are not 80 or 443) now type `+net HOST:PORT` first, as a person
  would: without it their rule refuses the gateway's answer, which is ADR-0328's point.
- **Ran (HVF, this session).** `make check`; `cargo test -p zoraya-abi`, `-p zoraya-net`, `-p zoraya-shell`, `-p
  zoraya-panel`; `make mutants` with the new `U-*` mutants of `tools/mutants/net-web.toml` and `W-web-rule-any-protocol`
  (11 caught, 0 survived), and `U-public-name-names-the-name` by hand against `zoraya-net` alone (both netd tests fail);
  `make test-netsurf` (186 and 19 steps, both server-log checks ok). Two earlier runs failed on this branch's own script
  until fixed: the pill `web HOST:PORT` was refused by the panel (no window), and the HTTPS steps needed their `+net` words.
  `make test-changed`: `make check`, host tests (51 binaries), build, test-boot, test-qemu, test-files, test-posix,
  test-tour-gui, test-gui, test-net all ok; its plan also named test-apps, test-netsurf, test-python and
  test-python-regr, which its lanes did not run (test-netsurf ran on its own, above; the others are left to CI).
- **Not done / limits.** curl and wget keep a URL's name rule (ADR-0328's open question). The refusal page says "does not
  cover HOST on port PORT" also when the rule covers the name but not its private answer. `W-web-name-any-port-gui` (a
  `make test-netsurf` mutant) was not rerun.
