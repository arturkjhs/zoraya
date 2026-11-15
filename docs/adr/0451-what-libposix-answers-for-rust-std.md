# ADR-0451: What `libposix` answers for Rust's `std`: a `long` `syscall()` with a fixed table, `FIONBIO`, ten names, and a Rust-only archive for the rest

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0100, ADR-0141, ADR-0301, ADR-0302, ADR-0320, ADR-0366, ADR-0411, ADR-0412, ADR-0450
- **Evidence:** CP-27, DM-053, DM-057, DM-120, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, rust-std/survey.md#the-probes, rust-std/survey.md#what-libposix-would-have-to-add, rust-std/survey.md#what-std-calls-through-syscall, rust-std/survey.md#what-std-links, rust-std/survey.md#zoraya-today

## Context

ADR-0450 recommends Rust's prebuilt `aarch64-unknown-linux-musl` std over `libc.a`. That `std` was compiled for Linux
and reaches Linux in three ways (rust-std/survey.md#what-std-links, rust-std/survey.md#what-std-calls-through-syscall):

- **22 libc names** `libc.a` does not export: six implemented and hidden by the tier list (`bcmp`, `pause`, `pipe2`,
  `sendmsg`, `setgroups`, `setsid`), sixteen absent, three of them on ADR-0005's deny list (`setuid`, `setgid`,
  `sigaltstack`).
- **`syscall()`**, through `std`'s `syscall!` macro, which falls back to `libc::syscall` because a weak reference never
  pulls a function out of a static archive: `futex` (`FUTEX_WAIT_BITSET` with an absolute `CLOCK_MONOTONIC` deadline and
  a full mask, `FUTEX_WAKE`), `getrandom`, `statx`, `copy_file_range`, `splice`, `gettid`, `pidfd_*`.
- **`ioctl(FIONBIO)`** for every `set_nonblocking`.

Measured (rust-std/survey.md#the-probes): libposix's `syscall` is `not_yet!(int syscall ...)` and returns a 32-bit `-1`;
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

- **Description:** Items 1 to 6 below.
- **Evidence for:** Every failure the probes found is answered by items 1 and 2, and the rest of the probe passed
  (rust-std/survey.md#the-probes); the archive follows `libpyshim.a` (ADR-0320) and libc++'s shim (ADR-0366).
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

### Option D: Change nothing

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
     `ENOSYS` for `statx`, `copy_file_range`, `splice` and `pidfd_*` (rust-std/survey.md#what-std-calls-through-syscall);
     these four are not reported.
2. **`ioctl(fd, FIONBIO, &int)`** sets or clears `O_NONBLOCK` as `fcntl(F_SETFL)` does, on every descriptor `fcntl`
   accepts; other requests are unchanged.
3. **Names `libc.a` adds** (the block below): the hidden `bcmp`, `pause` and `pipe2`; `sched_yield` (libposix's
   `zoraya_posix_sched_yield`); `clock_nanosleep` (`CLOCK_MONOTONIC` and `CLOCK_REALTIME`, relative and
   `TIMER_ABSTIME`, over `nanosleep`'s wait; other clocks `EINVAL`); `dirfd`; `getauxval` (musl's, over the auxiliary
   vector libposix lays out); `gettid` (the thread's kernel id, as `pthread_self`'s); `posix_spawn_file_actions_addchdir`
   (POSIX.1-2024) and its `_np` alias. `posix_spawnp` (ADR-0411) and `socketpair`, `sendmsg`, `recvmsg` (ADR-0412) are
   already accepted and come with their parts.
4. **`libzoraya-rust.a`**, built from `ports/rust/zoraya-rust.c` into the sysroot and linked only by `tools/zcargo`
   (ADR-0452), defines the names `std` references that `libc.a` keeps out, each failing with `ENOSYS`: `setuid`,
   `setgid`, `setgroups`, `setsid`, `chroot` (`CommandExt` options), `sigaltstack` (`std` then installs no alternate
   stack), `sched_getaffinity` (`std` falls back to `sysconf`), `sendfile`, `splice` (`std` falls back), `waitid`. A name
   enters it only by an ADR.
5. **The `__rustc` names** of libposix's Rust runtime (`_RNvCs..._7___rustc*`) are renamed when `libc.a` is assembled,
   by the same hiding step as the tier list (`posix-surface-check hide` stops treating them as internal), so that no
   Rust program collides with libposix.
6. **`std`'s probes** of `/proc/self/cgroup` and `/proc/self/mountinfo` (from `available_parallelism`) are `ENOENT`
   without a report or hint; `/proc/self/maps` likewise (the main thread's guard). Any other missing path is reported
   as today.

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

- **Positive.** C callers of `syscall()` get a correct `long`; `std`'s synchronisation sleeps; `HashMap`, `fs::copy`,
  `Command::output` and `connect_timeout` work; the surface grows by ten listed names.
- **Negative.** Zoraya answers three Linux system-call numbers and one Linux `ioctl` request; a Rust-only archive to keep
  in step with `std`.
- **Follow-up.** The part that builds ADR-0450 to ADR-0454; `docs/design.md`'s libposix section.

## Invariants

- **INV-0451-1.** `syscall()` returns `long` and answers exactly the numbers of item 1; any other number returns `-1`
  with `errno == ENOSYS`. *Check:* host tests in `zoraya-posix` for each answered number, for an unanswered one (the
  return read as a 64-bit `-1`, not 4294967295) and for each futex form; `posix-surface-check` reads `syscall`'s
  declared return type from the header.
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
