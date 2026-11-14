# ADR-0367: A CMake toolchain file for Zoraya: `CMAKE_SYSTEM_NAME Zoraya` with a platform module of its own

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0033, ADR-0109, ADR-0360, ADR-0364
- **Evidence:** CP-27

## Context
TASK-30 asks for `ports/cmake/zoraya.cmake`, so that CMake projects (LLVM, ICU's and Skia's builds, GoogleTest) cross-build
without edits. CMake decides a platform's conventions from `CMAKE_SYSTEM_NAME`, by loading `Platform/<name>.cmake`; for a name
it does not know it warns and uses nothing. Facts of the target: programs are static ET_EXEC (ADR-0033), `zcc` refuses `-shared`,
`-rdynamic` and `-pie`, POSIX programs use libposix (so `UNIX` holds), and nothing built runs on the host.
Measured: GoogleTest 1.18.0 configures and builds with the file (`make -f ports/Makefile gtest`: 29 programs, its own tests
among them) with one patch, which is GoogleTest's own list of systems that have pthreads and death tests.

## Decision drivers
1. A project's `if(UNIX)`, `find_package(Threads)` and `add_library` work as on a Unix (driver of TASK-30: "without edits").
2. Nothing tells CMake a lie it acts on: no shared libraries, no `-rdynamic` (zcc refuses it).
3. The file works from the repository's checkout with the sysroot `make -f ports/Makefile cxx` built.
4. Upstream build files unchanged where they can be (CP-27).

## Considered options
### Option A: `CMAKE_SYSTEM_NAME Zoraya` and `ports/cmake/Platform/Zoraya.cmake` (the recommendation)
- **Description:** the toolchain file names `zcc`, `zc++`, `zoraya-ar`, the sysroot as `CMAKE_FIND_ROOT_PATH` (programs from the
  host, the rest from the sysroot only) and adds its directory to `CMAKE_MODULE_PATH`; the platform module sets `UNIX`, `ZORAYA`,
  CMake's Unix paths, `TARGET_SUPPORTS_SHARED_LIBS` false, `.a` only, and empties the `-rdynamic` and PIE flags.
- **Evidence for:** drivers 1 to 3; projects can test `if(ZORAYA)`.
- **Evidence against:** CMake warns once that a `SHARED` library is built static (GoogleTest asks for one in its own tests).
- **Cost:** *(measured)* 40 and 25 lines.
- **Precedent:** Haiku's and QNX's platform modules in CMake itself (alternatives-postmortem: QNX, Haiku).
### Option B: `CMAKE_SYSTEM_NAME Linux` (the baseline)
- **Evidence against:** projects take Linux-only paths (`/proc`, `epoll`, `-rdynamic`), against INV-0109-2's spirit and driver 2.
### Option C: `CMAKE_SYSTEM_NAME Generic` (CMake's bare-metal)
- **Evidence against:** `UNIX` is not set, so projects drop their POSIX code paths (driver 1).
### Option D: no toolchain file; each port passes `CMAKE_C_COMPILER` and the rest by hand
- **Evidence against:** the task; every port repeats it.

## Recommendation
**Recommendation (not a decision): Option A**, as `ports/cmake/zoraya.cmake` and `ports/cmake/Platform/Zoraya.cmake`. A project
whose `try_run` checks need answers gets them in the cache from the port (as `PYTHON_CACHE` does for configure).

## Consequences
- **Positive.** GoogleTest builds unchanged apart from its platform list; the next CMake ports start from it.
- **Negative.** `try_run` checks fail without answers; shared-library targets become static.
- **Follow-up.** Upstream the platform module to CMake once the system's name is stable.

## Invariants
- **INV-0367-1.** A CMake project builds with the toolchain file and its programs run inside Zoraya. *Check:* `make
  test-cxx-gtest` (GoogleTest built by `make -f ports/Makefile gtest` with the file).

## Open questions
- Whether `CMAKE_CROSSCOMPILING_EMULATOR` should run a test program in QEMU for `try_run` (a boot per check; not tried).

## Revisit triggers
- A CMake port that needs shared libraries, or `try_run` answers in bulk.

## Architect's decision
