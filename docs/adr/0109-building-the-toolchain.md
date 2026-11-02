# ADR-0109: Building the toolchain

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0001, ADR-0002, ADR-0003, ADR-0005, ADR-0031, ADR-0033, ADR-0042, ADR-0100, ADR-0104, ADR-0107, ADR-0108
- **Evidence:** CP-03, CP-13, DM-007, DM-057, GR-010, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#redox, stage-6/sources.md#libc-implementations, stage-6/sources.md#tinycc, task-02/method.md#pinned-inputs, task-02/method.md#tiers-runs-and-sets, task-02/posix-surface.md#key-observations

## Context

ADR-0005's T1 is dash 0.5.13.5, GNU make 4.4.1, TinyCC at the pinned mob commit, toybox 0.8.15's T1 applets and busybox
1.38.0's `tr` and `diff`, compiled against `libposix` (ADR-0100) as static AArch64 ELF (ADR-0033), and run inside
Zoraya. tcc inside Zoraya must then compile and link programs that run there (W1 builds tcc itself). The repository has
no C compiler and `make test` must not need the network once the toolchain is installed (CLAUDE.md). This ADR decides
the target triple, the sysroot, how the programs are cross-compiled on the host, how tcc produces Zoraya executables
and is itself built inside Zoraya, and how third-party sources enter the build.

**What is measured.**

- TASK-02 built every program with gcc against musl on Alpine, dynamically, with `-O2 -g -U_FORTIFY_SOURCE`, and
  pinned the versions and the SHA-256 of every archive in `tools/posix-surface/versions.env` and `sources.lock`
  (`task-02/method.md#pinned-inputs`). W1 is `sh ./configure --cc=tcc --config-musl` then `make` in tcc's tree; W2
  is Lua's `make CC=tcc AR="tcc -ar rcu" RANLIB=true posix`; W3 compiles SQLite's two files with tcc
  (`task-02/method.md#tiers-runs-and-sets`). GNU hello's `configure`, run with `CC=tcc`, reports
  `aarch64-unknown-linux-gnu` through `config.guess` under musl (`task-02/posix-surface.md#key-observations`, item 7);
  tcc's own `configure` has no `config.guess` ([T3]).
- tcc: "generates machine code for i386, x86_64, arm, aarch64 or riscv64", with `arm64-gen.c`, `arm64-link.c` and
  `arm64-asm.c`; its own linker; `-ar` as an archiver; `-static`, `-B`, `-nostdlib`, `-Wl,--oformat`; `configure`'s
  `--cpu=`, `--targetos=`, `--crtprefix=`, `--sysincludepaths=`, `--libpaths=`, `--elfinterp=`, `--enable-cross`;
  `libtcc1.a` for arm64 is `lib-arm64.o`, `stdatomic.o`, `atomic.o`, `builtin.o`, `alloca.o`, `alloca-bt.o`,
  `armflush.o`, `dsohandle.o`, buildable by the host compiler with `arm64-libtcc1-usegcc=yes`; the linker adds
  `crt1.o`, `crti.o`, `crtn.o`, `-lc` and `libtcc1.a`; LGPL 2.1 ([T1] to [T7], `stage-6/sources.md#tinycc`).
- musl's startup: `_start` passes the stack pointer, `_start_c` reads `argc` and `argv` from it, `__libc_start_main`
  finds `envp` after `argv` and the auxiliary vector after `envp` ([M7], [M8], [M9],
  `stage-6/sources.md#libc-implementations`). Zoraya's processes start with every register zero and read their
  vectors from the startup block at a fixed address (ADR-0042, `docs/design.md` "Threads and processes").
- Licences: dash BSD-3-Clause, GNU make GPLv3, busybox GPLv2, toybox 0BSD, musl MIT, SQLite public domain ([D2],
  [K3], [B4], [B1], [M2], [S1]); Lua MIT and GNU hello GPLv3 `[outside evidence, not fetched]`. The repository is
  BSD-2-Clause except `tools/posix-surface/patches/` (`LICENSE`, its README).
- The pinned Rust toolchain ships `rust-lld` and `rust-objcopy` under `lib/rustlib/<host>/bin/`, with `gcc-ld/`
  wrappers (observed in this session's toolchain; `docs/dependencies.md` lists Rust and QEMU as the host tools).

**What is assumed.** That clang on macOS and Linux compiles musl's retained files and the five programs for a bare
AArch64 ELF target with musl's headers; that toybox's and GNU make's build systems accept an unknown OS name with the
patches below. Both are checked by the first task that builds them.

**Forces from TASK-01.** Where the stable ABI lives decides what can change (DM-057); a.out stayed for decades
(GR-010); the shared-library search order is DM-007, which static linking avoids; executing a file grants nothing
(CP-03). Plan 9 wrote its own compilers and spent most of its effort on standards anyway
(`alternatives-postmortem.md#plan-9--inferno`); Redox took ten years toward self-hosting and its target is its own
(`alternatives-postmortem.md#redox`); Fuchsia ships an SDK with clang and a sysroot for its own triple
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`; the SDK's shape is `[outside evidence]`).

## Decision drivers

1. **Honest identity**: a program must be able to tell it is on Zoraya (`uname`, the triple, the preprocessor), and
   nothing should lie about being Linux (CP-13's "decide the boundary before anyone depends on it").
2. **One cross toolchain that exists on every development host**: the Mac of `docs/design.md` and the Linux CI; no
   prebuilt cross gcc to fetch.
3. **No network in `make test`** after the toolchain is installed; pinned versions and hashes, as TASK-02 did.
4. **Static ELF only** (ADR-0033): no interpreter, no shared objects, so no `DM-007`.
5. **tcc's output must load** (ADR-0108 item 5) and tcc must build itself inside Zoraya (W1).
6. **Licences recorded and separated**: GPL programs are built from fetched sources with small patches; the repository's
   own code stays BSD-2-Clause.

## Considered options

### Option A: Masquerade as `aarch64-linux-musl` with a prebuilt musl cross compiler (Linux baseline)

- **Description:** Fetch a musl cross gcc for `aarch64-linux-musl`, build the programs as TASK-02 did, define
  `__linux__`, let `uname` report `Linux`, and link against `libposix` in place of musl's `libc.a`.
- **Evidence for:** Every program's build system already knows this triple; `config.guess` and `config.sub` work;
  the measured builds used exactly this configuration, so nothing in the programs' portability layers changes.
- **Evidence against:** Driver 1: `__linux__` makes programs take Linux-only paths (kernel headers, `/proc`, Linux
  system calls through `syscall()`), each of which `libposix` must then stub or fail at run time; `uname` lies, and
  every configure check that keys on it gets a Linux answer. A prebuilt cross gcc is a network fetch of a binary, or a
  bootstrap of gcc, on every host (driver 2, driver 3).
- **Cost:** *(estimate)* Smallest patching of the programs; a binary dependency and a permanent lie.
- **Precedent:** Starnix and WSL1 (which must be Linux by design).

### Option B: An own triple, `aarch64-unknown-zoraya`, clang plus `rust-lld`, a musl-header sysroot

- **Description:** A driver script `tools/zcc` invokes the host's clang with `--target=aarch64-unknown-none-elf`,
  `-D__zoraya__`, `-nostdinc -isystem build/sysroot/usr/include` (musl's headers, generated as musl's `Makefile`
  does), `-ffreestanding`-free code generation for AAPCS64 with the FP registers (the ABI `zoraya-rt` and the kernel
  already use), and links with `rust-lld -flavor gnu` (shipped with the pinned toolchain) as
  `-static -nostdlib crt1.o crti.o ... -lc crtn.o`. The programs are configured with `--host=aarch64-unknown-zoraya`
  and small patches; `uname` says `Zoraya`.
- **Evidence for:** Drivers 1 to 4: no Linux macro, the compiler and linker are already on every host (clang comes
  with Xcode's command-line tools and every Linux distribution's packages; `rust-lld` with rustup), static by
  construction. An own triple is what Fuchsia and Redox did. The patches are the ones a new OS always needs: a line
  in `config.sub`, an OS case in tcc's `configure`, the `#ifdef __linux__` branches of toybox's portability header.
- **Evidence against:** A host clang is a new build dependency to record and to version-check (Apple's clang and
  LLVM's differ in defaults). Every program's build must be taught the triple, and autoconf's cross checks need a
  cache of answers for tests that cannot run (a `config.site`). Unknown-OS branches in toybox and make may compile out
  features T1 needs; the first build finds them.
- **Cost:** *(estimate)* A 100-line driver script, a `ports/` makefile, a `config.site`, patches of a few lines per
  program, an archiver (below), `docs/dependencies.md` rows.
- **Precedent:** Fuchsia's SDK (clang, own triple, sysroot), Redox (own target).

### Option C: tcc as the host cross compiler (`arm64-tcc`), for everything

- **Description:** Build tcc on the host as a cross compiler for arm64 (`configure --enable-cross` or `make cross-arm64`,
  [T3], [T6]) and compile musl's files, `libposix`'s C parts and the five programs with it; link with tcc's linker.
- **Evidence for:** Self-contained: one small C program built with the host's `cc` is the whole toolchain, the same
  compiler inside and outside Zoraya, so what builds on the host builds in W1. tcc built Lua, SQLite and itself in
  the measurements.
- **Evidence against:** The measured programs were built with gcc; whether tcc compiles GNU make 4.4.1, toybox and
  busybox is not shown (toybox uses GNU C extensions its README documents for gcc and clang
  `[outside evidence]`), and tcc's diagnostics and optimisation are weaker, so the ports would be debugged against
  two compilers at once. The host still needs a C compiler to build tcc.
- **Cost:** *(estimate)* Smaller tooling than B; a risk on every program but tcc, Lua and SQLite.
- **Precedent:** tcc's own cross compilers (shipped, [T6]).

### Option D: Build everything inside Zoraya after a minimal bootstrap

- **Description:** Cross-compile only `libposix`, a first tcc, dash and the applets; then build make, toybox and the
  rest inside Zoraya with tcc.
- **Evidence for:** Self-hosting from the start; the host needs the least.
- **Evidence against:** Option C's compiler risk plus the guest's speed under TCG in CI, and the build of the test
  image would need the guest to run before the tests that check the guest. W1 already proves self-hosting for tcc;
  nothing asks for more.
- **Cost:** *(estimate)* Slowest `make test` by far.
- **Precedent:** None in the files.

## Recommendation

**Recommendation (not a decision): Option B**, with tcc's cross compiler (Option C) as the second choice for any
program clang cannot build for the target and the first choice for `libtcc1.a`.

1. **Triple and identity.** `aarch64-unknown-zoraya`, and `uname -s` is `Zoraya` (ADR-0107 item 11), both by the
   architect's decision. `tools/zcc` defines `__zoraya__` and nothing Linux; `config.guess` is not run (cross builds
   pass `--host`). Every autoconf package's `config.sub` must learn `zoraya*` as an OS before its `configure` accepts
   the triple: a patch of a few lines per package, paid for dash and GNU make here and, as a known cost of T2, for
   every autoconf project built on Zoraya (GNU hello first) until upstream `config.sub` carries the name. Code generation: AArch64 with FP and SIMD, AAPCS64, `-fno-stack-protector` (no `AT_RANDOM` source
   yet, below), `-O2 -g` as measured, PIC off (static `ET_EXEC`).
2. **Sysroot** at `build/sysroot/usr/`: `include/` from musl 1.2.5 (`include/`, `arch/aarch64/bits`,
   `arch/generic/bits`, the generated `bits/alltypes.h` and `bits/syscall.h`, the latter empty of Linux numbers),
   `lib/libc.a` (ADR-0100's archive), `lib/crt1.o`, `lib/crti.o`, `lib/crtn.o`, `lib/libm.a`, `lib/libpthread.a` and
   `lib/librt.a` as empty archives (musl's convention, so `-lm` and `-lpthread` resolve against `libc.a`),
   `lib/tcc/libtcc1.a` and `lib/tcc/include/` for tcc inside Zoraya.
3. **Archiver.** `tools/zoraya-ar`, a host tool writing the GNU `ar` format `rust-lld` and tcc read, so the build does
   not depend on the host's `ar` variant; `ADR-0100`'s export checker reads the same format.
4. **Startup.** `crt1.o` is built from `ports/crt/crt1.S` and the Rust function `zoraya_posix_start` in
   `zoraya-posix`: `_start` clears the frame pointer and link register as musl's does ([M9]), checks the zero-register
   contract of `zoraya-rt`, and calls `zoraya_posix_start(sp)`, which reads the startup block of ADR-0042, writes
   `argc`, the `argv` pointers, `NULL`, the `envp` pointers, `NULL` and the auxiliary vector onto the stack: `AT_PAGESZ`
   4096; `AT_PHDR`, `AT_PHENT` 56 and `AT_PHNUM` from the program header table the kernel now copies into the startup
   block, which musl's `__init_tls` walks with that stride for `PT_TLS` ([M10]); `AT_RANDOM` pointing at 16 bytes
   derived from the clock and the process's handle generations, labelled weak; `AT_NULL`. This is the one list for
   `crt1` (ADR-0100 item 4 refers to it). It then reads the bootstrap messages of ADR-0107 and tail-calls musl's
   `_start_c` with that stack. `crti.o` and `crtn.o` hold the empty `_init` and `_fini` prologues and epilogues.
5. **The startup block gains the program header table**: the kernel appends the ELF's program headers after the
   vectors (ADR-0042's layout, versioned by the header size), which `AT_PHDR` points at; ADR-0042's "Nothing may
   follow" becomes "then the program headers, and nothing may follow". The whole table goes, all `e_phnum` entries of
   56 bytes, not only the `PT_LOAD` ones `zoraya-elf` keeps: musl's `__init_tls` looks for `PT_TLS`, `PT_PHDR`,
   `PT_DYNAMIC` and `PT_GNU_STACK` ([M10]), which `zoraya-elf` skips when loading; `zoraya-elf` already refuses a table
   longer than its `MAX_PHDRS` of 32 and gains an accessor for the raw table. The block's room is
   `STARTUP_BLOCK_MAX`, one page beyond the 64 KiB `spawn` buffer, so a `spawn` whose vectors plus headers exceed it
   is refused with `LimitReached`, as an over-long buffer is today.
6. **The ports.** `make fetch-ports`, a step outside `make test` by the architect's decision, downloads each archive
   by URL from `ports/sources.lock` (versions and SHA-256 as TASK-02's, musl added) into the cache `build/ports/src/`,
   refuses a hash mismatch, and does nothing for an archive already in the cache with the right hash. `ports/Makefile`,
   run by `make ports` (a prerequisite of the stage 6 tests, ADR-0110), builds from the cache only: with an archive
   missing it fails at once with a message naming `make fetch-ports`, and never downloads. No third-party source,
   GPL or LGPL in particular, is committed to the repository; only `ports/sources.lock`, the patches of item 9 and
   the build files are. Then builds, with `tools/zcc`: musl's retained files and `zoraya-posix` into
   `libc.a`; dash (`./configure --host=aarch64-unknown-zoraya` with a `config.site` answering the checks that cannot
   run; its `mksignames` runs on the host); GNU make (`--host=...`, `--disable-load`, `--without-guile`; `posix_spawn`
   detected by the site file so make uses it, as measured); toybox (a `.config` with the T1 applets of
   `applets-by-tier.tsv`, `cat head ls mv rm sleep touch true uname`, plus `sort`, `wc` and `yes`, which W10's script
   runs (`tools/posix-surface/workloads/w10/w10.sh`: `sort`, `wc -l`, `yes`) and which the TSV, made before TASK-02b,
   lists at T2 or not at all, added on TASK-02b's evidence as ADR-0005 added `writev` (tension below); and the
   `__linux__` branches of its portability layer compiled out); busybox (`tr`, `diff`); tcc (`./configure --cpu=aarch64 --targetos=Zoraya --prefix=/usr
   --tccdir=/usr/lib/tcc --crtprefix=/usr/lib --sysincludepaths=/usr/include:/usr/lib/tcc/include --libpaths=/usr/lib
   --elfinterp=` with `CONFIG_TCC_STATIC`, cross-compiled by `zcc`), and `arm64-libtcc1.a` by `zcc` through
   `arm64-libtcc1-usegcc=yes` ([T6]; `lib/Makefile` chooses `dsohandle.o` and the native-only objects by OS branch, one
   more patch point for a `Zoraya` target). Outputs go to `build/rootfs-posix/` as `/bin/sh`, `/bin/dash`, `/bin/make`,
   `/bin/tcc`, `/bin/cc` → `tcc`, `/bin/toybox` with a link per applet (ADR-0108 item 6), `/bin/busybox` with `tr`
   and `diff` links, `/usr/include`, `/usr/lib`. `zoraya-mkfs` makes the image (ADR-0110).
7. **tcc inside Zoraya.** With item 6's configuration tcc links `-static` always, emits no `PT_INTERP`, finds
   `crt1.o`, `crti.o`, `crtn.o` and `libc.a` in `/usr/lib`, headers in `/usr/include` and `/usr/lib/tcc/include`, and
   `libtcc1.a` in `/usr/lib/tcc` ([T5], [T7]). Patches: `configure` gains a `Zoraya` case for `targetos` that takes
   the generic ELF branch; `tcc.h` gets `TARGETOS_Zoraya` with the Linux ELF code paths, no dynamic linker, and
   `CONFIG_TCC_STATIC`; `-run` and backtraces are disabled (`CONFIG_TCC_BACKTRACE=0`), as ADR-0005 excludes them.
   Text starts at 4 MiB as the loader requires (ADR-0108 item 5). The output is validated on the host by running
   `zoraya-elf` over `a.out` in a host test before any boot.
8. **W1 inside Zoraya.** `sh ./configure --cc=tcc --cpu=aarch64 --targetos=Zoraya ...` then `make` in tcc's tree,
   with `/bin/cc` the stage 6 tcc: the stage-2 tcc is linked against the same `libc.a` and compiles the test program
   of `W1-check`. `--config-musl` is dropped: it sets the dynamic linker path and `CONFIG_TCC_MUSL` ([T3]), whose other
   effects in tcc's sources the first toolchain task checks.
9. **Patches** live in `ports/patches/<program>/` with a README naming each original's version and licence, as
   `tools/posix-surface/patches/` does: dash (the `fork` group hint of ADR-0104 item 4 in `forkshell`, about 5 lines;
   `vforkexec` to `posix_spawn` with `POSIX_SPAWN_SETPGROUP` and the Zoraya-only `POSIX_SPAWN_TCSETPGRP` for the
   interactive foreground path, so that the parent, not the child, hands the terminal over before the child runs,
   ADR-0104 item 5, ADR-0003's follow-up, the M4 patch extended); make (none expected beyond `config.sub`); toybox and busybox (portability branches); tcc (item 7);
   musl (ADR-0100 item 2). A patch to a GPL program is distributed under that program's licence, which the README
   states.
10. **Host tools recorded** in `docs/dependencies.md`: clang (the version each development host has; the build prints
    it), `rust-lld` and `rust-objcopy` from the pinned toolchain, and the ported programs' versions and licences in a
    new table.

**Tensions with accepted ADRs.**

- *ADR-0005*'s "the TSV is authoritative" for applets: the T1 build adds `sort`, `wc` and `yes` (item 6) on
  TASK-02b's W10 evidence, as the ADR itself added `writev`; `applets-by-tier.tsv` predates TASK-02b. Accepting this
  ADR adds those three to T1's applet list.

- *ADR-0042*: the startup block's layout grows (item 5), versioned by its header size as the ADR provides.
- *ADR-0033*: unchanged in rule; the loader now also serves tcc's output, which item 7 shapes to it.
- *ADR-0003*'s follow-up on dash's interactive `vforkexec` is taken up as a patch (item 9).
- *CLAUDE.md*'s "no network" rule: met by `make fetch-ports` being outside `make test`, as `rustup` is; the CI
  workflow gains the fetch step before `make test`.
- *`docs/dependencies.md`*: third-party C code and a host compiler are new kinds of dependency; the file's rule speaks
  of crates.

### Effect of the vision

For target 2 nothing here changes: the toolchain is the hosted workload. For target 3 the own triple is the right
start: an SDK (`zcc`, sysroot, `zoraya-ar`) is what third parties would use, and a Rust target `aarch64-unknown-zoraya`
in rustc would follow Redox's path. Threads: `libpthread.a` is empty today and would gain content with T3.
Networking: no effect.

### Deferred to a later stage

A Rust target for Zoraya user programs with `std`; a dynamic loader; `AT_RANDOM` from a real entropy source and
stack protectors.

## Consequences

- **Positive.** One honest triple; the compiler and linker are already on every host; the sysroot is a directory the
  build makes; tcc's output runs through the same loader as every program.
- **Negative.** A host clang dependency; patches to five programs and musl to carry; autoconf cross checks answered by
  a site file that must be kept true; a fetch step before the first `make test`.
- **Follow-up.** `tools/zcc`, `tools/zoraya-ar`, `ports/`, `ports/sources.lock`, `config.site`, `crt1.S`,
  `docs/dependencies.md`, the CI step.

## Invariants

- **INV-0109-1.** Every program in `build/rootfs-posix/bin` and every object `tcc` links inside Zoraya is a static
  `ET_EXEC` AArch64 ELF with no `PT_INTERP` and no writable-executable segment, accepted by `zoraya-elf`. *Check:* a
  host test runs `zoraya-elf` over every file the ports build and over tcc's `a.out` from the W1 check.
- **INV-0109-2.** No port is compiled with `__linux__` defined, and `uname -s` inside Zoraya prints `Zoraya`.
  *Check:* `tools/zcc` refuses `-D__linux__`; a `grep` of the build logs for `__linux__`; the W5 script prints `uname`.
- **INV-0109-3.** `make test` runs without network once `make fetch-ports` has filled the cache and the Rust toolchain
  is installed; a missing source fails with a message naming `make fetch-ports`, never with a download. *Check:* CI runs the fetch step first and
  `make test` with network disabled `[assumption: the runner allows it; otherwise a host test runs `ports/Makefile` in
  a sandbox with no network]`.
- **INV-0109-4.** Every archive in `ports/sources.lock` matches its recorded SHA-256 before it is unpacked.
  *Check:* the fetch step verifies and a host test feeds it a corrupted archive.

## Open questions

- Whether Apple's clang accepts `--target=aarch64-unknown-none-elf` with musl's headers for every retained file; a
  Linux clang is the fallback for the Mac through the CI, which is not a development loop.
- Whether toybox's T1 applets build without `__linux__` unpatched; the first task finds out.
- `AT_RANDOM`'s weak source: acceptable while no port uses stack protectors or `getentropy` (T2's `getentropy` is
  toybox's).

## Revisit triggers

- A program clang cannot build for the target: Option C for that program.
- A measured need for shared libraries or a dynamic loader (ADR-0033's trigger).
- The fetch step failing on CI more than rarely: a mirror of the archives under the project's control, since the
  architect's decision keeps third-party sources out of the repository.

## Architect's decision

