# ADR-0309: How the threads are tested: musl's libc-test subset, SQLite's `threadtest3`, a stress program, OpenSSL with threads

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0001, ADR-0110, ADR-0300, ADR-0301, ADR-0302, ADR-0303, ADR-0304, ADR-0305, ADR-0306, ADR-0307, ADR-0308
- **Evidence:** DM-120, GR-011

## Context

TASK-27 part 9b is done when "a pthread conformance subset (for example from musl's libc-test or the Open POSIX Test
Suite), SQLite's threaded tests, a stress test, and OpenSSL rebuilt with threads" pass under HVF and TCG. The task leaves
the choice of suite to this design. The existing method (ADR-0110, ADR-0141): ports are pinned in `ports/sources.lock`
with a SHA-256, built with `zcc`, run inside Zoraya by a `tests/posix/*.script`, and wired into `make test-posix`-style
steps with a line in `tools/test-map.txt` and `tools/ci-tests.txt`. Locally only `make check` and `make test-changed` run;
CI runs the TCG runs, the tour's second run and everything the nightly set holds (CLAUDE.md).

What each candidate is `[outside evidence]` unless noted: musl's `libc-test` is the test suite of musl's author; it has
`src/functional` and `src/regression` directories whose tests are small standalone C programs that print failures and exit
non-zero, built by a makefile; it needs no harness beyond a shell; its licence is MIT. The Open POSIX Test Suite
(`posixtestsuite`, now part of LTP) has about 1,000 assertion programs for threads, each a program that prints PASSED or
FAILED and exits 0, 1 or 2 ("unresolved", "unsupported"); its tests are under the GPL, and many assertions are for the
real-time, process-shared and priority options that Zoraya refuses on purpose (ADR-0302 item 6). SQLite's `test/threadtest3.c`
is one C file, linked with the amalgamation (already pinned for W2 and W3), that runs several multi-threaded scenarios
(`walthread*`, `cgt_pager_1`, `dynamic_triggers`, `checkpoint_starvation`, `create_drop_index_1`, `lookaside1`,
`vacuum1`, `stress1`, `stress2`, ...), each a pass or a fail; it comes in the full source tree, not in the amalgamation
zip.

The research's lesson on the interface under test (GR-011, DM-120): futex users are the hardest to get right; the stress
test must therefore hammer the wait and wake path (lost wakes, broadcast, time-outs), not only run examples.

## Decision drivers

1. Tests of musl's own thread code are the best oracle: ADR-0302 retains that code, so a failure points at the shim, the
   kernel or the lock, not at the algorithm.
2. Licences: only permissively licensed code is vendored into the build of the system (CLAUDE.md "Dependencies"; test
   code that is run but not shipped is held to the same line for simplicity).
3. Each test is a separate program with an exit status: no framework in Zoraya is needed beyond `zosh` and a script.
4. The suites must run in minutes under TCG, which is 10 to 15 times slower than HVF (INV-0006-3's figures).
5. Tests must cover what ADRs 0300 to 0307 add and not only libc: the kernel's refusals, the quota, the two-phase end.

## Considered options

### Option A: libc-test's thread tests as the conformance subset, plus `threadtest3`, our stress program, and OpenSSL's threaded use

- **Description:** As recommended below.
- **Evidence for:** Drivers 1 to 5.
- **Evidence against:** libc-test has no releases (it is pinned by commit); its thread coverage is a few dozen programs,
  not the thousand assertions of the other suite.
- **Cost:** *(estimate)* a harness script of 80 lines, a pinned archive, an exclusion list.
- **Precedent:** musl's own CI runs it.

### Option B: The Open POSIX Test Suite's threads conformance directories

- **Description:** Build `conformance/interfaces/pthread_*` and `sem_*` and run them all.
- **Evidence for:** Coverage by assertion in the POSIX text, the broadest.
- **Evidence against:** Driver 2 (GPL); driver 4 (about 1,000 programs, most for options refused here); the results need a
  list of expected `UNSUPPORTED` outcomes that would be a second source of truth beside ADR-0302 item 6.
- **Cost:** *(estimate)* a larger harness and an expected-failure list of several hundred lines.
- **Precedent:** LTP.

### Option C: Our own tests only

- **Description:** Write the pthread tests in `tests/posix/` ourselves.
- **Evidence for:** No new pinned source.
- **Evidence against:** The oracle is the author's reading of the same ADRs; musl's tests encode years of bugs in the same
  code (`pthread_cond-smasher`, `pthread_once-deadlock`, `pthread_exit-dtor`).
- **Cost:** *(estimate)* 1,500 lines to reach libc-test's coverage.
- **Precedent:** ADR-0110's own workloads for T1.

## Recommendation

**Recommendation (not a decision): Option A.** The four things:

1. **Conformance subset: musl's `libc-test`**, pinned by commit with a SHA-256 in `ports/sources.lock` (the commit and
   hash are recorded by the 9b step that fetches it, then never changed; the archive is kept in the ports cache). The
   subset is a rule, not a list: every program of `src/functional` and `src/regression` whose name begins with `pthread_`,
   `sem_`, `tls_`, `thrd` or `cnd`, plus `fork`-, `vfork`- and `spawn`-named ones, **minus** the written exclusions below,
   which the harness reads from `tests/posix/threads-excluded.txt` (one name and a reason per line). Initial exclusions,
   each with its reason: `pthread_mutex_pi` (priority inheritance refused, ADR-0302 item 6); `sem_open` (named
   semaphores not exported, ADR-0308 item 2); the `*_dlopen` and `tls_align_dlopen` ones (no `dlopen`); tests that need
   an asynchronous signal (`sigaltstack`, `sigreturn`, `raise-race`, `pthread_kill` handlers run in a compute loop: ADR-0304
   limit); and `pthread_create-oom` if it relies on `RLIMIT_AS`. The first run decides the rest: a test that fails for a
   reason that is a documented limit is added to the file with that reason; a test that fails otherwise is a defect of
   the step. The harness prints the counts: run, passed, excluded, failed.
2. **SQLite: `threadtest3`.** Its source is `test/threadtest3.c` from `sqlite-src-3540100.zip` (the full tree of the version
   already pinned for the amalgamation; one new line in `ports/sources.lock`, the hash recorded at fetch). It is
   compiled with the amalgamation and `-DSQLITE_THREADSAFE=1 -DSQLITE_OMIT_LOAD_EXTENSION` and run with a time limit per
   scenario. Scenarios that need shared memory (`walthread*` use WAL's `-shm` mapping, which ADR-0005 excludes) are excluded in
   `threads-excluded.txt` with that reason and replaced by `PRAGMA journal_mode=DELETE` runs of the same scenarios if the
   program allows a flag; the rest must pass. SQLite's file locks need `fcntl` record locks, which the files service
   gives per ADR-0013 `[to be read by the step]`.
3. **Stress program: `tests/posix/t_threads.c`**, ours, one binary with named cases, each with a seed and an iteration count
   and a wall limit: (a) a mutex counter with 8 threads; (b) a condition-variable producer and consumer ring with
   timed waits and broadcast; (c) 100,000 `pthread_once` races; (d) create and join 2,000 threads in batches of 32, and
   detached ones; (e) a thread per core of the quota: creation to the `EAGAIN` of `threads` and to the memory limit
   (ADR-0307), then recovery; (f) cancellation of threads in each wait (ADR-0305); (g) `fork` from a threaded process with
   a sibling holding each lock (ADR-0306); (h) signals: `pthread_kill`, `sigwait` thread, process-directed `SIGTERM`
   with all threads blocking it (ADR-0304); (i) `exit` from a non-main thread with siblings spinning, `exec` with siblings
   opening files (INV-0306-4); (j) TLS: `errno` and a `_Thread_local` array per thread (ADR-0303). The cases are the
   in-QEMU tests of the `INV-03xx` rows that name "an in-QEMU `libposix` test".
4. **OpenSSL with threads.** The 9a build of OpenSSL is rebuilt with `threads` (and `no-shared`, `no-dso`, `no-engine`
   as before), its lock and `CRYPTO_THREAD_*` functions over `pthread_*`, `pthread_once` and thread-local keys. Two checks
   in `tests/posix/ossl_threads.c`: 8 threads each doing 200 SHA-256, AES-GCM and `RAND_bytes` operations whose
   results match single-threaded ones, and 4 threads each making an HTTPS request to the runner's TLS test server
   (9a's), plus `curl -O https://...` from 9a repeated unchanged, which must still pass.
5. **Where and when.** One step `test-posix-threads` (`make test-posix-threads`), added to `tools/ci-tests.txt` and to
   `tools/test-map.txt` for `kernel/src/process.rs`, `sched.rs`, `syscall.rs`, `crates/zoraya-posix/`, `ports/` and
   `tests/posix/`; local `make test-changed` runs it under HVF; the candidate set (CLAUDE.md) runs it on TCG too, with
   per-case limits scaled for TCG. A failure of a pinned-suite test that is not in the exclusion file fails the step.

**Tensions with accepted ADRs.** *ADR-0110's* acceptance harness gets a second script. *CLAUDE.md* "Dependencies": test code
pinned in `ports/sources.lock` is not a dependency of the kernel or of a `no_std` crate.

## Consequences

- **Positive.** The oracle is outside the project for libc and SQLite, and ours for the Zoraya-specific parts.
- **Negative.** libc-test's thread coverage is narrow; the exclusion file is a list to keep honest; two new pinned inputs.
- **Follow-up.** The pin lines; `tools/test-map.txt`; the CI job name in `tools/ci-tests.txt`.

## Invariants

- **INV-0309-1.** Every test excluded from the pinned subset has a reason in `tests/posix/threads-excluded.txt`, and no test program
  of the subset is excluded without one. *Check:* a script in the harness that fails on a line without a reason or on a
  name not in the pinned tree.
- **INV-0309-2.** `make test-posix-threads` prints the counts run, passed, excluded and failed, and exits non-zero for any failure
  outside the exclusion file. *Check:* the step itself, run on a deliberately failing copy of one test.
- **INV-0309-3.** The two new sources are in `ports/sources.lock` with a SHA-256, and `make check-ports` verifies them. *Check:*
  `make check-ports`.
- **INV-0309-4.** The stress program's cases map one to one to the in-QEMU invariants of ADRs 0300 to 0307 that name a `libposix`
  test. *Check:* a table in the program's header, compared with the ADRs by a grep in the harness.

## Open questions

- Which libc-test commit: the latest at the time of the fetch.
- Whether Open POSIX's `pthread_*` assertions that Zoraya supports are worth a one-off run by the architect (not wired in).

## Revisit triggers

- A pthread defect found in a port that the subset did not catch: add its test to `t_threads.c`.

## Architect's decision

(Left empty for the human.)
