# ADR-0100: How `libposix` is built

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0003, ADR-0005, ADR-0013, ADR-0014, ADR-0044, ADR-0052, ADR-0057
- **Evidence:** CP-01, CP-13, CP-15, CP-27, CP-28, DM-048, DM-057, DM-060, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, alternatives-postmortem.md#redox, alternatives-postmortem.md#summary-across-systems, stage-6/sources.md#libc-implementations, task-02/libc-surface.csv, task-02/method.md#known-limits, task-02/method.md#pinned-inputs, task-02/posix-surface.md#jumps-between-tiers, task-02/posix-surface.md#key-observations, task-02/posix-surface.md#linux-syscall-surface-m2, task-02/posix-surface.md#per-category-min-upper--lower

## Context

ADR-0002 puts the POSIX data path in a per-process library, `libposix`, and ADR-0005 fixes its surface: the `T0` and
`T1` rows of `task-02/libc-surface.csv`, 248 symbols, minus the set-uid rows, plus `writev`, exercised by W1 to W3,
W5, W2-j, W3-j and W8 to W10. This ADR decides how that library is made: from which code, in which language, how it is
built, and how INV-0005-1 to INV-0005-3 are checked against it. ADR-0103 to ADR-0108 decide what the OS-facing
functions do; this ADR decides where they live and what surrounds them.

**What is measured.**

- The surface by category, from `task-02/libc-surface.csv` (the 248 `T0` and `T1` rows): string 53, process 51,
  stdio 43, file 40, dir 7, signal 12, time 11, other 11, memory 6, tty 6, locale 4, threads 3, network 1. Of these,
  185 were seen called by program code (the `dynamic` column), 63 are imports only. The categories are a choice of the
  pipeline (`task-02/posix-surface.md#per-category-min-upper--lower`), the totals are not.
- Roughly half of the rows are pure computation with no system call behind them: the string, locale, most stdio,
  `qsort`, `bsearch`, `setjmp`, `strtod`, the `mb*`/`wc*` functions, the time formatters. The other half touch the
  machine: files, directories, processes, signals, waits, memory, the terminal.
- The T1 programs are single-threaded; tcc's three `sem_*` calls synchronise nothing (`task-02/posix-surface.md#key-observations`,
  item 3).
- Symbols reached only through `dlsym` are invisible to the static bound; `dlopen` is imported by make (`load`) and
  tcc (`-run`) and never called (`task-02/method.md#known-limits`).
- musl's own source (`stage-6/sources.md#libc-implementations`, [M3], [M4]) puts the machine boundary in two small
  files: `arch/aarch64/syscall_arch.h` (about 78 lines, the `svc` instruction and the register convention) and
  `src/internal/syscall.h` (about 400 lines, the `syscall()` macro and the `SYS_*` naming). Everything above them is
  C that calls `syscall(SYS_x, ...)` or a wrapper.

**What is assumed.** Line counts for a Rust layer are estimates. No part of musl or relibc has been compiled for Zoraya.

**Forces from TASK-01.** Every alternative OS that hosted real software built a compatibility layer
(`alternatives-postmortem.md#summary-across-systems`). Fuchsia's libc is musl with the Linux system-call layer replaced
by calls into fdio and Zircon, and Starnix is the other route, a Linux system-call emulator under unmodified binaries
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`; the musl basis of Fuchsia's libc is
`[outside evidence: fuchsia.dev libc page, not fetched]`). Redox wrote relibc in Rust with a platform abstraction
(`alternatives-postmortem.md#redox`, [R1], [R4]). QNX hosts POSIX through its own libc over messages
(`alternatives-postmortem.md#qnx`). Where the stable boundary lies, the system calls or the libc, decided what Linux
and the BSDs could change later (DM-057). C libc internals with hidden global state are a documented source of bugs
(DM-060), and a memory-unsafe language in the trusted base is DM-048; `libposix` is not in the kernel's trusted base
(CP-15), but it is in every POSIX process.

## Decision drivers

1. **The surface is a fixed list, checked mechanically** (CP-27, INV-0005-1, INV-0005-2): the build must be able to
   say what it exports.
2. **Half of the work is pure C library code that exists, is correct and is MIT-licensed** ([M1], [M2]); rewriting
   `printf`, `strtod`, `regcomp`, `strftime` and the multibyte functions is cost with no measured benefit.
3. **The other half is the mapping onto native handles, which does not exist anywhere** and is where the design of
   ADR-0101 to ADR-0108 lives; it should be in the language the rest of the system is written in, with host tests,
   as every other `no_std` crate in this repository is.
4. **The native boundary is handles and protocols, not Linux system calls** (ADR-0002 rule 1, CP-01): emulating
   `openat(AT_FDCWD, path)` and `clone` under an unchanged musl would reintroduce a Linux-shaped layer that then
   has to be mapped again.
5. **No nightly Rust and no external crate without a record** (`docs/dependencies.md`, CP-13's stable toolchain).
6. **Licensing stays permissive**: the repository is BSD-2-Clause and CLAUDE.md allows MIT, Apache-2.0, BSD, ISC and
   Zlib code.

## Considered options

### Option A: Unmodified musl over a Linux system-call emulator (baseline, Starnix-shaped)

- **Description:** `libposix` is a Linux-ABI layer: musl is built for `aarch64-linux-musl` as it is, and a Zoraya
  component implements the 49 T1 Linux system calls (`task-02/posix-surface.md#linux-syscall-surface-m2`) either in
  the process, by linking a `syscall()` replacement, or outside it, by trapping `svc` and emulating in a runner as
  Starnix does.
- **Evidence for:** No musl patch at all; any Linux-musl binary of the pinned programs could run unmodified, which is
  what forced Fuchsia to build Starnix (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). The system-call
  list is measured and finite (49).
- **Evidence against:** It is the Linux ABI in disguise: `openat` with `AT_FDCWD` and a path, `clone`, `rt_sigaction`,
  `wait4` and `ioctl` must each be mapped onto handles, pids and the broker, which is the same work as mapping the
  libc functions, plus a second layer of Linux semantics to get right (DM-057: the system-call layer is the harder
  boundary to keep). Trapping `svc` needs exception channels, which ADR-0004 rule 5 defines and nothing has built, and
  the Starnix RFC itself expected the trapping design to be too slow
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). Binary compatibility is not a goal: the programs are
  rebuilt (ADR-0005 pins their versions), and `uname`, `/proc` and `/dev` would have to lie.
- **Cost:** *(estimate)* 49 system calls to emulate, plus the Linux `struct stat`, `dirent64`, `termios` and signal
  frame layouts to honour; the hard parts of Starnix (signals, process groups) in full.
- **Precedent:** Starnix (active; performance work still listed five years in), WSL1 (cited by the Starnix RFC as a
  warning).

### Option B: musl ported, its system-call layer replaced by a Rust crate (Fuchsia's libc shape)

- **Description:** musl's machine-independent C is kept and compiled by the cross toolchain of ADR-0109: `string`,
  `stdio`, `stdlib`, `ctype`, `multibyte`, `locale`, `time` (formatting and arithmetic), `math`, `regex`, `search`,
  `prng`, `passwd`, `misc` (`getopt`, `uname` formatting), `setjmp`, `errno`, `env` (startup). Its `src/internal/syscall.h`
  is replaced by a header that routes the few `syscall(SYS_x, ...)` uses inside the retained files (stdio's `readv`,
  `writev`, `lseek`, `close`, `fcntl`, `ioctl(TIOCGWINSZ)`, `clock_gettime`) to functions of a Rust crate. Every
  OS-facing public function (`open`, `read`, `fork`, `posix_spawn`, `waitpid`, `kill`, `sigaction`, `pselect`,
  `pipe`, `dup2`, `mmap`, `malloc`, `opendir`, `tcsetpgrp`, ...) is implemented in that Rust crate, `zoraya-posix`
  (`no_std`, `extern "C"`, built as a static library), on native handles and the protocols of ADR-0052, with the fd
  table and the port inside it. The two are archived into one `libc.a` with musl's headers as the sysroot's
  `/usr/include`.
- **Evidence for:** Driver 2 and driver 3 at once: the pure C stays, the mapping is Rust with host tests. It is the
  shape Fuchsia shipped for its own programs (musl above, fdio below, `[outside evidence: fuchsia.dev libc page, not
  fetched]`). The cut is at a boundary musl already has
  ([M3], [M4]): the retained files call a dozen `SYS_*` numbers, each a one-line route. The export list is the
  archive's symbol table, which a host tool can read and diff (driver 1). MIT (driver 6).
- **Evidence against:** Two languages in one library and a C cross compiler on the host (ADR-0109). musl's internals
  assume Linux in places that are not system calls: `__init_libc` reads an auxiliary vector ([M8]), `stdio` assumes
  `readv`/`writev` on an integer fd, `malloc` wants `mmap`/`brk`. Each is a small adaptation, but each is a patch to
  carry across musl versions. The C half keeps DM-060's hidden state (`strtok`, `localtime`), which the hosted programs
  use anyway. Memory safety is only in the Rust half.
- **Cost:** *(estimate)* Rust: the 133 rows of the machine-facing categories (process 51, file 40, signal 12, dir 7,
  tty 6, memory 6, time 11) plus `read`, `write` and `close` from stdio, less the rows that are pure and stay musl C
  (`memcpy`, `memcmp`, `memset`, `getopt`, `getopt_long`, `getenv`, `putenv`, `setenv`, `atexit`, `sigemptyset`,
  `sigaddset`, `sigfillset`, the four `dl*` rows, and six time formatters and converters): about 110 functions
  `[estimate]` on top of the fd table, resolver, startup and `malloc`: 6,000 to 9,000 lines with tests.
  C: about 150 musl source files compiled unchanged plus a dozen patched; a shim header of about 100 lines; `crt1`
  replaced (ADR-0109). Build: one archive, one generated header set.
- **Precedent:** Fuchsia's libc, which the postmortem shows shipping on Nest Hubs
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); that it is musl over fdio is
  `[outside evidence: fuchsia.dev libc page, not fetched]`, as in Context.

### Option C: relibc with a Zoraya platform layer (Redox precedent)

- **Description:** relibc, Redox's libc in Rust, implements a `Pal` trait, "a platform-agnostic abstraction over
  syscalls", with `open`, `read`, `fork`, `execve`, `pipe2`, `waitpid`, `setpgid`, `dup2`, `fcntl`, `mmap` and the
  rest as methods ([R4]). `libposix` is relibc with `src/platform/zoraya` implementing `Pal` on native handles.
- **Evidence for:** One language. The abstraction point is the right one for Zoraya's shape, and Redox proves a Rust
  libc can host a POSIX userland and a desktop (`alternatives-postmortem.md#redox`). MIT ([R2]).
- **Evidence against:** `Pal`'s methods take Linux-shaped integer fds and `mode_t`; `fork` is a method, so the
  process model of ADR-0003 and the fd table of ADR-0103 would be built *under* relibc's assumptions rather than in
  the open. relibc is "under heavy development" ([R1]), depends on crates and nightly features that
  `docs/dependencies.md` would have to record and that `rust-toolchain.toml`'s stable channel may not build
  `[outside evidence: relibc's Cargo.toml and lib.rs, not fetched]`, and Redox's `redox-rt` carries the POSIX process
  logic Zoraya would replace. Redox is not self-hosting after ten years (`alternatives-postmortem.md#redox`), which is
  a warning about the maturity of its libc as much as its kernel.
- **Cost:** *(estimate)* A `Pal` implementation of comparable size to Option B's Rust half, plus an audit and a
  pinned fork of a large moving dependency, plus whatever its nightly features cost on stable.
- **Precedent:** Redox (active, incomplete).

### Option D: A new C-ABI libc in Rust (own; QNX wrote its own libc)

- **Description:** Every one of the 248 functions is written in Rust under `extern "C"`, including `printf`,
  `strtod`, `regcomp`, `strftime`, `qsort` and the multibyte functions.
- **Evidence for:** One language, no C cross compiler needed for the library itself, memory-safe throughout, and no
  third-party code to track. QNX's libc is its own and has hosted conventional software for decades
  (`alternatives-postmortem.md#qnx`).
- **Evidence against:** Driver 2: the pure half is large, standardised and already exists under MIT; a new
  `printf`/`strtod`/`regex` is months of work and a source of conformance bugs that W1 to W3 would find late
  (configure scripts and makefiles exercise `printf` and `sed` regexes heavily, `task-02/posix-surface.md#jumps-between-tiers`).
  It would still need a C cross compiler for the ported programs.
- **Cost:** *(estimate)* 20,000 lines or more `[assumption]`; the formatters and `regex` dominate.
- **Precedent:** QNX (a company, decades); relibc (ten years and not finished).

## Recommendation

**Recommendation (not a decision): Option B.** The pure half of POSIX is a solved problem with a permissive licence; the
machine half is Zoraya's design and belongs in Rust with host tests. Concretely:

1. **Layout.** `ports/musl/` holds the pinned musl source (1.2.5, the version TASK-02 measured against,
   `task-02/method.md#pinned-inputs`) fetched as ADR-0109 fetches every port, and `ports/musl/zoraya/` the patches and
   the shim header. `crates/zoraya-posix/` is the Rust crate: `no_std`, `#![forbid(unsafe_code)]` outside its `ffi`
   module, `crate-type = ["staticlib"]`, every public symbol `extern "C"` and listed in one `exports.rs`. Its logic
   (fd table, resolver, signal state, `pselect` set building, `stat` synthesis) is host-tested; only its `ffi` module
   and the system-call stubs of `zoraya-rt` need the machine.
2. **The cut.** Retained from musl, compiled unchanged: `src/{string,ctype,multibyte,locale,stdlib,math,regex,search,
   prng,setjmp,errno,misc,passwd,fenv,complex}` and the pure parts of `src/{stdio,time,env,exit}`. Replaced by Rust:
   `src/{unistd,fcntl,stat,dirent,process,signal,select,termios,mman,sched,thread,ipc,network,linux,ldso,malloc,
   aio,mq,legacy}` and the system-call-making files of `stdio`, `time` and `env`. `src/internal/syscall.h` becomes a
   shim that maps each `SYS_*` the retained files use to a `zoraya_posix_*` function (`readv`, `writev`, `lseek`,
   `close`, `fcntl`, `ioctl`, `clock_gettime`, `getpid`, `exit_group`, `set_tid_address`, and `mmap` for the one use
   in `__init_tls`), and no other `SYS_*` is defined, so a retained file that needs another fails to compile rather
   than silently calling Linux. Two files of the replaced directories are retained as exceptions because musl's
   start-up needs them: `src/env/__init_tls.c` and `src/thread/aarch64/__set_thread_area.s`.
3. **`malloc`.** `zoraya-heap` behind `malloc`, `calloc`, `realloc`, `free`, over memory objects, as `user/src/heap.rs`
   already does for Rust programs (ADR-0080); the arena grows by mapping another object, never by `brk`.
4. **Startup.** `crt1.o` is Zoraya's (ADR-0109 item 4, which fixes the layout): it reads the startup block of
   ADR-0042, lays out `argc`, `argv`, `envp` and the auxiliary vector ADR-0109 lists on the stack in the form musl's
   `_start_c` expects ([M7], [M8]), initialises `zoraya-posix` from the bootstrap messages of ADR-0107, and calls
   `__libc_start_main`. musl's `__init_libc` then runs unchanged.
5. **Threads.** `sem_init`, `sem_post`, `sem_wait` are single-threaded no-ops that keep a count (ADR-0005 keeps them
   in scope). No `pthread_*` is exported (INV-0005-2). musl's TLS set-up (`__init_tls`, retained as item 2 says) is
   kept, since `errno` and `stdio` locks are thread-local in musl and the one thread needs a TLS block: it walks the
   program headers from `AT_PHDR` with the stride `AT_PHENT` for `PT_TLS`, uses its built-in block when the TLS fits
   and `SYS_mmap` otherwise ([M10]), which the shim routes to a memory object; `__set_thread_area` is
   `msr tpidr_el0` on AArch64 and needs no system call.
6. **Stubs.** Rows that are imported but never called by any T1 workload are exported with a documented return:
   `fgetxattr`, `flistxattr`, `fsetxattr` return `ENOTSUP` (ADR-0014 rule 8); `tee`, `syscall` and `mprotect` return
   `ENOSYS`; `getloadavg` reports zero load; `getrlimit` and `setrlimit` report fixed limits; `linkat` is `publish`
   for the one form ADR-0010 and ADR-0013 rule 8 allow (`AT_EMPTY_PATH` on an unnamed file) and `EPERM` otherwise
   (no hard links); `vfork` is `fork`; `fchown`, `fchownat` succeed and change nothing (ADR-0014 rule 5).
   Each is marked `static-only` in the tier list (INV-0005-3).
7. **The export check** (INV-0005-1, INV-0005-2). A host tool, `tools/posix-surface-check`, reads `libc.a`'s members
   with its own `ar` reader (an archive is a header per member) and `ET_REL` symbol-table reader, and lists defined
   global symbols. The tier list is
   generated into `docs/adr/posix-surface-T1.txt` from `task-02/libc-surface.csv`: the `T0` and `T1` rows, minus
   `setuid` and `setgid` (ADR-0005), minus `dlopen`, `dlsym`, `dlclose` and `dlerror` (the ports are built without
   them, item 8, and `dlopen` is on INV-0005-2's deny list), plus `writev`: 248 − 2 − 4 + 1 = 243 names. Symbols
   beginning with `__` or `_` are musl internals and are not exports for this check; every other defined global
   symbol in the archive must be in the list, and the check runs under `make test`. Data symbols (`environ`, `stdin`,
   `stdout`, `stderr`, `optarg`, `optind`) are in the list and in the archive. The tier rows that begin with `_`
   (`_exit`, `__assert_fail`, `__errno_location`) are outside this check and are checked by the link of the ports.
   The tool needs an `ar` reader and an `ET_REL` symbol-table reader, which `zoraya-elf` (program headers of
   `ET_EXEC` only) does not have; both are small and are part of the tool, not of `zoraya-elf`.
8. **`dlopen` and `-run`.** tcc is built with `CONFIG_TCC_STATIC`, under which it defines its own dummy `dlopen` and
   `dlsym` `[outside evidence: tcc's libtcc.c, not fetched for this line]`, and GNU make with `--disable-load`, so
   neither links the `dl*` symbols and `libposix` does not export them.
9. **Errors.** Every native `Status` has one `errno` in a table in `zoraya-posix` (`NotFound` → `ENOENT`,
   `AccessDenied` → `EACCES`, `WouldBlock` → `EAGAIN`, `PeerClosed` → `EPIPE`, `LimitReached` → `EMFILE` or `EAGAIN`
   by call, `NoMemory` → `ENOMEM`, `StorageFailed` → `EIO`, `NoSpace` → `ENOSPC`, `TooManyOpen` → `ENFILE`,
   `BadState` → `EBUSY`, `InvalidArgument` → `EINVAL`, `BadAddress` → `EFAULT`, `BadHandle` → `EBADF`), stated once
   and tested on the host.

**Tensions with accepted ADRs.**

- *ADR-0005 INV-0005-1*: its generator rule is "the CSV rows of the accepted tier, minus the struck set-uid rows, plus
  `writev`"; item 7 strikes four more rows, `dlopen`, `dlsym`, `dlclose` and `dlerror`. ADR-0005 lists `dlopen` both
  as a `T1` row and on INV-0005-2's deny list, so its "disjoint by construction" did not hold; item 7 makes it hold.
  Accepting this ADR changes INV-0005-1's generator rule by those four names.
- *ADR-0033*: nothing changes in its rule; the loader's acceptance of what tcc links is ADR-0108's and ADR-0109's.
- *CLAUDE.md's dependency rule* speaks of crates; third-party C in the build is a new kind of dependency, recorded in
  `docs/dependencies.md` under its own table (ADR-0109 item 10).

**Why B.** A over B: A pays for Linux compatibility nobody asked for and maps two layers instead of one. C over B: C
would be the right answer if relibc were a stable dependency on stable Rust with a handle-shaped platform layer; it is
none of the three today, and its `fork` method shows the shape. D over B: D is Option B's Rust half plus a rewrite of
the half that needs no design.

**Principles this recommendation strains.** CP-15 and DM-048: C code runs in every POSIX process. It is not in the
kernel's trusted base, and the processes it runs in are the hosted programs, which are C anyway.

### Effect of the vision

Target 1 and 3: no change. Target 2, untrusted code: `libposix` runs inside the sandboxed process and is never trusted
by anything outside it, so its language matters only for the sandboxed program's own robustness; what the kernel and
services check is the same whatever the library does. Threads: musl's retained code is thread-safe by construction
(it is a threaded libc), and the Rust half would gain a lock per table; nothing here precludes T3. Networking:
`src/network` is on the replaced list, so sockets would be Rust functions over a native socket object when CP-28 is
decided. Moving to Option D later is replacing C files one by one behind the same archive and the same export check;
nothing changes for the programs.

### Deferred to a later stage

T2's 43 rows (`regcomp`, `getpwuid`, `mkdtemp`, `popen`, `getline`, ...) are mostly retained musl code and three Rust
functions; nothing in the cut changes for them. A dynamic loader (`ET_DYN`, ADR-0033) is not part of this design.

## Consequences

- **Positive.** The surface is the archive's symbol table, checked against the CSV in CI. Most of the libc is
  existing, conformant code. The part Zoraya designs is Rust with host tests, and ADR-0103 to ADR-0108 describe it.
- **Negative.** A C cross compiler is a build dependency of the repository (ADR-0109). Two languages in one archive.
  musl patches to carry. musl's version is pinned to the measured one; moving it is a decision.
- **Follow-up.** `docs/dependencies.md` gains musl 1.2.5 (MIT, with the exceptions of [M2]) and the host C compiler.
  The export tool. The `errno` table. ADR-0109's `crt1`.

## Invariants

- **INV-0100-1.** Every defined global symbol of `libc.a` whose name does not begin with `_` is in
  `docs/adr/posix-surface-T1.txt`, and that file is generated from `task-02/libc-surface.csv` by the rule in item 7.
  *Check:* `tools/posix-surface-check`, run by `make test`, fails on an extra symbol or a stale generated list.
- **INV-0100-2.** No retained musl source file reaches a Linux system call: the shim header defines only the
  `SYS_*` names item 2 lists, each routed to a `zoraya_posix_*` function, and the archive contains no `svc`
  instruction outside `zoraya-rt`'s stubs. *Check:* the build fails on an undefined `SYS_*`; a host test in
  `tools/posix-surface-check` scans the code sections of the archive's `ET_REL` members for the encoding of `svc #0`
  (`0xd4000001`) and finds it only in the one object that `zoraya-rt` contributes.
- **INV-0100-3.** `zoraya-posix` has no `unsafe` outside its `ffi` module, and its logic crates compile and test on
  the host. *Check:* `#![forbid(unsafe_code)]` on every module but `ffi`, enforced by the compiler, and
  `cargo test -p zoraya-posix`.
- **INV-0100-4.** Every native `Status` value maps to exactly one `errno` per call, from one table. *Check:* a host
  test that iterates every `Status` variant through the table and asserts a mapping exists.

## Open questions

- Whether Apple's and Linux distributions' clang build musl's retained files for a bare `aarch64` ELF target without
  further patches: the first stage 6 task finds out (ADR-0109).
- The exact list of musl files that make a system call in the retained directories is known only once the shim header
  exists: the compiler reports each missing `SYS_*`.
- Whether `__init_libc`'s `AT_SECURE` path (polling fds 0 to 2) should be compiled out rather than fed a zero auxiliary
  entry.

## Revisit triggers

- relibc on stable Rust with a handle-shaped platform layer: reconsider Option C.
- A measured defect rate in the retained C that the Rust half does not have: move files to Rust (Option D, piecewise).
- A need to run Linux binaries unmodified: Option A as a separate component, not a change to this library.

## Architect's decision

