# ADR-0364: The default C++ standard of `zc++`: `-std=c++23`, with `_GNU_SOURCE` for every standard

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0109, ADR-0143, ADR-0360
- **Evidence:** CP-27

## Context
TASK-30 asks `zc++` to build with `-std=c++23` unless a build says otherwise. clang's own default for C++ is `gnu++17`. A strict
`-std=c++NN` defines `__STRICT_ANSI__`, under which musl's headers declare no POSIX name (`features.h`), and libc++'s headers
call `nanosleep` (`<thread>`) and, for musl, `strtod_l` and the other `*_l` functions, which musl declares only under
`_GNU_SOURCE`. Measured: without a feature macro the runtime does not compile (`use of undeclared identifier 'nanosleep'`);
with `_XOPEN_SOURCE=700` and `_BSD_SOURCE` it stops at `strtof_l`. clang defines `_GNU_SOURCE` for C++ on Linux and musl
targets for this reason; the bare-metal target `zcc` uses defines nothing.

## Decision drivers
1. The task's default, C++23, which the ports of the next stage (browser engines' dependencies) build with or below.
2. A build that names its own standard (`-std=c++17`, CMake's `CXX_STANDARD`) gets it.
3. libc++'s headers must compile under every standard (driver 2), so the feature macro cannot depend on the standard.
4. Nothing Linux is defined (INV-0109-2): `_GNU_SOURCE` is a feature-test macro of the C library, not a system's name.
5. No patch of libc++'s headers to avoid the macro (CP-27).

## Considered options
### Option A: `-std=c++23` unless `-std=` or `-x` is on the line, and `-D_GNU_SOURCE` always (the recommendation)
- **Evidence for:** drivers 1 to 4; the same declarations libc++ is built and tested with on musl.
- **Evidence against:** `_GNU_SOURCE` makes musl declare GNU extensions that `libc.a` does not export; a program that uses one
  fails to link instead of to compile.
- **Cost:** *(measured)* five lines of `tools/zcc`.
- **Precedent:** clang and GCC on Linux (`_GNU_SOURCE` for C++ always).
### Option B: `-std=gnu++23`
- **Evidence for:** musl's default feature set without a macro.
- **Evidence against:** a project that asks for `-std=c++17` still breaks (driver 3); GNU dialect extensions on by default.
### Option C: `_XOPEN_SOURCE=700` and `_BSD_SOURCE` (musl's own default set)
- **Evidence against:** libc++ needs `strtod_l` and the other `_GNU_SOURCE` declarations on musl (measured).
### Option D: clang's default (`gnu++17`)
- **Evidence against:** the task names C++23; C++17 lacks what newer code uses.

## Recommendation
**Recommendation (not a decision): Option A.** `zc++` adds `-std=c++23` when the command line has no `-std=` and no `-x`, and
`-D_GNU_SOURCE` always (a later `-U_GNU_SOURCE` of the caller still wins).

## Consequences
- **Positive.** libc++'s headers compile under every standard; CMake's `CXX_STANDARD` works.
- **Negative.** GNU-only names are declared that `libc.a` may lack.
- **Follow-up.** None.

## Invariants
- **INV-0364-1.** `zc++` without `-std=` compiles as C++23 (`__cplusplus` is `202302L`). *Check:* `make test-cxx`
  (`tests/cxx/hello.cpp` is built without `-std=` and has `static_assert(__cplusplus == 202302L)`).

## Open questions
- None.

## Revisit triggers
- C++26 is the norm in the ports' upstreams.

## Architect's decision
