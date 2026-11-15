# Rust `std` for POSIX programs on Zoraya: survey and probes

The research behind proposed ADR-0450 to ADR-0455 (TASK-35): whether a program written in Rust with the standard library
can run on Zoraya as a POSIX program, built with the stable toolchain only, and what it would take. It makes no
decisions.

**How it was made.** One session on 2026-11-15 on the architect's MacBook Air M1 (8 GB), in the worktree of branch
`adr/rust-std` at `f969cf41`. The main session read Rust 1.99.1's `library/std` source (the `rust-src` component of the
pinned toolchain), built and ran the probes, and wrote this file. Two subagents (Haiku) helped: one digested Zoraya's
ADRs (every fact used here was checked by the main session in the file and line named), one fetched outside pages for
[How other systems did it](#how-other-systems-did-it) under TASK-01's rules: every URL fetched in the session, the claim
checked against the fetched text, no citation from memory. A sentence marked `(inference)` is the researcher's;
`[unverified]` rests on no fetched page and no measurement, and no ADR relies on it alone; `(estimate)` marks a number
nobody measured. A result marked **measured** was produced by a command in this session; the commands are in
[Measurements](#measurements).

**What was done to the machine.** `rustup target add aarch64-unknown-linux-musl` and `rustup component add rust-src`
for the pinned 1.99.1 toolchain (user-level toolchain components, 152 MB for the target). Nothing under `kernel/`,
`services/`, `crates/` or `user/` changed; the probes live in [`probes/`](probes/) and build into
`build/rust-std-probe/`. `RUSTC_BOOTSTRAP=1` was used once, only to print the target's specification
(`rustc --print target-spec-json`, a nightly-only option); no probe was built with it.

**What was not done.** Route B was not built (it needs nightly). No probe ran under TCG; every run was HVF on the M1,
one run each, so the times below are single measurements. No Ladybird code was built; its `Cargo.lock` was read.

## Zoraya today

What the probes run on, from the repository (ADRs accepted unless marked).

- **The personality.** POSIX programs are static `ET_EXEC` AArch64 ELF files linked by `tools/zcc` against `libc.a`:
  musl 1.2.5's retained objects plus `zoraya-posix` (libposix, Rust), with every name outside the tier lists renamed
  `__zoraya_NAME` (ADR-0141, `ports/Makefile` "musl's objects first"). Names a list does not hold are therefore absent
  at link time, which is the gate on the surface (ADR-0005, CP-27).
- **`__linux__`.** `zcc` refuses `-D__linux__` and its variants, and `uname -s` prints `Zoraya` (INV-0109-2,
  `tools/zcc`); ADR-0143 keeps `__linux__` and `__unix__` undefined for C.
- **`syscall()`** is exported, but as `not_yet!(int syscall ...)` (`crates/zoraya-posix/src/ffi.rs:2796`): it returns a
  32-bit `-1` with `errno = ENOSYS` for every call. musl's own call sites never reach it; they go through the shim of
  `ports/musl/zoraya/syscall.h`, which routes eleven calls at compile time and routes `SYS_futex` to
  `zoraya_posix_futex` (ADR-0141, ADR-0302 item and `syscall.h:97-104`).
- **The futex** (ADR-0301): `futex_wait(address, expected, deadline)` and `futex_wake`, keyed per process.
  `zoraya_posix_futex` (`ffi.rs:1417`) answers `FUTEX_WAIT` with a relative timeout, `FUTEX_WAKE`, `FUTEX_REQUEUE` and
  `FUTEX_CMP_REQUEUE` (as wakes), and `ENOTSUP` for the rest, `FUTEX_WAIT_BITSET` included.
- **The deny list** (INV-0005-2): `libposix` exports none of `setuid`, `setgid`, `sigaltstack`, `epoll_*`, `dlopen` and
  the rest of ADR-0005's exclusion table unless an accepted ADR adds it.
- **Shims beside `libc.a`** are an existing pattern: CPython's `libpyshim.a` (ADR-0320 item 3: `sched_yield`, `pause`,
  `dirfd` and others), libc++'s `zoraya-shim.c` (ADR-0366: `sched_yield`, `truncate`, `pathconf`, `link`, `statvfs`),
  NetSurf's `nsshim.c`, and the engine's always-failing `dlopen` shim (ADR-0410 item 3, accepted part).
- **Unwinding** (ADR-0360, ADR-0361): LLVM 20.1.8's `libunwind.a` in the sysroot finds `.eh_frame_hdr` through
  `PT_GNU_EH_FRAME` and musl's static `dl_iterate_phdr`.
- **Faults** (ADR-0106 item 12): no handler runs on a fault; the process ends with reason `Fault`.
- **Accepted additions not yet built** that Rust's `std` also uses: `posix_spawnp` (ADR-0411), `socketpair`, `sendmsg`
  and `recvmsg` with `SCM_RIGHTS` (ADR-0412), `poll` and `ppoll` (built, ADR-0398 proposed as-built), `eventfd`
  (ADR-0414).
- **The Rust policy.** `docs/design.md` (Platform): "Stable toolchain 1.99.1 ... No nightly features are used."
  ADR-0418 item 4 (accepted): "No Rust `std`, no Swift for POSIX programs now; Rust `std` returns with Ladybird
  (ADR-0410's revisit) or a Rust POSIX tool worth porting, as its own ADR." The architect's note on ADR-0410
  (2026-11-14) names this route as the question.

## Route A: the prebuilt musl std

Rust ships `aarch64-unknown-linux-musl` as a tier 2 target with host tools, so `rustup` gives a prebuilt `std` for it
(platform-support, [R1][R2]). The target's specification (printed, see above) says `"os": "linux"`, `"env": "musl"`,
`"crt-static-default": true`, `"position-independent-executables": true` and `"metadata": {"description": "ARM64 Linux
with musl 1.2.5"}`: the same musl release as Zoraya's `libc.a`. Its `std` is compiled with `cfg(target_os = "linux")`,
so it takes Linux's branches of `library/std/src/sys`, and its `libc` crate holds Linux's constants and structure
layouts for musl on AArch64, which are the values of musl's `arch/aarch64` headers that Zoraya's sysroot installs
(ADR-0109; the probes saw Linux's numbers throughout: `ENOSYS` 38, `ENOTTY` 25, `EACCES` 13, `ENOENT` 2, `EPERM` 1,
with `libc.a`'s own `strerror` text).

### Building on stable

Every flag below is a stable `rustc` option ([R5]); the probe's [`build.sh`](probes/build.sh) and
[`.cargo/config.toml`](probes/.cargo/config.toml) are the whole recipe.

- `--target aarch64-unknown-linux-musl`, the prebuilt `std`.
- `-C linker=tools/zcc`: `zcc` adds Zoraya's `crt1.o`, `crti.o`, `crtn.o`, `-static`, the image base and `-lc`.
- `-C relocation-model=static`: the target defaults to static-pie, which `zcc` refuses (ADR-0033); with a static
  relocation model `rustc` links `static-nopic-exe` and passes `-static -no-pie`.
- `-C link-self-contained=no`: `rustc` then adds neither its own `crt*.o` nor its own `libc.a` and `libunwind.a`;
  `-lc` and `-lunwind` (from the `libc` and `unwind` crates) find the sysroot's.
- `-C target-feature=+crt-static` (the default, stated).

`rustc` also passes `-Wl,--eh-frame-hdr`, `-Wl,--gc-sections`, `-Wl,--strip-debug` (release), `-nodefaultlibs` and
`-Wl,-z,relro,-z,now` after `zcc`'s own `-z norelro`. **Measured:** the probe links with these, the result has
`PT_GNU_EH_FRAME`, a 152-byte `PT_TLS` and a `PT_GNU_RELRO`, and `zoraya-elf-check` accepts it ("entry 0x43ef30, r--,
r-x, rw-, rw-"). The probe is 1,470,928 bytes; an incremental release build takes 2.2 s on the M1.

### What std links

Two obstacles appear at the first link (**measured**, `hello` and then the full probe):

1. **Six duplicate symbols.** libposix inside `libc.a` is a Rust static library with its own allocator shims and panic
   handler: `__rustc::__rust_alloc`, `__rust_alloc_zeroed`, `__rust_dealloc`, `__rust_realloc`,
   `__rdl_alloc_error_handler` and `rust_begin_unwind` (and two more names it defines but the program does not:
   `__rust_alloc_error_handler`, `__rust_no_alloc_shim_is_unstable_v2`). Their v0-mangled names carry the hash of
   the `__rustc` pseudo-crate, `_RNvCs5fGHjzWXRav_7___rustc...`, which is the same for every target of one compiler,
   so they collide with the program's `std`. The tier-list renaming does not touch them: `posix-surface-check`'s
   `is_internal` treats every name that starts with `_` as internal (`tools/posix-surface-check/src/surface.rs:149`).
   Every other Rust symbol of libposix carries the crate hash of its `aarch64-unknown-none` build and does not
   collide. The probe renames the `__rustc` prefix to `__zrstc` in a copy of `libc.a`, same length, so the archive's
   index stays valid (`build.sh`); 168 bytes change.
2. **22 names `libc.a` does not export**: `bcmp`, `chroot`, `clock_nanosleep`, `dirfd`, `getauxval`, `pause`, `pipe2`,
   `posix_spawn_file_actions_addchdir_np`, `posix_spawnp`, `recvmsg`, `sched_getaffinity`, `sched_yield`, `sendfile`,
   `sendmsg`, `setgid`, `setgroups`, `setsid`, `setuid`, `sigaltstack`, `socketpair`, `splice`, `waitid`. Six of them
   are implemented in `libc.a` and hidden by the tier list as `__zoraya_NAME`: `bcmp`, `pause`, `pipe2`, `sendmsg`,
   `setgroups`, `setsid`. Sixteen are absent. `hello` alone needs four: `bcmp`, `getauxval`, `pause`, `sigaltstack`.

What `std` does with each, from the source:

| Name | Where `std` uses it | Kind in the probe shim |
|---|---|---|
| `bcmp` | compiler-emitted for slice comparison | hidden, forwarded |
| `getauxval` | `AT_MINSIGSTKSZ` for the guard handler (`pal/unix/stack_overflow.rs:304`); `std_detect` reads `AT_HWCAP` | composed: `0`, `ENOENT` |
| `sigaltstack` | the stack-overflow handler of every thread (`stack_overflow.rs:258`) | `ENOSYS`; `std` then reads a zeroed `stack_t` and installs no alternate stack |
| `pause` | exit races (`sys/exit.rs:52`), a failed guard record (`thread_info.rs:107`) | hidden, forwarded |
| `pipe2` | every pipe, `O_CLOEXEC` (`sys/pipe/unix.rs:28`) | hidden, forwarded |
| `posix_spawnp`, `posix_spawn_file_actions_addchdir_np` | `Command::spawn` and `Command::current_dir` (`process/unix/unix.rs:482`, `615`) | `posix_spawnp` composed (`$PATH` search over `posix_spawn`); `addchdir_np` `ENOSYS` |
| `waitid`, `recvmsg`, `sendmsg` | pidfds (`process/unix/pidfd.rs`, `unix.rs:924`) | `ENOSYS`; hidden `sendmsg` forwarded |
| `sched_yield` | `thread::yield_now`, spin loops | composed: `nanosleep` of 0 |
| `clock_nanosleep` | `thread::sleep_until` | composed over `nanosleep` |
| `dirfd` | `fs::remove_dir_all` (the `openat` walk) | composed: libposix's `DIR` holds the fd after its 280-byte `dirent` (`ffi.rs:2124`) |
| `sched_getaffinity` | `thread::available_parallelism` (`thread/unix.rs:175`), falls back to `sysconf` | `ENOSYS` |
| `sendfile`, `splice` | `io::copy` between descriptors (`io/kernel_copy/linux.rs`), falls back | `ENOSYS` |
| `setuid`, `setgid`, `setgroups`, `chroot`, `setsid` | `CommandExt::uid`, `gid`, `groups`, `chroot`, `process_group`/`setsid` only | `ENOSYS`; hidden `setgroups`, `setsid` forwarded |
| `socketpair` | `UnixStream::pair` | `ENOSYS` |

`setuid`, `setgid` and `sigaltstack` are on ADR-0005's deny list (INV-0005-2), so they cannot come from `libc.a`
without an ADR; `sendfile` was kept out by ADR-0366.

### What std calls through syscall

`std` reaches some Linux calls not by a libc function but by its `syscall!` macro (`pal/unix/weak/syscall.rs`): it
looks the name up as a weak symbol and, if that is null, calls `libc::syscall(SYS_name, ...)`. In a static link a weak
undefined reference never pulls a member out of an archive (inference from how archive linking works; consistent with
the probe: `getrandom` is exported by `libc.a`, yet `std` reached `syscall()`), so on Zoraya every one of these goes to
`syscall()`. Read from the source:

| Call | Where | `std`'s return type | On `ENOSYS` |
|---|---|---|---|
| `futex` (`FUTEX_WAIT_BITSET` with `FUTEX_PRIVATE_FLAG`, absolute `CLOCK_MONOTONIC` deadline, full mask; `FUTEX_WAKE`) | `sys/sync/futex/unix.rs:64`, `97`, `106`: every `Mutex`, `Condvar`, `RwLock`, `Once`, park | `c_long` (direct `libc::syscall`) | a wait "returns true" at once: every wait becomes a spin |
| `getrandom` | `sys/random/linux.rs:77`: `HashMap`'s keys, `random` | `ssize_t` | falls back to `/dev/urandom`, which Zoraya does not have: panic |
| `statx` | `sys/fs/unix.rs:158`: every `metadata` | `c_int` | probes once more with null arguments, then uses `fstatat` for good |
| `copy_file_range` | `io/kernel_copy/linux.rs:696`: `fs::copy`, `io::copy` | `ssize_t` | falls back to `read` and `write` |
| `splice` | `kernel_copy/linux.rs:819` | `ssize_t` | falls back |
| `gettid` | `thread/unix.rs:344`: the thread id in panic messages | `pid_t` | the id is printed as `-1` |
| `pidfd_open`, `pidfd_send_signal` | `process/unix/pidfd.rs`, only with `CommandExt::create_pidfd` | `c_long` | an error to the caller |

Not used by `std` at all: `clone3`, `epoll_*`, `eventfd`, `memfd_create`, `prctl` outside `thread::set_name` (which
uses `pthread_setname_np` on musl), `io_uring`. `std` also reads `/proc/self/cgroup` and `/proc/self/mountinfo` in
`available_parallelism` (`thread/unix.rs:820`, `969`), `/proc/self/maps` for the main thread's guard, and
`/proc/self/exe` in `current_exe`.

**The return-type defect (measured).** libposix's `syscall` returns `int`. On AArch64 a function returning `int`
leaves `-1` in `w0`; a caller reading `long` from `x0` sees `0x00000000ffffffff`, so `std` sees 4294967295, a large
positive result, not `-1`. The probe's own call `syscall(SYS_gettid)` printed `returned 4294967295 (0xffffffff), errno
Function not implemented`. Through `std`: `HashMap::new` panicked with "range start index 4294967295 out of range for
slice of length 16" (the `getrandom` loop took 4294967295 bytes as written); `fs::copy` reported 18446744073709551615
bytes and left a wrong file; every futex wait returned at once. `statx` and `gettid`, declared `c_int` and `pid_t`
by `std`, read the low half and behave as `-1` by luck.

**Non-blocking descriptors.** On Linux `std` sets `O_NONBLOCK` with `ioctl(fd, FIONBIO, &v)` (`sys/fd/unix.rs:632`,
`net/connection/socket/unix.rs:594`), not with `fcntl`. libposix's `ioctl` answers `ENOTTY` (**measured**), and
`Command::output` unwraps the result (`read2` puts both pipes in non-blocking mode), so it panics.
`TcpStream::connect_timeout` and `set_nonblocking` fail. Close-on-exec on Linux goes through `fcntl` (the `FIOCLEX`
branch excludes Linux, `fd/unix.rs:570`), and `FIONBIO` is the only `ioctl` `std` makes.

### The probes

[`probes/src/main.rs`](probes/src/main.rs) is one program, `rprobe MODE`, that prints a line per check. It was built two
ways ([`build.sh`](probes/build.sh)):

- **today**: Zoraya's `libc.a` (with the `__rustc` rename) and the probe-local [`zshim.c`](probes/zshim.c) for the 22
  names of the table above, nothing else. This is "what works today": without a shim of this kind the program does
  not link at all.
- **fixed**: also [`zshim-fixed.c`](probes/zshim-fixed.c), linked with `-Wl,--wrap=syscall -Wl,--wrap=ioctl`, which
  emulates what [What libposix would have to add](#what-libposix-would-have-to-add) proposes: a `syscall()` that
  returns `long` and answers `futex` (`FUTEX_WAIT_BITSET` with a full mask turned into libposix's relative
  `FUTEX_WAIT`; `FUTEX_WAKE`) and `getrandom`, `ENOSYS` for the rest; `ioctl(FIONBIO)` through `fcntl`.

[`run.sh`](probes/run.sh) puts the program on a disk three times and boots the normal kernel with `tools/test-runner`:
`/bin/rprobe` for `posixrun`, `/bin/cxxtest` so that the shell runs it through the broker under cxxtest's existing
description (`user/describe/posix/cxxtest.cmd`, which takes `+r` and `+w` grants), and `/bin/zfetch` so that it runs
under zfetch's description, which grants the connection to the URL on the line (the probe takes the net mode when its
name ends in `zfetch`). Scripts: [`scripts/basic.script`](probes/scripts/basic.script),
[`scripts/more.script`](probes/scripts/more.script), [`scripts/net.script`](probes/scripts/net.script) (with
`zoraya-test-httpd` on the host). All six boots passed; each log is in `target/test-logs/rust-std-*-auto.log`.

**Measured, 2026-11-15, HVF on the M1, one run each:**

| Area | Check | today | fixed |
|---|---|---|---|
| start | `println!`, `args`, `env::vars`, `set_var`, `eprintln!`, through `posixrun` and the broker | works | works |
| start | broker messages `posixrun` counted for the run it launched | 0 | 0 |
| threads | 8 threads spawned and joined; `available_parallelism` | works; 1 | works; 1 |
| threads | 4 threads, 80,000 contended `Mutex` locks | 4.9 ms | 4.7 ms |
| threads | `Condvar` ping-pong, 1000 rounds between two threads | **37.5 s** (every wait spins until the tick) | 10.0 ms |
| threads | `Condvar::wait_timeout(200 ms)` with nobody notifying | **never times out**: 3,152,322 spurious wake-ups in 2 s | timed out after 210 ms, 0 spurious |
| threads | `thread::park_timeout(100 ms)` | **returns after 1.7 µs** | 110 ms |
| threads | `mpsc` 400 messages from 4 threads; `RwLock` and `Barrier`; scoped threads | works | works |
| threads | `Builder::name` and `stack_size`, a `thread_local!` with `Drop` dropped once at thread exit | works | works |
| hash | `HashMap::new` (random keys) | **panics** (4294967295, above) | works |
| time | `SystemTime::now` (1794708254 s), `Instant` around `sleep(50 ms)` | works (55 to 60 ms) | works (59 ms) |
| fs | `create_dir_all`, `write`, `read_to_string`, append, `seek`, `set_len`, `sync_all`, `metadata` (`len`, `is_file`, `modified`), `read_dir` with file types, `rename`, `symlink` and `read_link` and a read through the link, `set_permissions` (0o600 read back), `remove_dir_all`, `NotFound` | works in `/tmp` and in a `+w` directory | works |
| fs | `metadata().created()` | unknown (Zoraya keeps no birth time) | unknown |
| fs | `fs::copy` of 300,000 bytes | **wrong**: "copied 18446744073709551615", contents differ | works |
| fs | `hard_link` | `EPERM` (ADR-0013: no hard links) | `EPERM` |
| fs | `canonicalize` of `/tmp/rprobe/a/b/..` | `/tmp/rprobe/a` | same |
| fs | `canonicalize` inside `+w /work/granted` | `ENOENT`, hint "tried to read /work" (the parent is not granted) | same |
| fs | a directory the line does not grant (`/work/secret`) | `ENOENT` and the shell's hint and `+r`/`+w` suggestion, per path | same |
| fs | `current_dir`, `current_exe`, `temp_dir` | `/home`, `/bin/cxxtest`, `/tmp` | same |
| process | `Command::status` of the program itself with `exit(3)` | exit status 3 | 3 |
| process | `Command::output` (env, stdout, stderr) | **panics**: `ENOTTY` from `FIONBIO` | works: "out yes\n", "err\n" |
| process | `stdin` piped to a child and read back | works | works |
| process | `Child::kill` | signal 9 | signal 9 |
| process | a missing program | `NotFound` | `NotFound` |
| process | `Command::current_dir` | `ENOSYS` (no `addchdir`) | `ENOSYS` |
| process | a child that makes a `HashMap` | (not reached: `output` panics first) | **exit 101**: "failed to generate random data: ... NotFound" (a child has no random connection, INV-0411-4, and `std` falls back to `/dev/urandom`) |
| net | `ToSocketAddrs` of `10.0.2.2:PORT` and of `test.zoraya:PORT` (`net.hosts`) | works | works |
| net | `TcpStream` GET of a page; of 1,000,090 bytes by name | works; 205 ms and 46 ms in two runs | works; 51 ms |
| net | `TcpStream::connect_timeout` | **fails**: `ENOTTY` from `FIONBIO` | `EACCES` (the port is not the URL's: the grant's answer) |
| net | the same program through `posixrun`, no grant | `EACCES` | `EACCES` |
| panic | `catch_unwind` of a panic 20 frames down, 20 destructors run; `resume_unwind` with a `u32` payload; a panic in a thread seen by `join`; a poisoned `Mutex`; an index out of bounds caught | works | works |
| panic | `Backtrace::force_capture` | captured, 6 lines, no function names (release build without debug information; `/proc/self/maps` not given) | same |
| panic | an uncaught panic | message, exit status 101; the thread id prints as 18446744073709551615 (`gettid` is `ENOSYS`) | same |
| panic | `process::abort` | "ended by signal 6" | same |
| panic | unbounded recursion | "crashed (data abort, class 0x24)", no "has overflowed its stack" message (no handler runs on a fault, ADR-0106) | same |
| noise | `available_parallelism` reads `/proc/self/cgroup`: the shell prints "hint: cxxtest tried to read /proc/self/cgroup" | once per program that asks | same |

So **today**, with nothing but the probe shim, a Rust `std` program runs and gets files, processes, sockets, panics
and threads, but every blocking wait of `std`'s synchronisation spins (correct but CPU-bound: a condvar round trip
waits for the next scheduler tick), `HashMap::new` panics, `fs::copy` is wrong and `Command::output` and
`connect_timeout` panic or fail. All of those come from two causes, `syscall()`'s return type and missing answers,
and `FIONBIO`; with both emulated (**fixed**), every check passes except `Command::current_dir`, which needs
`posix_spawn_file_actions_addchdir`, a child's `HashMap` (a spawned child has no random connection), and the
platform's own refusals (hard links, ungranted paths and ports).

### What libposix would have to add

From the two sections above and the **fixed** run:

1. **`syscall()` returns `long`** and answers a fixed table: `SYS_futex` (98) with `FUTEX_WAIT`, `FUTEX_WAKE` and
   `FUTEX_WAIT_BITSET` with `FUTEX_BITSET_MATCH_ANY` and an absolute `CLOCK_MONOTONIC` deadline, each with or without
   `FUTEX_PRIVATE_FLAG`; `SYS_getrandom` (278) as `getrandom`; `SYS_gettid` (178). Every other number fails with
   `ENOSYS` (as `-1`, `long`). The emulation is 91 lines of C; in libposix it is a match in `ffi.rs` beside
   `zoraya_posix_futex` (estimate: under 150 lines with tests).
2. **`ioctl(FIONBIO)`** sets or clears `O_NONBLOCK` as `fcntl(F_SETFL)` does (Linux and the BSDs define it so; C
   programs use it too).
3. **Names to export**: the six hidden ones `std` needs (`bcmp`, `pause`, `pipe2`, `sendmsg` and, if their semantics
   are wanted, `setgroups`, `setsid`) and absent ones with a real meaning on Zoraya: `sched_yield` (libposix has
   `zoraya_posix_sched_yield`, ADR-0366), `clock_nanosleep` (over the same wait as `nanosleep`), `dirfd`,
   `getauxval` (musl's reads the auxiliary vector libposix lays out, ADR-0361), `gettid`,
   `posix_spawn_file_actions_addchdir` (POSIX.1-2024) and its `_np` alias. `posix_spawnp` (ADR-0411) and
   `socketpair`, `sendmsg`, `recvmsg` (ADR-0412) are already accepted.
4. **Names that stay out of `libc.a`** because the deny list or an ADR keeps them out (`setuid`, `setgid`,
   `sigaltstack`, `sendfile`) or because they mean nothing here (`chroot`, `sched_getaffinity`, `splice`, `waitid`):
   a small archive linked only into Rust programs, each failing with `ENOSYS` (the pattern of `libpyshim.a`).
5. **The `__rustc` names** of libposix's Rust runtime renamed when `libc.a` is assembled, so that no Rust program's
   `std` collides with them.
6. **`std`'s probes of `/proc/self/cgroup` and `/proc/self/mountinfo`** answered `ENOENT` without the shell's hint,
   or the hint stays as visible noise once per program.
7. **Randomness for spawned children.** A `posix_spawn` child has no random connection (INV-0411-4), so a Rust child
   that makes a `HashMap` panics (measured); either the child gets a connection made from its parent's, or every
   `Command` user must pass ADR-0411's grant mask, which `std` cannot.

### Crates that bypass libc

`cfg(target_os = "linux")` is true for every crate a Rust program depends on, not only `std`. Crates that issue Linux
system calls themselves would issue Linux `svc` instructions on Zoraya, whose kernel numbers its calls differently
(ADR-0002), so such a call reaches an unrelated Zoraya call or an error (inference). The known case is `rustix`,
whose default on Linux is its `linux_raw` backend over `linux-raw-sys` `[unverified: rustix's documentation was not
fetched]`. Crates that call Linux-only libc functions (`epoll_*` through `mio` and `tokio`, `inotify`, `memfd_create`,
`dlopen` through `libloading`) fail at link, because `libc.a` exports none of them, which is the surface gate working
as intended. **Measured** on the probe: one `svc` instruction in the whole program, inside
`zoraya_rt::raw_syscall` (libposix's), found by disassembling and attributing each `svc` to its function; a check of
that shape would catch a `rustix` that slipped through.

### Route A and INV-0109-2

INV-0109-2 is stated for C ("No port is compiled with `__linux__` defined"), and Route A keeps it: a Rust program's
C parts go through `zcc` (by `CC_aarch64_unknown_linux_musl=zcc`, inference: how the `cc` crate picks a compiler was
not tested), and `uname -s` still prints `Zoraya`. Its intent, that no port takes Linux branches, does not hold for
Rust code: `std` and every crate take theirs. The probes show what that costs: Linux's branches of `std` assume
`syscall()`, `FIONBIO`, `/proc/self/*`, pidfds and `statx`, and all of them either fall back or are answered by the six
items above. The constants and layouts are musl's, which is what Zoraya's headers are. What the ADRs must add is a rule
for Rust that replaces the `__linux__` rule's intent with a mechanical one: the surface Rust programs reach is
`libc.a`'s exports, the Rust-only archive and the `syscall()` table, and nothing else (no Linux `svc`).

## Route B: a Zoraya target

A target `aarch64-unknown-zoraya` with `std`'s `sys` module written for Zoraya.

- **The target specification.** A JSON target file (`os = "zoraya"`, the musl ABI or none) is the start; whether
  stable `rustc` accepts a JSON target without `-Z unstable-options` was not tested here `[unverified]`, and it would
  not matter: no prebuilt `std` exists for it.
- **`std` for it.** Either cargo's `-Z build-std`, which "is still in very early stages of development" and needs "both
  a nightly Cargo and a nightly rustc" [R3], or a Rust toolchain built from source with the target added (the
  project's own bootstrap builds `std` that way). The first breaks the no-nightly rule; the second makes Zoraya carry
  a compiler fork, rebuilt at each toolchain step (estimate: an hour of build per step on the M4, and a patch to
  rebase). `RUSTC_BOOTSTRAP=1` makes a stable compiler accept nightly options; it is the nightly rule broken by an
  environment variable.
- **The `sys` module.** `library/std/src/sys` dispatches per `target_os` in each of about twenty subdirectories
  (`alloc`, `args`, `env`, `fd`, `fs`, `io`, `net`, `os_str`, `pal`, `path`, `pipe`, `process`, `random`, `stdio`,
  `sync`, `thread`, `thread_local`, `time` and more, listed above). A Zoraya `sys` could sit on libposix like the
  `unix` one (most of the code shared by `cfg(unix)`, with `target_os = "zoraya"` branches where Linux's differ) or on
  the native ABI. The `libc` crate needs a `zoraya` module of constants and structures (estimate: 2,000 to 4,000
  lines across `std` and `libc`, two to four weeks to a passing `std` test subset).
- **Upstreaming.** A tier 3 target needs named maintainers and documentation, and "Rust provides no guarantees about
  tier 3 targets" [R4]; it still has no prebuilt `std`, so it still needs build-std. Tier 2, which ships `std` by
  `rustup`, needs "at least 2 developers", reliable CI in Rust's CI and no significant stubs [R4]. Precedents:
  Fuchsia's and Redox's x86-64 targets are tier 2; Redox on AArch64 and Hermit are tier 3 [R1].
- **Ecosystem crates** would not know `target_os = "zoraya"`: `libc`, `getrandom`, `socket2`, `parking_lot_core`,
  `mio` would each need a port or a patch in every lock file (inference).

## Route C: no std

Native Zoraya programs are already Rust without `std` (`no_std` with `zoraya-rt` and `alloc`), so a window program built
outside the repository can be written the native way today, against the native ABI, without POSIX at all. That is a
different kind of program: it uses no crate that needs `std`.

Ladybird's Rust cannot take this route. Its 1,018,566 lines of Rust in 31 local crates
(browser-engine/survey.md#ladybird) depend, through `Cargo.lock`, on crates such as `tempfile`, `getrandom`,
`parking_lot_core`, `socket2`, `crossbeam-epoch` and `tokio` (next section), all `std` crates. Porting it to `no_std`
is a rewrite of the engine's Rust half, not a port (estimate).

## How other systems did it

From pages fetched on 2026-11-15; times to port were not found in any of them.

- **Redox.** `x86_64-unknown-redox` is tier 2; `aarch64-unknown-redox` tier 3 [R1]. The target page: "In addition a copy
  of relibc needs to be present in the linker search path" and "Std is fully supported" [R6]. Its own target over its
  own libc in Rust: Route B with a libc Zoraya rejected (ADR-0100). The postmortem's Redox section
  (alternatives-postmortem.md#redox) describes the shim living in relibc in user space.
- **Fuchsia.** `x86_64-unknown-fuchsia` and `aarch64-unknown-fuchsia` are tier 2; "Fuchsia targets support `std`" [R7],
  maintained by five named Google engineers. Route B, upstreamed, by the system's vendor. Fuchsia also runs Linux
  binaries unmodified through Starnix, a user-space Linux-ABI runner (alternatives-postmortem.md#fuchsia--zircon-incl-starnix):
  for Linux programs, Fuchsia chose to answer Linux's interface rather than to port them.
- **Hermit.** All four `-unknown-hermit` targets are tier 3 with `std` [R1]; maintained by two people [R8].
  Route B without prebuilt `std`.
- **illumos and Solaris.** `x86_64-unknown-illumos` and `x86_64-pc-solaris` are tier 2 with host tools [R1]; "The
  illumos target supports `std`" [R9]. Each is a Unix whose libc is its own interface, with maintainers from the
  system's companies.
- **Theseus.** No Rust target; its `tlibc` is "a work in progress and currently a proof-of-concept library" [R10].
  No source fetched shows `std` on Theseus.
- **SerenityOS.** No Rust target in the platform table [R1]; no Rust port directory could be fetched (404). Ladybird,
  which came from it, builds its Rust on Linux and macOS only (browser-engine/survey.md#ladybird).
- **Linux compatibility layers.** FreeBSD's Linuxulator runs "unmodified Linux binaries" but needs a Linux userland and
  is "not enabled" by default [R11]; WSL 1 "used a translation layer" without "full system call compatibility" [R12].
  These answer the whole Linux ABI; Route A answers only the calls one library makes, chosen and listed (inference).

The pattern (inference): every system with a tier 2 `std` of its own has two or more maintainers paid to keep it in
Rust's CI; small systems without that are tier 3 and build `std` with nightly. No source showed a system that used
the musl target's prebuilt `std` over its own libc, as Route A proposes.

## Ladybird's Rust

Read from `Cargo.lock` at `8ab2c0bb9c43` (the commit TASK-33 measured; SHA-256
`37c0896cdd41d1e46ac7de3636bf9e4ecafc630611079aa28268028a104c39b0`, 433 packages, 31 local), and its
`rust-toolchain.toml` (`channel = "1.98.0"`). **Measured** by walking the lock's dependency lists:

- `rustix` 1.1.4 and `linux-raw-sys` 0.12.1 are reached only through `tempfile` 3.27.0, which only `cbindgen` uses;
  `cbindgen` generates C headers at build time (inference: a host tool, so `rustix` runs on the build machine, not
  in Zoraya; the lock file does not mark build dependencies).
- `tokio` 1.54.1, `mio` 1.2.4 (`epoll` on Linux), `nix` 0.26.4 and `libloading` 0.8.9 (`dlopen`) are reached only from
  `ladybird_webrtc` and `webrtcclient_service`.
- `libc` 0.2.190, `getrandom` (0.2.17, 0.3.4, 0.4.2), `socket2`, `parking_lot_core`, `crossbeam-epoch` and `errno`
  are ordinary `std` crates over libc.
- A local crate is named `libwasm_jit`; with the JIT off (ADR-0410 item 4's rule) it must not be built (not read
  further).

So a Ladybird build without WebRTC needs no crate that bypasses libc in the target (inference from the lock), and its
Rust parts would link against the surface of Route A. Whether WebRTC can be left out of a build was not read.

## Measurements

From the repository root, on the M1, after `make -f ports/Makefile sysroot cxx` (with `build/ports/src` copied from
another clone's verified cache), `make build/zoraya-kernel.elf target/debug/zoraya-test-runner tools-storage` and
`cargo build -p zoraya-test-httpd -p zoraya-elf-check`:

```
NM=llvm-nm docs/research/rust-std/probes/build.sh
for v in today fixed; do for s in basic more net; do
  VARIANT=$v docs/research/rust-std/probes/run.sh docs/research/rust-std/probes/scripts/$s.script
done; done
grep -a '^rprobe: ' target/test-logs/rust-std-*-auto.log
llvm-objdump -d build/rust-std-probe/rprobe-today        # svc instructions, by function
llvm-nm -A build/sysroot/usr/lib/libc.a | grep 7___rustc  # the colliding names
```

The 22 missing names come from the linker's `undefined symbol` lines of the first link without `zshim.o`; "hidden"
means `llvm-nm` shows `T __zoraya_NAME` in `libc.a`. The `std` source read is the toolchain's `rust-src` component,
`library/std/src/sys/`, at the line numbers given.

## Sources

- [R1] Rust, "Platform Support", https://doc.rust-lang.org/nightly/rustc/platform-support.html, primary: the tiers of
  the targets named; "Tier 3 targets are those which the Rust codebase has support for, but which the Rust project
  does not build or test automatically."
- [R2] Rust, "aarch64-unknown-linux-musl",
  https://doc.rust-lang.org/nightly/rustc/platform-support/aarch64-unknown-linux-musl.html, primary: "Tier: 2"; "This
  target is distributed through `rustup`".
- [R3] Cargo, "Unstable Features" (build-std), https://doc.rust-lang.org/cargo/reference/unstable.html, primary: "still
  in very early stages of development"; "You must use both a nightly Cargo and a nightly rustc".
- [R4] Rust, "Target Tier Policy", https://doc.rust-lang.org/nightly/rustc/target-tier-policy.html, primary: tier 3
  needs designated target maintainers; "Rust provides no guarantees about tier 3 targets"; tier 2 "must have at least
  2 developers"; "must build reliably in CI".
- [R5] Rust, "Codegen Options", https://doc.rust-lang.org/rustc/codegen-options/index.html, primary:
  `link-self-contained` "controls whether the linker will use libraries and objects shipped with Rust"; `crt-static`.
- [R6] Rust, "redox", https://doc.rust-lang.org/nightly/rustc/platform-support/redox.html, primary.
- [R7] Rust, "fuchsia", https://doc.rust-lang.org/nightly/rustc/platform-support/fuchsia.html, primary.
- [R8] Rust, "hermit", https://doc.rust-lang.org/nightly/rustc/platform-support/hermit.html, primary; hermit-rs README,
  https://github.com/hermit-os/hermit-rs.
- [R9] Rust, "illumos", https://doc.rust-lang.org/nightly/rustc/platform-support/illumos.html, primary; "solaris",
  https://doc.rust-lang.org/nightly/rustc/platform-support/solaris.html.
- [R10] Theseus book, "tlibc", https://www.theseus-os.com/Theseus/book/c/tlibc.html, primary.
- [R11] FreeBSD Handbook, "Linux Binary Compatibility", https://docs.freebsd.org/en/books/handbook/linuxemu/, primary.
- [R12] Microsoft, "Comparing WSL Versions", https://learn.microsoft.com/en-us/windows/wsl/compare-versions, primary.
- [R13] Ladybird, home page, https://ladybird.org/, fetched 2026-11-15: "Our first Alpha for macOS and Linux is coming
  later this year." Newest post listed: "This Month in Ladybird - October 2026" (2026-10-31).
- [R14] Ladybird `Cargo.lock` and `rust-toolchain.toml` at `8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e`,
  https://raw.githubusercontent.com/LadybirdBrowser/ladybird/8ab2c0bb9c4376f50bbcb7ea6f1b7717b089ff6e/Cargo.lock, primary.

Could not be fetched: the x86-64 musl target page (404), a Theseus target page (none), SerenityOS `Ports/rust` (404),
relibc on GitLab (403), illumos LX zone pages (404).
