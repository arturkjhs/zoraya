# ADR-0360: The C++ runtime: LLVM's libc++, libc++abi and libunwind 20.1.8, static, built by `zc++` into the sysroot

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0033, ADR-0100, ADR-0109, ADR-0141, ADR-0302, ADR-0303, ADR-0320, ADR-0361, ADR-0364, ADR-0366
- **Evidence:** CP-27

## Context
TASK-30 asks for C++ for Zoraya programs: every large program still missing (a browser engine, LLVM, CMake, ICU, HarfBuzz,
Skia) is C++, and `zcc` builds C only (ADR-0109). A C++ program needs three libraries beside `libc.a`: the standard library,
the ABI library (exceptions, RTTI, `new`, guards of local statics, `__cxa_thread_atexit`) and an unwinder. `zcc` runs the host's
clang (Apple clang 21 on the Mac, Ubuntu's clang 18 in CI) for a bare AArch64 ELF target with musl's headers, and links with
rust-lld. Programs are static ET_EXEC (ADR-0033); there is no dynamic loader (ADR-0100).

Measured (TASK-30): libc++ 21 and later warn "only supports Clang 19 and later" (`__configuration/compiler.h`), and 20 supports
Clang 18; LLVM 20.1.8 is the last 20.x release. libc++ 20's `from_chars` for floating point includes LLVM libc's `shared/`
headers, which only the whole-project archive holds (147 MB; four directories of it are unpacked). The three archives built by
`ports/cxx/cxx.mk`: `libc++.a` 2.5 MB, `libc++abi.a` 0.6 MB, `libunwind.a` 0.1 MB (no debug information); the build takes
about a minute with three jobs on the Mac mini (M4). `hello` (iostream and one exception) is 3.5 MB with `libc.a`'s debug
information, 1.0 MB without (`-Wl,--strip-debug`).

## Decision drivers
1. Licence: only permissive code in what programs link (`docs/dependencies.md`, CLAUDE.md "Dependencies").
2. The compiler is clang; the library must be built and used by the clang CI has (18) and the Mac's.
3. The ports' method: a pinned, hashed archive, patches under `__zoraya__`, no committed copy (ADR-0109 item 6, CP-27).
4. Static programs only, no loader (ADR-0033, ADR-0100).
5. The next ports (ICU, HarfBuzz, Skia, a browser engine) build against it unchanged where they can.

## Considered options
### Option A: LLVM's libc++, libc++abi and libunwind, static (the recommendation)
- **Description:** LLVM 20.1.8 (`ports/sources.lock`, the release's own SHA-256), built by `ports/cxx/cxx.mk` with the file lists
  and flags of LLVM's CMake for a static build: exceptions, RTTI, threads over libposix's pthreads (musl's `src/thread`,
  ADR-0302), localization (the C locale, ADR-0363), `std::filesystem`, `random_device` over `getentropy` (patch `llvm/0001`),
  no time zone database, no `libc++experimental`, no modules, the hardening mode `fast` by default (`ports/cxx/__config_site`).
  Headers in `build/sysroot/usr/include/c++/v1` (and libunwind's in `usr/include`), archives in `usr/lib` (ADR-0365).
- **Evidence for:** Apache-2.0 with LLVM exception (driver 1); made for clang and builds with 18 and 21 (driver 2); runs on
  musl (it has a `_LIBCPP_HAS_MUSL_LIBC` configuration); Fuchsia and Android ship it.
- **Evidence against:** the 147 MB archive for four directories; the version is held back by CI's clang (a newer clang in
  CI would allow a newer libc++).
- **Cost:** *(measured)* `cxx.mk` 100 lines, `__config_site` 40, three patches of 25, 13 and 22 lines, a shim of 70 (ADR-0366).
- **Precedent:** Fuchsia, Android (NDK), FreeBSD, macOS, Chimera Linux (musl and libc++).
### Option B: GCC's libstdc++ and libgcc_s's unwinder (the Linux baseline)
- **Evidence against:** GPL-3.0 with the runtime exception, which the dependency rule does not list (driver 1); built by GCC,
  which is not the toolchain here; libgcc's unwinder comes with a compiler runtime we do not build.
- **Precedent:** every GNU/Linux distribution; Haiku and Redox (alternatives-postmortem: both ship GCC's runtime).
### Option C: libc++ with libcxxrt or LLVM's libunwind replaced by `libgcc_eh`
- **Evidence against:** libcxxrt (FreeBSD's ABI library) is BSD-licensed but a second upstream to follow; `libgcc_eh` is GCC's
  (Option B's licence).
### Option D: no exceptions and no RTTI (`-fno-exceptions -fno-rtti`), libc++ alone
- **Evidence against:** the task asks for both; ICU and Skia build without, but a browser engine's dependencies and CMake do not.
- **Precedent:** Fuchsia's kernel, Chromium's base (no exceptions).

## Recommendation
**Recommendation (not a decision): Option A**, LLVM 20.1.8, static, with the configuration of `ports/cxx/__config_site`.
The hardening mode `fast` (cheap bounds checks of containers, `optional`, `span` and the like trap with `brk`, which the
shell reports as "crashed (breakpoint, class 0x3c)") is a choice for safety over the last per cent of speed; a program built
with `-D_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_NONE` turns it off.

## Consequences
- **Positive.** C++ programs build with `zc++` and run from the volume; the same libraries serve every C++ port.
- **Negative.** 1 MB more per program than C for iostream; the version waits for CI's clang; `std::chrono`'s time zones and
  `libc++experimental` are absent.
- **Follow-up.** A newer clang in CI (Ubuntu's `clang-19` or later) to move to LLVM 21 or later; the time zone database when
  the system has one.

## Invariants
- **INV-0360-1.** A C++ program built by `zc++` throws and catches an exception across frames and threads, uses RTTI, static
  and `thread_local` destructors, threads and iostreams inside Zoraya. *Check:* `make test-cxx` (`tests/cxx/cxx.script`).
- **INV-0360-2.** The chosen groups of libc++'s own tests pass inside Zoraya. *Check:* `make test-cxx-libcxx`.

## Open questions
- Whether `fast` hardening costs anything measurable for the next ports (not measured).

## Revisit triggers
- CI's clang moves to 19 or later (a newer libc++ supports it).
- A port needs `libc++experimental` or the time zone database.

## Architect's decision
