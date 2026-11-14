# TASK-27: review of the getrandom fix in a process with threads (branch `work/py-roots`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...origin/work/py-roots` at `c8f1f38` ("libposix: getrandom in a process with threads waits for rngd
inside the lock"): `crates/zoraya-posix/src/net.rs`, `src/random.rs`, `tests/random.rs` and the two TLS clients of
`tests/python/`. Read with the code around it (`jobs.rs` `wait_for`, `wait_point`, `end_run`, `interrupt_sources`;
`ffi.rs` `state`, `wait_unlocked`, `RtSys::wait`; `ffi/procs.rs` `restartable`; `ffi/sockets.rs` `getrandom`), against
ADR-0302 (items 8 to 10, INV-0302-2), ADR-0315 (items 1, 3, 5, 9), ADR-0310 (INV-0310-1, INV-0310-5) and ADR-0227
item 4, and the consumers of `getrandom` in the ports: OpenSSL 3.5.9's `rand_unix.c` patch, CPython 3.15.2's
`Python/bootstrap_hash.c` and `Modules/_randommodule.c` (the unpacked tree of `build/ports/work`), musl 1.2.5's
`getentropy.c` and `__randname.c`.

What was run: `cargo test -p zoraya-posix` (all pass), `cargo test -p zoraya-posix --test random` with the source
change of `c8f1f38` reverted (the new test fails: `left: Err(5)`, `right: Ok(5000)`, that is `EIO`), the same with
finding 1's span set to 4096 (the new test of finding 1 fails), and `make check`. No QEMU.

## Findings

1. **error (fixed on the branch).** `crates/zoraya-posix/src/random.rs:76` (the chunk loop) with
   `src/net.rs:144` and `src/ffi/sockets.rs:496` (before the fix). **What.** The fix makes every chunk's wait for
   `rngd` a wait inside the library lock in a process with threads, and `Posix::getrandom` runs its whole loop, one
   2 KiB request after another, in one run of `restartable` (one hold of the lock). So a `getrandom` of `n` bytes held
   the lock for `ceil(n / 2048)` round trips, against INV-0302-2 ("no thread blocks for longer than a service round
   trip while holding the library lock"). Before the fix the threaded path never waited in the lock (it failed
   instead), so the fix is what made this reachable. **Scenario.** A python3 (always threaded, patch 0004) runs
   `os.urandom(8 << 20)` on one thread: 4096 round trips with the lock held, during which every other thread's
   `read`, `write`, `getpid` and signal delivery waits. **Fix.** `random::in_spans` (`random.rs:46`): the C boundary
   calls `restartable` once per span of `SPAN` = one request, so each hold is one round trip; the result is
   `Posix::getrandom`'s (all of it, the first span's error with nothing written, or the count written so far).
   A side effect, within Linux's rules: without threads, a signal whose handler restarts (`SA_RESTART`, or no handler
   ran) during a later span now resumes the call to the full count instead of returning a short count. Test
   `the_c_boundary_holds_the_lock_for_one_request_at_a_time` (one request per call, spans 2048/2048/904, the bytes, the
   zero-length, `EINVAL` and `ENOSYS` answers, a failure in the first and in a later span, a short span).

2. **note.** `src/net.rs:144`, the fix itself: **sound.** `net_call_with(.., true)` has one caller, `getrandom`
   (`random.rs:90`). Every other `wait_point`/`wait_for` site either passes `Err(PENDING)` up with `?` (`jobs.rs:463`,
   `jobs.rs:547`, `net.rs:321`, `process.rs:1237`, `ffi/procs.rs:873`) or handles it (`process.rs:1338`); this arm
   was the only one that turned it into another errno. With `!self.defer_waits` the arm is taken only when
   `wait_for` waits itself, so `PENDING` cannot reach it, and nothing is left in `self.pending` (`end_run` drops it
   for a result other than `PENDING` anyway). The wait in the lock is what ADR-0302 item 9 and ADR-0315 item 1 allow
   for a service's reply. Lock order: the thread holds `LIB` (and maybe a lock of the program above it, such as
   OpenSSL's DRBG mutex, which is above `LIB` in the order), never `MEM` or a musl lock (ADR-0315 item 9); `rngd` is
   another process and needs nothing from this one, so no cycle. A `fork` in a sibling waits for `LIB` for one round
   trip and then proceeds; the child has no connection (`ENOSYS`). A thread interrupt cannot end `RtSys::wait`
   (`port_wait` on the library's own port, which no `ThreadRec` registers: `wait_unlocked` registers a port of its
   own per wait), so no spurious error comes from a routed signal or a cancel.

3. **note.** `src/ffi.rs:130` (`RtSys::wait`, infinite deadline). **No bound on the wait.** `rngd` gone is an end:
   `PEER_CLOSED` without `READABLE` is `ENETDOWN`, then `EIO` (test `a_service_that_is_gone_is_eio` and the second
   half of the new test). `rngd` alive but silent stalls the calling thread with the lock held, and with it the
   process's library; only a kernel-delivered end (`SIGKILL`, the job killed) ends it. This is the same as every
   service call inside the lock (the known limitation of ADR-0315 item 1); not new, not fixed here.

4. **note.** Signals and cancellation. With threads a signal for the thread in `getrandom` waits for at most one
   round trip (after finding 1) and is then delivered by `restartable`'s next poll; it is never `EINTR` there. Linux
   does not interrupt a `getrandom` of up to 256 bytes once the pool is ready, and may interrupt a larger one
   (`EINTR` or a short count), so never being interrupted is within its behaviour. Without threads, an `EINTR` of
   the first chunk of a small request is possible (pre-existing, unlike Linux for at most 256 bytes); every
   consumer read here retries `EINTR` (OpenSSL's `ossl_pool_acquire_entropy`, CPython's `py_getrandom`, musl's
   `getentropy`). `getrandom` is not a cancellation point in libposix, with or without threads, before or after the
   fix (musl's `src/linux/getrandom.c` is `syscall_cp`, but that directory is not compiled; glibc documents it as one).
   A `pthread_cancel` of a thread in `getrandom` takes effect at its next cancellation point. Recorded in
   `docs/design.md`.

5. **note.** `src/net.rs:148` and `:155`, the other `ENETDOWN` arms. Still reachable: `wait_any` failing (a port or
   subscription the kernel refuses, a quota) is mapped by `finish_wait` to an errno and then to `ENETDOWN`, so `EIO`
   from `getrandom`: loud, never weaker. `wait_point` giving `Ok(0)` (a wake by an interrupt source that is "no longer
   a reason", `jobs.rs:227`) would also end in `ENETDOWN` instead of reading again; it is unreachable today, since
   `interrupted` returns `EINTR` for both kinds of source `interrupt_sources` adds. If a third kind is added, the
   `s & READABLE == 0` test should go round again on `s == 0`.

6. **note.** `tests/random.rs`, the new test `a_process_with_threads_gets_the_bytes_when_the_reply_comes_late`:
   it forces the timing (the fake service answers only inside a wait) and fails without the fix (run above). Its
   `Mode::Gone` half gives `(Err(EIO), 1)` with and without the fix, so it guards only against a future deferral of
   that case, not the bug. The `deferred` helper reimplements `ffi::state`'s loop; it is not the boundary itself.

7. **note (should be recorded; recorded in `docs/design.md`).** CPython's `random` module seeds itself from the time
   and pid when `os.urandom` fails (`Modules/_randommodule.c:304-311`, `PyErr_Clear` and `random_seed_time_pid`),
   silently. Before the fix every python3 was threaded and its `getrandom` failed intermittently, so `random` (and
   `tempfile`'s names, which use it) could be time-seeded in a normal run; the hash seed failed loudly
   ("failed to get random numbers to initialize Python", `bootstrap_hash.c:584`) and `os.urandom`, `secrets` and
   `SystemRandom` raised. After the fix it remains for a child of `os.fork` (no connection, `ENOSYS`, then
   `/dev/urandom`, which no default namespace entry provides). `random` is documented as not for security and
   `tempfile` opens with `O_EXCL`, so the cost is predictability, not a weak key.

8. **note.** CPython's `ENOSYS` fallback opens `/dev/urandom` without checking that it is a character device
   (`bootstrap_hash.c` `dev_urandom`). The default namespace has `/dev/null` and `/dev/tty` only; a program given a
   writable directory that covers `/dev` and a regular file named `urandom` in it would, in a process without the
   connection, read that file's bytes as entropy for `os.urandom`, `secrets` and `ssl`'s Python side. OpenSSL is not
   affected (getrandom only, no `/dev/urandom` code, INV-0310-5).

9. **note.** Other consumers, checked and sound: OpenSSL's seed source is `getrandom` only (patch 0001,
   `--with-rand-seed=getrandom`); a short count is added and the rest asked again, `EINTR` retried, any other error
   ends the seeding (the original symptom, `ERR_R_RAND_LIB`). musl's `getentropy` (compiled, `src/misc`) loops on
   `EINTR` and fails on the rest. musl's `__randname` (`mkstemp`, `mkdtemp`) uses the clock and an address, as
   upstream, with `O_EXCL`. libposix's `AT_RANDOM` (`ffi.rs:1032`) is the clock and an address, the weak source
   ADR-0109's open question records; its only reader is musl's `__init_ssp`, and `tools/zcc` compiles with
   `-fno-stack-protector`; musl's `malloc` is not compiled (the heap is libposix's). curl, wget and NetSurf take their
   TLS randomness from OpenSSL; their own non-TLS uses of `rand` were not read (their sources are not unpacked here).

## Checked and found sound

The fix's branch condition and its single caller; that no `PENDING` is swallowed elsewhere; the lock order and the
`fork` freeze with a sibling waiting for `rngd`; that a thread interrupt cannot end the wait inside the lock; `EIO`
for a service that is gone, with and without threads; the diagnostic lines of the two Python TLS clients (they only
print and re-raise).
