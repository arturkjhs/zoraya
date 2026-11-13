# ADR-0308: POSIX threads enter the tier list: what supersedes ADR-0100's "no threads", ADR-0141's `futex` answer and ADR-0180's stubs

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0005, ADR-0006, ADR-0041, ADR-0100, ADR-0102, ADR-0104, ADR-0106, ADR-0141, ADR-0180, ADR-0225, ADR-0260, ADR-0261, ADR-0263, ADR-0285, ADR-0300, ADR-0301, ADR-0302, ADR-0303, ADR-0304, ADR-0305, ADR-0306, ADR-0307
- **Evidence:** DM-004, task-02/posix-surface.md#jumps-between-tiers, task-02/posix-surface.md#key-observations

## Context

ADR-0005 put threads outside T0 to T2 because the measured programs create none; `pthread_create` is on its deny list,
INV-0005-2, "unless an accepted ADR adds it", and the tcc `sem_*` calls stay as single-threaded no-ops
(`task-02/posix-surface.md#key-observations`, item 3; `task-02/posix-surface.md#jumps-between-tiers`: thread functions and thread clones appear only
at T3, where ld.lld creates four threads). ADR-0100 item 5 turned that into construction: no `pthread_*` export,
`sem_*` counters, one thread. ADR-0141 item 2 answered `futex` with `-ENOSYS` in the shim's header and listed "a second
thread in a POSIX process (T3): `futex` needs a real route" among its revisit triggers. ADR-0180 item 9 made
`pthread_setcancelstate` and the cleanup functions stubs (the `getpwnam` case in `docs/backlog.md`). ADR-0225 is the
precedent for adding a tier by an accepted ADR: "the TN rows".

Stage 9 (TASK-27) needs threads for 9a's OpenSSL built with them, 9c's CPython (threads are not optional in CPython since
3.7 `[outside evidence: CPython 3.7 release notes]`), SQLite's threaded users and the programs 9d brings. The
task's done criterion is that "ADR-0100's 'no threads' is superseded by an accepted ADR". Only the architect accepts or
supersedes (ADR-0001 rule 2); this ADR is that proposal and says exactly what it replaces. ADR-0100 itself stays
`accepted`: only its item 5 and the parts named below are replaced, as ADR-0225 and ADR-0287 replaced parts of earlier
ADRs.

## Decision drivers

1. State exactly which sentences of which accepted ADRs stop binding, so that no agent has to guess (ADR-0001 rule 4).
2. The export check (INV-0005-1, INV-0100-1, INV-0005-2) must know the new names, so that "everything exported is in a list"
   stays true.
3. A program that uses no thread must see no change (ADR-0302 INV-0302-5).
4. Nothing outside `libposix` and the kernel call set changes: the broker, the services and the personality boundary
   (ADR-0002) are as they are.

## Considered options

### Option A: One new tier, T9 "threads", with musl's whole thread surface minus the refused parts

- **Description:** As recommended below.
- **Evidence for:** Drivers 1 to 4; one list, one rule, and the programs of stage 9 find the names musl gives.
- **Evidence against:** More exported names than any one program needs; each is a name to test (ADR-0309's rule: a name is
  exported only if the conformance subset or a port uses it, or it is part of a pair with one that is).
- **Cost:** *(estimate)* a list file of about 160 names, a tool change of 40 lines.
- **Precedent:** ADR-0225's TN tier.

### Option B: Only the names the stage 9 ports call (a minimal set)

- **Description:** `pthread_create`, `join`, `detach`, `self`, `mutex_*`, `cond_*`, `once`, `key_*`, `sigmask`, and the
  `sem_*` already there.
- **Evidence for:** The smallest surface; less to keep true.
- **Evidence against:** musl's thread code is one body: rwlocks, barriers, spinlocks and attributes link anyway, and a name
  that links but is not in the list fails INV-0100-1; a port that needs `pthread_rwlock_*` later needs another ADR.
- **Cost:** *(estimate)* exclusion of retained files by hand; a list that changes with each port.
- **Precedent:** ADR-0140's per-program stub list, which ADR-0141 replaced by the retained archive.

### Option C: No threads: stage 9 ports are built without them (`no-threads`, single-threaded CPython)

- **Description:** Keep ADR-0100 item 5; OpenSSL stays `no-threads`; CPython and SQLite's threaded users are not ported.
- **Evidence for:** Nothing to build.
- **Evidence against:** CPython has required threads since 3.7 (outside evidence above); 9c would not exist; the task says
  threads are wanted; DM-004's cost of retrofitting threads grows with every library written without them.
- **Cost:** *(estimate)* none now, a larger one later.
- **Precedent:** TASK-27 part 9a (`no-threads` until 9b).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **What this ADR supersedes, once accepted, and nothing else.** The list is complete: it names every sentence of an
   accepted ADR that ADR-0300 to ADR-0307 change, so that **part 9b cannot merge until all of ADR-0300 to ADR-0308 are
   accepted** (each of them changes accepted text, as the bullets say; ADR-0309 changes none and is the test method).
   What is *not* superseded is said too: ADR-0006 rule 5 and INV-0006-6 stand (no `Status` is added, ADR-0300 item 5).
   - ADR-0100 item 5, all of it: `sem_*` as counters, "No `pthread_*` is exported (INV-0005-2)", and the premise "a process
     has one thread" wherever ADR-0100 or ADR-0141 uses it as a reason (`ffi.rs`'s "one thread: a cell with no lock"). The
     TLS part of item 5 (musl's `__init_tls` retained, `__set_thread_area` is `msr tpidr_el0`) stays and is extended by
     ADR-0303.
   - ADR-0100 item 2's list of replaced directories, for `src/thread`: it is retained from now on, minus the files
     ADR-0302 item 1 lists.
   - ADR-0141 item 2's sentence "`futex` is answered with `-ENOSYS` in the header itself": the route is ADR-0302 item 2's.
     Its revisit trigger "A second thread in a POSIX process (T3)" is fulfilled.
   - ADR-0180 item 9 (`pthread_setcancelstate` changes nothing; the `nscd` socket calls fail): the first half is replaced by
     musl's functions (ADR-0305 item 3); the second half stands.
   - ADR-0005's exclusion row for `pthread_create` (INV-0005-2): the name leaves the deny list; the rest of the list stays.
   - ADR-0005's remark that tcc's `sem_*` "stay as single-threaded no-ops": they become real semaphores that behave the
     same with one thread.
   - `docs/design.md`'s sentences "One thread per process" and "Not built: threads" (the architect updates the Vision
     sections; agents update the as-built sections).
   - From **ADR-0300:** ADR-0041's rule that a process ends on its one thread (it ends on the owner thread of ADR-0300
     item 6; the zombie rule is unchanged); ADR-0105 item 8 (`job_suspend` parks "that thread": every thread of the
     process); ADR-0030's premise of one thread per process (the scheduler is unchanged); ADR-0031's premise in
     `map_reserve` that the process's one thread is in the call (ADR-0301 item 6); ADR-0270 and ADR-0285's comment in
     `exit_current` that the process has one thread there.
   - From **ADR-0301:** ADR-0141 item 2's `futex` answer (already above); ADR-0260's table row "Kernel deadlines: 288 in
     one queue" (a derived `QUEUE_SIZE`, ADR-0301 item 5).
   - **ADR-0006 INV-0006-2's lint**, and nothing else of ADR-0006: its parameter set `{ Handle, Port, Deadline, &mut
     [Packet], flags }` gains `Address` and `Word32` for `futex_wait`, and its count of blocking calls (exactly two,
     `crates/zoraya-abi/src/tests.rs`) becomes three (`port_wait`, `sleep_until`, `futex_wait`). Rule 2 (every blocking
     call takes a `Deadline`) holds; rule 5 and INV-0006-6 (`Status::ALL.len() == 17`, no `Interrupted`) are unchanged.
   - From **ADR-0302:** ADR-0100 item 2 (`src/thread` leaves the replaced directories) and ADR-0141 (above);
     ADR-0005's INV-0005-2 row.
   - From **ADR-0304:** ADR-0106 item 2 (the pending set is per process plus per thread) and item 5 (the entry poll is
     per calling thread); ADR-0106 item 6 and INV-0106-3 hold (ADR-0304 item 11).
   - From **ADR-0306:** ADR-0102 item 5 (the child's one thread: now the whole rule, with the freeze) and item 2's
     stepped copy (held under the mapping lock); ADR-0104 item 6 (`exec` holds the library lock until the old image ends).
   - From **ADR-0307:** ADR-0260's table row "Threads: later" and its open question on threads and processes; ADR-0261
     point 1 (`JobLimits` gains `threads`); ADR-0263 point 4's table (rows for threads); ADR-0285 point 4 (the usage
     record grows from 120 to 152 bytes); ADR-0266 point 1 stays five words.
2. **The T9 list.** `docs/adr/posix-surface-T9.txt`, written by the first implementation step and checked like the TN rows
   (ADR-0225 item 1, INV-0225-5): every defined global symbol of `libc.a` whose name does not begin with `_` is in
   `posix-surface-T1.txt`, in the TN list, or in this one. It holds the names of musl's `src/thread` and `<semaphore.h>`
   minus the refused ones: `pthread_attr_*` (`init`, `destroy` and every accessor),
   `pthread_atfork`, `pthread_barrier*`, `pthread_cancel`, `pthread_cond*`, `pthread_condattr_*`, `pthread_create`,
   `pthread_detach`, `pthread_equal`, `pthread_exit`, `pthread_getattr_np`, `pthread_getconcurrency`,
   `pthread_getspecific`, `pthread_join`, `pthread_key_create`, `pthread_key_delete`, `pthread_kill`, `pthread_mutex*`
   (without the priority-ceiling pair), `pthread_mutexattr_*`, `pthread_once`, `pthread_rwlock*`, `pthread_rwlockattr_*`,
   `pthread_self`, `pthread_setcancelstate`, `pthread_setcanceltype`, `pthread_setconcurrency`, `pthread_setspecific`,
   `pthread_sigmask`, `pthread_spin_*`, `pthread_testcancel`, `pthread_tryjoin_np`, `pthread_timedjoin_np`,
   `pthread_getname_np`, `pthread_setname_np`; C11's `call_once`, `cnd_*`, `mtx_*`, `thrd_*`, `tss_*`; `sem_init`,
   `sem_destroy`, `sem_post`, `sem_wait`, `sem_trywait`, `sem_timedwait`, `sem_getvalue`. **Not exported** (they return the
   error ADR-0302 item 6 gives, or are absent): `sem_open`, `sem_close`, `sem_unlink` (named semaphores need a shared
   namespace; absent), `pthread_getcpuclockid` (absent), `pthread_setschedparam`, `pthread_getschedparam`,
   `pthread_setschedprio` (exported, returning `ENOTSUP`), `pthread_mutex_getprioceiling` and `setprioceiling`
   (absent), `pthread_attr_setaffinity_np` and the other `_np` names not listed (absent).
3. **The tool.** `tools/posix-surface-check` reads the T9 list as a third list (a flag, as the TN rows were added) and the
   deny list loses `pthread_create`; `make check-ports` keeps checking both against the archive and the broken copies.
4. **Constants.** `sysconf(_SC_THREAD_THREADS_MAX)` is 128, `_SC_NPROCESSORS_ONLN` and `_SC_NPROCESSORS_CONF` are 1,
   `_SC_THREAD_STACK_MIN` is musl's `PTHREAD_STACK_MIN`, `_SC_THREADS` and `_SC_THREAD_SAFE_FUNCTIONS` are 200809, and
   `_POSIX_THREAD_PROCESS_SHARED` is -1, `_POSIX_THREAD_PRIO_INHERIT` and `_PROTECT` are -1 (the rows ADR-0100 item 6 left
   at `sysconf`).
5. **The architect's step.** When the architect accepts this ADR, ADR-0100 stays `accepted` and the index row for ADR-0100
   gets "item 5 superseded by ADR-0308" in its summary, and the rows of the other ADRs in item 1 get the same pointer;
   agents do not edit any of their texts or statuses.

**Tensions with accepted ADRs.** This ADR is the tension: items 1 above names each one. *ADR-0005's* "T3 and threads"
table row (`thread functions and thread clones appear only in T3`) stays true as a measurement and false as a scope
statement.

## Consequences

- **Positive.** One place says what changed; the export check keeps its meaning; stage 9's ports link.
- **Negative.** About 160 names in a list; `docs/design.md` and the ADR text of 0100, 0141 and 0180 read as stale in the
  superseded places until the architect marks them (the index row carries the pointer).
- **Follow-up.** The T9 list and the tool change (step 1 of the 9b plan); `docs/design.md`'s as-built text.

## Invariants

- **INV-0308-1.** Every defined global symbol of `libc.a` whose name does not begin with `_` is in `posix-surface-T1.txt`, the TN list
  or `posix-surface-T9.txt`, and `pthread_create` is in none of the deny lists. *Check:* `tools/posix-surface-check` with the
  T9 list, run by `make check-ports` and `make test-libposix`.
- **INV-0308-2.** The names the ADR lists as not exported are not defined in `libc.a`, and the three listed as `ENOTSUP` return it.
  *Check:* the same tool's deny list, and a `libposix` test of the three.
- **INV-0308-3.** A program built without `-pthread` and without calling a thread function links and runs as before: the
  `make test-posix` workloads' call log has no thread call. *Check:* the call log's union (ADR-0110) with the
  futex counter of INV-0302-5.
- **INV-0308-4.** `docs/design.md` has no sentence saying a process has one thread after the architect accepts this ADR. *Check:*
  a grep script added with the acceptance (until then, not applicable).
- **INV-0308-5.** The amended lint of INV-0006-2 expects exactly three blocking calls (`port_wait`, `sleep_until`,
  `futex_wait`), allows `Address` and `Word32` parameters for `futex_wait` only, and the lint of INV-0006-6 is unchanged:
  `Status::ALL.len()` is 17 and no variant is named `Interrupted`. *Check:* `cargo test -p zoraya-abi`
  (`crates/zoraya-abi/src/tests.rs`), with the new expectations written by step 1 of the 9b plan.

## Open questions

- Whether `sem_open` should be built over a named stream in the temp service; no port needs it.
- The names of `_np` extensions the ports use (`pthread_setname_np` in CPython): found by compiling.

## Revisit triggers

- A port that needs a refused name (process-shared locks, priority inheritance).

## Architect's decision

Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.
