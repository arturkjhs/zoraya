# ADR-0451: What `libposix` answers for Rust's `std`: a `long` `syscall()` with a fixed table, `FIONBIO`, ten names, and a Rust-only archive for the rest

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0100, ADR-0141, ADR-0301, ADR-0302, ADR-0320, ADR-0366, ADR-0411, ADR-0412, ADR-0450
- **Evidence:** CP-27, DM-053, DM-057, DM-120, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, rust-std/survey.md#the-probes, rust-std/survey.md#what-libposix-would-have-to-add, rust-std/survey.md#what-std-calls-through-syscall, rust-std/survey.md#what-std-links, rust-std/survey.md#zoraya-today

## Context

ADR-0450 recommends Rust's prebuilt `aarch64-unknown-linux-musl` std over `libc.a`. That `std` was compiled for Linux
and reaches Linux in three ways (rust-std/survey.md#what-std-links, rust-std/survey.md#what-std-calls-through-syscall):

- **22 libc names** `libc.a` does not export: seven hidden by the tier list, of which four are implemented (`bcmp`,
  `getauxval`, `pause`, `pipe2`) and three are libposix's `ENOSYS` stubs (`sendmsg`, `setgroups`, `setsid`); fifteen
  absent, three of them on ADR-0005's deny list (`setuid`, `setgid`, `sigaltstack`). `std` references `posix_spawnp`,
  `socketpair`, `sendmsg` and `recvmsg` strongly, so no Rust program links without them.
- **`syscall()`**, through `std`'s `syscall!` macro, which falls back to `libc::syscall` because a weak reference never
  pulls a function out of a static archive: `futex` (`FUTEX_WAIT_BITSET` with an absolute `CLOCK_MONOTONIC` deadline and
  a full mask, `FUTEX_WAKE`), `getrandom`, `statx`, `copy_file_range`, `splice`, `gettid`, `pidfd_*`.
- **`ioctl(FIONBIO)`** for every `set_nonblocking`.

Measured (rust-std/survey.md#the-probes): libposix's `syscall` is `not_yet!(int syscall tee sendmsg ...)` and returns a
32-bit `-1` (so do `tee` and `sendmsg`, whose prototypes return `ssize_t`);
`std` reads `long`, sees 4294967295 and treats it as success, so `HashMap::new` panics, `fs::copy` writes a wrong
file and every futex wait returns at once (a condvar ping-pong of 1000 rounds took 37.5 s; with a correct futex,
10 ms). `ioctl(FIONBIO)` answers `ENOTTY`, so `Command::output` panics. With `syscall()` and `FIONBIO` emulated in the
program, every probe check passed except `Command::current_dir`. libposix's `zoraya_posix_futex` answers `FUTEX_WAIT`
with a relative timeout, not `FUTEX_WAIT_BITSET` (rust-std/survey.md#zoraya-today).

Six colliding symbols (libposix's own Rust allocator shims and panic handler under the `__rustc` pseudo-crate, whose
mangled hash is the same for every target of one compiler) stop the link until renamed.

## Decision drivers

1. **The surface is a list** (CP-27): every name and every system-call number answered is written down.
2. **Linux's numbers are not Zoraya's ABI** (DM-057): the table is the few calls `std` makes, not "Linux".
3. **Correct by type**: a `syscall()` that returns `int` is a defect for C callers too (`<unistd.h>` declares `long`).
4. **The deny list holds** (INV-0005-2): `setuid`, `setgid`, `sigaltstack` do not enter `libc.a` without an ADR, and this
   ADR does not want them there.
5. **Typed calls over op codes** (DM-120, DM-053): the futex and `ioctl` answers map onto libposix's typed operations
   and accept only the exact forms `std` sends.
6. **Measured need**: only what the probes or `std`'s source show is answered (rust-std/survey.md#what-libposix-would-have-to-add).

## Considered options

### Option A: Answer Linux's system calls by number (the Linux baseline)

- **Description:** `syscall()` dispatches every Linux number libposix can map, as a Linux kernel would.
- **Evidence for:** Any program or crate that calls `syscall()` works; the Starnix shape
  (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).
- **Evidence against:** An unbounded, unlisted surface (drivers 1 and 2); Linux's numbering becomes a second ABI
  (DM-057).
- **Cost:** *(estimate)* Grows with every program.
- **Precedent:** Starnix, the Linuxulator.

### Option B: A fixed `syscall()` table, `FIONBIO`, ten exported names, a Rust-only archive

- **Description:** Items 1 to 7 below.
- **Evidence for:** Every failure the probes found is answered by items 1 and 2, except a spawned child's randomness
  (ADR-0453) and `Command::current_dir` (item 3), and the rest of the probe passed (rust-std/survey.md#the-probes); the archive follows `libpyshim.a` (ADR-0320) and libc++'s shim (ADR-0366).
- **Evidence against:** A program that calls `syscall()` with any other number gets `ENOSYS`; the names in the archive
  are failures, so a crate that relies on them gets errors.
- **Cost:** *(estimate)* 200 to 400 lines in `zoraya-posix` with host tests, a 50-line archive, the renaming rule.
- **Precedent:** ADR-0366's surface for C++.

### Option C: Put every fix in the program (the probe's "fixed" variant)

- **Description:** Leave `libc.a` as it is; link Rust programs with `-Wl,--wrap=syscall -Wl,--wrap=ioctl` and a shim
  that does items 1 and 2 (rust-std/survey.md#the-probes).
- **Evidence for:** Measured working; no change to libposix.
- **Evidence against:** The `int` defect stays for C; the wrapped `syscall` must call libposix's internal
  `zoraya_posix_futex` by name; two linker tricks in every Rust program.
- **Cost:** *(estimate)* The 91-line shim in the archive.
- **Precedent:** the probe.

### Option D: Answer `std` through the native ABI with a `std` of Zoraya's own (the Fuchsia way)

- **Description:** As Fuchsia's `std` calls its kernel's typed futex and handle calls directly, a Zoraya `sys` module
  would call `futex_wait`, the random service and the stream calls with no `syscall()` and no `ioctl`
  (alternatives-postmortem.md#fuchsia--zircon-incl-starnix for Zircon's typed futex calls, DM-120).
- **Evidence for:** No Linux numbers at all (driver 2); typed calls (driver 5).
- **Evidence against:** It needs a `std` built for a Zoraya target, which is ADR-0450's rejected Route B (nightly or a
  compiler fork).
- **Cost:** *(estimate)* ADR-0450 Option C or D.
- **Precedent:** Fuchsia, Redox.

### Option E: Change nothing

- **Description:** Rust programs get what `libc.a` has today.
- **Evidence for:** No work.
- **Evidence against:** They do not link (22 names, six collisions); with a shim they spin and panic
  (rust-std/survey.md#the-probes).
- **Cost:** None.
- **Precedent:** Zoraya today.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`syscall()`** becomes `long syscall(long number, ...)` in `zoraya-posix` and answers exactly:
   - `SYS_futex` (98): `FUTEX_WAIT` and `FUTEX_WAKE` as today, and `FUTEX_WAIT_BITSET` whose mask is
     `FUTEX_BITSET_MATCH_ANY`, whose deadline is absolute on `CLOCK_MONOTONIC` (converted to ADR-0301's monotonic
     deadline), each with or without `FUTEX_PRIVATE_FLAG`; any other operation or mask `ENOSYS`.
   - `SYS_getrandom` (278): as `getrandom`.
   - `SYS_gettid` (178): as `gettid` (item 3).
   - Every other number: `-1` with `errno = ENOSYS`, and a line on the `REPORT` channel once per number (as the
     resolver reports a path, ADR-0100), so that a program's unanswered call is visible. `std` falls back from
     `ENOSYS` for `statx`, `copy_file_range`, `splice`, `pidfd_open` and `pidfd_send_signal`
     (rust-std/survey.md#what-std-calls-through-syscall); these five are not reported.
   - Every `not_yet!` stub returns its prototype's type: `tee` and `sendmsg` (`ssize_t`) return a 64-bit `-1` too.
2. **`ioctl(fd, FIONBIO, &int)`** sets or clears `O_NONBLOCK` as `fcntl(F_SETFL)` does, on every descriptor `fcntl`
   accepts; other requests are unchanged.
3. **Names `libc.a` adds** (the block below): the hidden `bcmp`, `getauxval` (musl's, over the auxiliary vector
   libposix lays out, ADR-0361), `pause` and `pipe2`; `sched_yield` (libposix's
   `zoraya_posix_sched_yield`); `clock_nanosleep` (`CLOCK_MONOTONIC` and `CLOCK_REALTIME`, relative and
   `TIMER_ABSTIME`, over `nanosleep`'s wait; other clocks `EINVAL`); `dirfd`; `gettid` (the thread's kernel id, as `pthread_self`'s); `posix_spawn_file_actions_addchdir`
   (POSIX.1-2024) and its `_np` alias. This reverses two earlier choices, named here: ADR-0366 rejected exporting
   `sched_yield` from libposix (its Option B), and libc++'s shim keeps a weak `sched_yield` (INV-0366-2), which the strong
   export now overrides without a clash, so INV-0366-2 stands unchanged; CPython's `libpyshim.a` (ADR-0320 item 3)
   defines `dirfd` (`ENOTSUP`), `pause` and `sched_yield`, and the part that builds this removes those three from it,
   recording the change in ADR-0320 (ADR-0001 rule 6 note).
4. **Order of parts.** `std` needs `posix_spawnp` (ADR-0411 item 1) and `socketpair`, `sendmsg`, `recvmsg` (ADR-0412)
   to link. The Rust part builds `posix_spawnp` itself as ADR-0411 item 1 specifies it, if the foundation part has not
   yet; `socketpair`, `sendmsg` and `recvmsg` are in the Rust-only archive as `ENOSYS` until ADR-0412's part exports them,
   and leave the archive in that part.
5. **`libzoraya-rust.a`**, built from `ports/rust/zoraya-rust.c` into the sysroot and linked only by `tools/zcargo`
   (ADR-0452), defines the names `std` references that `libc.a` keeps out, each failing with `ENOSYS`: `setuid`,
   `setgid`, `setgroups`, `setsid`, `chroot` (`CommandExt` options), `sigaltstack` (`std` then installs no alternate
   stack), `sched_getaffinity` (`std` falls back to `sysconf`), `sendfile`, `splice` (`std` falls back), `waitid`. A name
   enters it only by an ADR. **This is a scoped exception to INV-0005-2's intent**, named here: in a program `zcargo`
   links, `setuid`, `setgid` and `sigaltstack` resolve (to failures), so a crate's C code that calls them links and gets
   `ENOSYS` at run time instead of failing at link. `libposix` still exports none of them, and no C or C++ program built
   by `zcc` or `zc++` links the archive (INV-0451-2).
6. **The `__rustc` names** of libposix's Rust runtime (`_RNvCs..._7___rustc*`) are renamed when `libc.a` is assembled,
   by the same hiding step as the tier list (`posix-surface-check hide` stops treating them as internal), so that no
   Rust program collides with libposix.
7. **`std`'s probes** of `/proc/self/cgroup` and `/proc/self/mountinfo` (from `available_parallelism`) are `ENOENT`
   without a report or hint; `/proc/self/maps` likewise (read when a backtrace is symbolised). Any other missing path is
   reported as today.

```posix-surface-added
bcmp
clock_nanosleep
dirfd
getauxval
gettid
pause
pipe2
posix_spawn_file_actions_addchdir
posix_spawn_file_actions_addchdir_np
sched_yield
```

### Effect of the vision

For target 2, the table and the archive are the whole of what a Rust program can ask beyond C's surface, and every
unanswered call is reported. Target 3: Rust programs' threads wait instead of spinning. Networking: `FIONBIO` makes
`std::net`'s non-blocking connect work under the existing grant. Threads: the futex answer is ADR-0301's, unchanged.

### Examples

```
$ rust-tool                     # a Rust program whose crate calls syscall(SYS_memfd_create)
rust-tool: syscall 279 is not answered (ENOSYS)     # the report's wording is the part's (illustrative)
```

## Consequences

- **Positive.** C callers of `syscall()` get a correct `long`; `std`'s synchronisation sleeps; `HashMap`, `fs::copy`
  and `Command::output` work (measured with the emulation); `connect_timeout` gets past `FIONBIO` (its non-blocking
  connect path was not exercised); the surface grows by ten listed names.
- **Negative.** Zoraya answers three Linux system-call numbers and one Linux `ioctl` request; a Rust-only archive to keep
  in step with `std`.
- **Follow-up.** The part that builds ADR-0450 to ADR-0454; `docs/design.md`'s libposix section.

## Invariants

- **INV-0451-1.** `syscall()` returns `long` and answers exactly the numbers of item 1; any other number returns `-1`
  with `errno == ENOSYS`; every libposix definition returns the width its C prototype declares. *Check:* a C test
  program of `make test-libposix`, compiled by `zcc` against the sysroot's headers, calls `syscall` with an unanswered
  number, `tee` and `sendmsg` through their prototypes and asserts each result is `-1L` (not 4294967295), and calls each
  futex form; host tests in `zoraya-posix` for each answered number; `not_yet!` takes the return type per name.
- **INV-0451-2.** `libzoraya-rust.a` defines only names listed in this or a later accepted ADR, each failing, and is
  linked into no program but those `tools/zcargo` links. *Check:* `make check-rust-archive` lists its defined names
  against the ADR's list and greps every non-Rust link line (`zcc`'s, `zc++`'s) for it.
- **INV-0451-3.** No defined name of `libc.a` comes from the `__rustc` pseudo-crate. *Check:* `posix-surface-check`
  fails on a defined `_RNvCs*_7___rustc*` name in the archive.

## Open questions

- Whether `getauxval`'s answer for `AT_HWCAP` should describe the CPU (Rust's `std_detect` then uses optional
  instructions) or stay 0; 0 is safe and was measured.
- Whether `gettid` should equal the broker's pid for the main thread, as Linux's does.

## Revisit triggers

- A Rust release adds a `syscall!` use or an `ioctl` that the table does not answer.
- A program needs a number outside the table: a new ADR, never a silent row.

## Architect's decision

Accepted 2026-11-15. Decided by the architect; recorded by an agent at the architect's request.

As recommended, including the stated limit on INV-0109-2 for Rust programs and the changes to ADR-0418 item 4, ADR-0288 item 6, INV-0411-4, ADR-0310 item 3 and INV-0310-1; each change is recorded by the part that implements it, with an Opus review.
