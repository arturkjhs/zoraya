# ADR-0143: tcc's Zoraya target, and what the compilers predefine

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0005, ADR-0033, ADR-0100, ADR-0108, ADR-0109, ADR-0140, ADR-0141
- **Evidence:** CP-13, CP-16, DM-007, alternatives-postmortem.md#redox, stage-6/sources.md#tinycc

## Context

ADR-0109 item 7 configures tcc (`stage-6/sources.md#tinycc`) for Zoraya: `--targetos=Zoraya` taking "the generic ELF
branch", `TARGETOS_Zoraya` in `tcc.h` "with the Linux ELF code paths, no dynamic linker, and `CONFIG_TCC_STATIC`",
no `-run`, no backtraces, and libraries in `/usr/lib` and `/usr/lib/tcc`. Item 8 leaves to this task what
`--config-musl` does besides the interpreter path, ADR-0108 asks whether tcc's ARM64 output needs a patch for the
loader, and item 1 says `zcc` defines `__zoraya__` "and nothing Linux". TASK-17 built tcc at the pinned commit and
found:

1. tcc predefines `__linux__`, `__linux`, `__unix__` and `__unix` for every ELF target that is not a BSD, and its
   `tccdefs.h` chooses four things by `__linux__`: `wchar_t`, `int64_t`, macro versions of `__builtin_huge_val`,
   `__builtin_huge_valf`, `__builtin_huge_vall` and `__builtin_nanf` (for targets without `__linux__`), and the
   bounds checker's `malloc` redirection.
2. A tcc compiled by `zcc` for AArch64 counts as native (its `TCC_IS_NATIVE` test compares the compiler's own
   architecture with the target's) and so compiles `-run`, which needs `dlopen` and executable memory.
3. With `--libpaths=/usr/lib`, tcc looks for `libtcc1.a` in `/usr/lib` only: it searches the library path, not its own
   directory.
4. `lib/armflush.c`, part of `libtcc1.a` on arm64, compiles only with tcc itself (a tcc builtin); it serves `-run`.
5. tcc loads sections flagged `SHF_EXCLUDE`. The precompiled Rust libraries in `libc.a` (ADR-0140) carry LLVM's
   embedded bitcode in such sections, with mapping symbols in them, and tcc then reports `unresolved reference to '$d'`
   for any program that uses `printf`.
6. `--config-musl` changes only the `-dumpmachine` string and the bounds checker (`lib/bcheck.c`), besides the
   interpreter path.
7. configure records the build host's `uname -r` (`CONFIG_OS_RELEASE`), and the Makefile asks the enclosing git
   repository for a version string, which inside this repository is Zoraya's branch.
8. A tcc built from the same source for the host links a hello world against the sysroot into a static `ET_EXEC` with
   four `PT_LOAD` segments (R, R-X, RW-, RW-) from 4 MiB and no `PT_INTERP`, which `zoraya-elf` accepts unchanged.
9. Apple's and LLVM's clang, for `aarch64-unknown-none-elf`, define neither `__linux__` nor `__unix__`; `zcc` adds
   `__zoraya__`.
10. **Who tests `__unix__`** (the sources of W1 to W3 at TASK-02's pins, hashes checked, and of every port, grepped for
    `__unix__`, `__unix` and `unix` as a macro):
    - **Lua 5.5.1 (W2):** never. Its only OS test is `_WIN32`; W2's `make posix` passes `-DLUA_USE_POSIX`, which
      selects every POSIX path (`luaconf.h`, `loslib.c`, `liolib.c`, `lua.c`).
    - **SQLite 3540100 (W3):** never. `sqlite3.c` chooses its Unix VFS whenever the target is not Windows (`_WIN32`,
      `WIN32`, `__CYGWIN__`, `__MINGW32__`, `__BORLANDC__`), and `shell.c` likewise; `__linux__` selects only optional
      features (`fdatasync`, `O_TMPFILE`, batch atomic writes), which are then left out.
    - **tcc (W1):** never in its own code; `tccpp.c` only predefines `__unix__` for its targets. `__linux__` appears
      in `conftest.c` (the host's triplet, at configure time) and in the bounds checker, which Zoraya does not build.
    - **The ports:** musl's headers, dash, GNU make (with gnulib) and toybox never; busybox only together with
      `__digital__` (Tru64).
    So no program in scope selects anything by `__unix__`, and nothing takes a non-Unix path for its absence.
11. **What else the two compilers predefine differently** (the OS, object-format, architecture and ABI-type macros of
    `zcc -dM -E` and the host tcc's `-dM -E`): clang defines `__ELF__` and `_LP64`, tcc neither; clang's bare-metal
    target says `__WINT_TYPE__` is `int`, tcc says `unsigned int`, and musl's `wint_t` on AArch64 is `unsigned`.

## Decision drivers

1. **Honest identity** (ADR-0109 driver 1, CP-13): nothing says Linux, and `zcc` and tcc agree on what Zoraya is.
2. **Static ELF only, loaded by the one loader** (ADR-0033, DM-007, CP-16): tcc's output must pass `zoraya-elf`.
3. **No `-run`** (ADR-0005): no executable memory from user space.
4. **Small patches to an LGPL program**, each with its reason, reproducible builds.

## Considered options

### Option A: tcc as a Linux target with musl (baseline, TASK-02's `--config-musl`)

- **Description:** Configure as TASK-02 did and only change paths.
- **Evidence for:** No patch to tcc's sources; measured on Linux.
- **Evidence against:** Every program tcc compiles inside Zoraya sees `__linux__` (finding 1), which INV-0109-2
  forbids; `-run` is compiled in (finding 2).
- **Cost:** None now; a lie in every compile.
- **Precedent:** Alpine's tcc.

### Option B: A Zoraya target OS that predefines `__zoraya__` only (recommended)

- **Description:** Patch 0001 adds `Zoraya` to configure (static by default, no backtraces, no bounds checker) and
  `TARGETOS_Zoraya` to `tcc.h`, `tccpp.c`, `tccdefs.h`, `libtcc.c` and `tcc.c`: tcc predefines `__zoraya__` and
  neither the Linux names nor `__unix__`, as clang does for `zcc`; `wchar_t` is `unsigned int` and `int64_t` `long`, as
  AAPCS64 and musl say; `TCC_IS_NATIVE` is undefined, so there is no `-run`; `-dumpmachine` prints
  `aarch64-unknown-zoraya` and `-v` `AArch64 Zoraya`; `libtcc1.a` leaves out `armflush.o`. Patch 0002 skips
  `SHF_EXCLUDE` sections, as the ELF gABI says a link must.
- **Evidence for:** Drivers 1 to 4; the hello world links with the Rust members in `libc.a` (finding 5).
- **Evidence against:** A program that selects its POSIX code by `__unix__` would take its non-Unix path on Zoraya;
  no program in scope does (finding 10). Of finding 1's other two branches, the bounds checker's is moot (no
  `-b`), and Zoraya gets the macro forms of the huge-value and NaN builtins; compiled by the host tcc, musl's
  `INFINITY`, `HUGE_VAL` and `HUGE_VALL` are infinities and `NAN` is a NaN (checked once, TASK-17).
- **Cost:** *(estimate)* 150 lines of patches, as built.
- **Precedent:** Redox gave its own target to its compilers (`alternatives-postmortem.md#redox`).

### Option C: The same target, with `__unix__` predefined by tcc and `zcc`

- **Description:** As B, and both compilers also define `__unix__` and `__unix`.
- **Evidence for:** Programs that test `__unix__` for POSIX interfaces would take that path; outside the programs in
  scope (T2 and later) some do.
- **Evidence against:** No program in scope tests it (finding 10), so it buys nothing measured. Zoraya is not Unix:
  POSIX is a personality over native handles (ADR-0002), and a program that keys on `__unix__` may expect more than
  the tier list (`/proc`, `fork` at volume, signals anywhere); defining it later is one line in each compiler, while
  taking it back after programs depend on it is not (CP-13).
- **Cost:** As B, plus the definition in both compilers.
- **Precedent:** Most Unix-like hobby systems define it.

## Recommendation

**Recommendation (not a decision): Option B**, with this configuration (`ports/Makefile`):
`--cpu=aarch64 --targetos=Zoraya --prefix=/usr --tccdir=/usr/lib/tcc --crtprefix=/usr/lib
--sysincludepaths=/usr/include:/usr/lib/tcc/include --libpaths={B}:/usr/lib --elfinterp= --os-release=0`.
`{B}` (tcc's directory) is added to ADR-0109's library path so that `libtcc1.a` stays in `/usr/lib/tcc` as item 2's
layout has it (finding 3). `--os-release=0` (an empty value leaves configure's default, the build host's `uname -r`; the value is used only
for BSD note sections) and `make GITHASH=no` keep the build host out of the build (finding 7).
`--config-musl` is dropped as item 8 says; its other effects are only the `-dumpmachine` string and the bounds checker
(finding 6). tcc's `c2str` runs on the host and is compiled by the host's compiler before `make`; `libtcc1.a` is
compiled by `zcc` (`arm64-libtcc1-usegcc=yes`).

**One identity for both compilers** (finding 11). Both predefine `__zoraya__`, no Unix or Linux name, `__ELF__`,
`__aarch64__`, `__AARCH64EL__`, `__LP64__` and `_LP64`, unsigned `char`, little-endian byte order, and the same
`size_t`, `ptrdiff_t`, `wchar_t` (`unsigned int`), `wint_t` (`unsigned int`) and `int64_t`: patch 0001 adds `__ELF__`
and `_LP64` to tcc's Zoraya target, and `zcc` sets `__WINT_TYPE__` to musl's. `ports/compare-predefs.sh`, a step of
`make check-ports`, compares these 26 names (defined or not, and their values) between `zcc` and the host tcc and
fails on any difference; 15 are defined. Compiler-specific macros (`__clang__`, `__TINYC__`, feature macros) are not
compared.

**The host check of item 7.** The tcc built for Zoraya cannot run on the host. `make check-ports` therefore builds the
same source with the same patches and configuration, compiled by the host's C compiler and with every path rooted at
`build/sysroot`; that tcc compiles and links `ports/tests/hello.c` (variadic calls, `long double`, `wchar_t`) with
the sysroot's crt objects, `libc.a` and `libtcc1.a`, `zoraya-elf-check` accepts the result (finding 8, which answers
ADR-0108's open question: no patch for the loader's ranges or alignment), and its predefined identity is `zcc`'s. What it does not check is the cross-built tcc's own code generation on
Zoraya, which only W1 in 6e can.

## Consequences

- **Positive.** One identity for both compilers; tcc's output loads; nothing executes generated code in memory.
- **Negative.** Two patches to carry across tcc commits; a program testing `__unix__` needs a patch or a `-D`.
- **Follow-up.** W1 (6e) builds tcc inside Zoraya with `--targetos=Zoraya`, which needs these patches in the source
  tree the image carries (`/src/tcc`, ADR-0110 item 1).

## Invariants

- **INV-0143-1.** tcc for Zoraya and `zcc` predefine the same identity: `__zoraya__`, none of `__unix__`, `__unix`, `unix`, `__linux__`, `__linux`, `linux`, `__gnu_linux__`, and equal values for the object-format, architecture and ABI-type macros. *Check:* `ports/compare-predefs.sh` in `make check-ports`; `zcc` refuses `-D__linux__`.
- **INV-0143-2.** tcc for Zoraya has no `-run`. *Check:* `TCC_IS_NATIVE` is undefined under `TARGETOS_Zoraya` (patch 0001), so `tccrun.c` compiles to nothing, and `make check-ports` finds the cross compiler's refusal of `-run` in the binary.

## Open questions

- `zcc` refuses the `__linux__` definitions it can see on its command line (`-D`, `-Wp,-D`, `--define-macro`,
  response files, `-Xclang`, a caller's target); a header given by `-include`, or a port's own `#define __linux__`, is
  not refused, and the check of the build logs sees only command lines.
- Whether the host-built tcc and the cross-built one generate the same code for the same input (they should: the
  code generator does not depend on the host); a byte comparison of their outputs is possible only in 6e.

## Revisit triggers

- A program in scope that tests `__unix__` (T2 brings autoconf projects; the grep of finding 10 is the check to repeat).
- A tcc update that changes `TCC_IS_NATIVE` or its target-OS macros.

## Architect's decision

Accepted as recommended, 2026-11-03: `__unix__` is not defined, and `ports/compare-predefs.sh` stays a step of `make check-ports`. Decided by the architect; recorded by an agent at the architect's request.
