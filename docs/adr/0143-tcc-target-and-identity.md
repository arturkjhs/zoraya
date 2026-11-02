# ADR-0143: tcc's Zoraya target, and what the compilers predefine

- **Status:** proposed
- **Date:** 2026-11-02
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
   `tccdefs.h` chooses `wchar_t` and `int64_t` by `__linux__`.
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
- **Evidence against:** A program that selects its POSIX code by `__unix__` takes its non-Unix path on Zoraya (none of
  the T1 programs does: they built without it).
- **Cost:** *(estimate)* 150 lines of patches, as built.
- **Precedent:** Redox gave its own target to its compilers (`alternatives-postmortem.md#redox`).

### Option C: The same target, with `__unix__` predefined by tcc and `zcc`

- **Description:** As B, and both compilers also define `__unix__` and `__unix`.
- **Evidence for:** Programs that test `__unix__` for POSIX interfaces would take that path.
- **Evidence against:** Zoraya is not Unix: POSIX is a personality over native handles (ADR-0002), and a program that
  keys on `__unix__` may expect more than the tier list; the T1 programs need neither.
- **Cost:** As B, plus the definition in both compilers.
- **Precedent:** Most Unix-like hobby systems define it.

## Recommendation

**Recommendation (not a decision): Option B**, with this configuration (`ports/Makefile`):
`--cpu=aarch64 --targetos=Zoraya --prefix=/usr --tccdir=/usr/lib/tcc --crtprefix=/usr/lib
--sysincludepaths=/usr/include:/usr/lib/tcc/include --libpaths={B}:/usr/lib --elfinterp= --os-release=`.
`{B}` (tcc's directory) is added to ADR-0109's library path so that `libtcc1.a` stays in `/usr/lib/tcc` as item 2's
layout has it (finding 3). `--os-release=` and `make GITHASH=no` keep the build host out of the binary (finding 7).
`--config-musl` is dropped as item 8 says; its other effects are only the `-dumpmachine` string and the bounds checker
(finding 6). tcc's `c2str` runs on the host and is compiled by the host's compiler before `make`; `libtcc1.a` is
compiled by `zcc` (`arm64-libtcc1-usegcc=yes`).

**The host check of item 7.** The tcc built for Zoraya cannot run on the host. `make check-ports` therefore builds the
same source with the same patches and configuration, compiled by the host's C compiler and with every path rooted at
`build/sysroot`; that tcc compiles and links `ports/tests/hello.c` (variadic calls, `long double`, `wchar_t`) with
the sysroot's crt objects, `libc.a` and `libtcc1.a`, `zoraya-elf-check` accepts the result (finding 8, which answers
ADR-0108's open question: no patch for the loader's ranges or alignment), and its predefined macros contain
`__zoraya__` and nothing of Linux or Unix. What it does not check is the cross-built tcc's own code generation on
Zoraya, which only W1 in 6e can.

## Consequences

- **Positive.** One identity for both compilers; tcc's output loads; nothing executes generated code in memory.
- **Negative.** Two patches to carry across tcc commits; a program testing `__unix__` needs a patch or a `-D`.
- **Follow-up.** W1 (6e) builds tcc inside Zoraya with `--targetos=Zoraya`, which needs these patches in the source
  tree the image carries (`/src/tcc`, ADR-0110 item 1).

## Invariants

- **INV-0143-1.** tcc for Zoraya predefines `__zoraya__` and none of `__linux__`, `__linux`, `__unix__`, `__unix`;
  `zcc` likewise. *Check:* `make check-ports` greps the host tcc's `-dM -E` output; `zcc` refuses `-D__linux__`.
- **INV-0143-2.** tcc for Zoraya has no `-run`. *Check:* `TCC_IS_NATIVE` is undefined under `TARGETOS_Zoraya`
  (patch 0001), so `tccrun.c` compiles to nothing; a review item.

## Open questions

- Whether the host-built tcc and the cross-built one generate the same code for the same input (they should: the
  code generator does not depend on the host); a byte comparison of their outputs is possible only in 6e.

## Revisit triggers

- A program in scope that needs `__unix__`.
- A tcc update that changes `TCC_IS_NATIVE` or its target-OS macros.

## Architect's decision

