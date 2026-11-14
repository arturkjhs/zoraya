# TASK-30 report: C++ for Zoraya programs

## Status for the architect (2026-11-14, branch `work/cxx`, not merged)

Written by the implementing session; the numbers below come from commands run in this session on the Mac mini (M4,
QEMU 11.1.2, HVF) unless marked otherwise. Nothing is merged and no tag `stage-cxx` exists: the task's last steps (green
candidate CI, merge, tag) wait for the orchestrator.

**What is on `work/cxx`.** The runtime (LLVM 20.1.8's libc++, libc++abi and libunwind, static, `make -f ports/Makefile cxx`),
`zc++` (`tools/zc++`, a link to `tools/zcc`), the CMake toolchain file (`ports/cmake/zoraya.cmake` and
`ports/cmake/Platform/Zoraya.cmake`), the gaps of `libc.a` (ADR-0366), GoogleTest 1.18.0 built by CMake with the toolchain file,
three test steps (`make test-cxx`, `make test-cxx-libcxx`, `make test-cxx-gtest`, each under HVF here and HVF and TCG in CI),
the CI wiring (`tools/ci-tests.txt`, `TEST_STEPS`, `tools/plan-lib.sh`, `tools/test-map.txt`, the jobs' limits, `cmake` in the
CI image and the bare runner's tools, `make ci-build`), `docs/design.md` ("C++ for Zoraya programs (TASK-30)"), this report,
and the review (`docs/tasks/TASK-30-review.md`).

**Proposed ADRs** (every one `proposed`; recommendations, not decisions):

| ADR | Recommendation |
|---|---|
| [0360](../adr/0360-cxx-runtime-library.md) the library | LLVM's libc++, libc++abi, libunwind 20.1.8 (newest libc++ that supports CI's clang 18), static, exceptions, RTTI, threads, the C locale, filesystem, hardening `fast`. |
| [0361](../adr/0361-cxx-unwinder-and-eh-frame-lookup.md) unwinder and `.eh_frame` | libunwind over musl's static `dl_iterate_phdr` (the auxiliary vector) and `PT_GNU_EH_FRAME`; `zc++` passes `--eh-frame-hdr`. |
| [0362](../adr/0362-uncaught-exceptions-and-terminate.md) terminate | libc++abi's line on standard error, then `abort()`: "ended by signal 6". |
| [0363](../adr/0363-cxx-locale-support.md) locale | The C locale only; musl accepts other names and keeps the C rules. |
| [0364](../adr/0364-cxx-default-standard.md) default standard | `-std=c++23` unless the line names a standard or a language; `_GNU_SOURCE` always. |
| [0365](../adr/0365-cxx-runtime-and-headers-in-the-image.md) where it lives | The sysroot's `usr/include/c++/v1` and `usr/lib`; out of the POSIX image until a C++ compiler is in it. |
| [0366](../adr/0366-surface-for-cxx.md) the gaps (`posix-surface-added`) | 51 names of musl's code and libposix's new `aligned_alloc`, `posix_memalign` into `libc.a`; five weak shims in `libc++.a`; patches `llvm/0002` (`copy_file` without `sendfile`) and `llvm/0003` (self-join throws). |
| [0367](../adr/0367-cmake-toolchain-file.md) CMake | `CMAKE_SYSTEM_NAME Zoraya` with a platform module of its own (`UNIX`, static only, no `-rdynamic`). |

**Sizes.** `hello` in C++ (iostream and one exception): 1.0 MB stripped of `libc.a`'s debug information (3.5 MB with it).
`libc++.a` 2.5 MB, `libc++abi.a` 0.6 MB, `libunwind.a` 0.1 MB (no debug information); headers 12 MB in the sysroot.

**Build times** (Mac mini, `CARGO_BUILD_JOBS=3`, `-j3`): the runtime about a minute (`ports/cxx/cxx.mk`, after `libc.a`);
GoogleTest with its tests under a minute; the libc++ subset's 450 programs about 140 s (`make cxx-libcxx-tests`). CI's times: not yet
measured (no candidate run yet).

**What libc++ needed from `libc.a`** (ADR-0366): 46 names of musl's code found by linking the whole of the three archives
(`*_l` locale functions, wide-character I/O and conversions, `strtod_l`/`strtof_l`/`strtold_l`, `swprintf`, `vsscanf`,
`getentropy`, `dl_iterate_phdr`), three for GoogleTest (`execv`, `wcsstr`, `towlower`), `aligned_alloc` and `posix_memalign`
(new in libposix: `malloc`'s header now holds the block's offset and alignment), and five shims (`sched_yield`, `truncate`,
`pathconf`, `link`, `statvfs`). One behaviour: musl's `pthread_join` of the calling thread waits for ever; libc++ now throws
first (patch `llvm/0003`). The libc++ tests' filesystem helper uses `std::system` (no shell in the image): patch `llvm/0004`,
test support only.

**Tests run in this session (HVF, after the review's fixes):** `make test-cxx` passed (76 steps); `make test-cxx-gtest` passed
(166 steps: GoogleTest's 27 test programs, 4 of their tests filtered out because they change to `/`); `make test-cxx-libcxx`
passed (1358 steps: 450 of libc++'s run tests of the exception, RTTI, new/delete, thread and filesystem groups, in 99 s; 40 left
out: 7 by lit's directives, 2 that need `AF_UNIX`, 31 filesystem tests that failed on properties of the platform, listed with
their reasons in `tests/cxx/libcxx-excluded.txt`). `make check` passed. TCG was not run locally (CLAUDE.md: CI runs it); its
times are not measured, and CI's candidate run is the first.

**Known limits** (what C++ programs cannot do here): no C++ compiler inside Zoraya; no unwinding through `libc.a`'s callbacks
(`qsort` with a throwing comparator terminates); no names in libunwind's backtraces; named locales are the C locale; no time
zone database and no `libc++experimental`; `std::filesystem::space` and `create_hard_link` fail with `ENOTSUP`; a POSIX
program's namespace has no `/`, so `current_path("/")` fails; no `AF_UNIX` socket files; CMake `try_run` needs cached answers;
the libc++ version is held at 20 by CI's clang 18.

**What the next port (ICU, HarfBuzz, FreeType, Skia) will need that this part did not add:** a newer clang in CI if a
dependency needs C++ newer than libc++ 20 gives; `mmap` of files shared between processes and `madvise` (Skia's discardable
memory; libposix has private anonymous `mmap` only); `dlopen` is absent, so ICU's data must be linked in (`--with-data-packaging=static`);
FreeType and HarfBuzz are plain C/C++ and should build with the toolchain file as they are; Skia needs `pthread` with
`PTHREAD_STACK_MIN`-sized stacks and `posix_memalign` (now present); a browser engine needs `MAPPING_SLOTS` (256 per process,
ADR-0320's open question) raised or a heap that does not map 1 MiB at a time, JIT-free modes, `AF_UNIX` and server sockets for
its multi-process model, and `fork`-free process creation (`posix_spawn`).

## The work, in order

(Each step was committed and pushed to `origin/work/cxx`.)

1. Pinned LLVM 20.1.8 and GoogleTest 1.18.0 (`ports/sources.lock`). The three runtime archives were tried first; libc++ 20's
   `from_chars` includes LLVM libc's `shared/` headers, which only the whole-project archive has, so the pin is that archive and
   four directories of it are unpacked.
2. The runtime build (`ports/cxx/cxx.mk`, `ports/cxx/__config_site`) and `zc++`. `_GNU_SOURCE` was found necessary by building
   (a strict standard hides POSIX in musl; musl declares `strtod_l` only under `_GNU_SOURCE`).
3. `make test-cxx`. Two of my own test bugs were found by running (an allocation the compiler removed; `getline`'s cleared
   string), and musl's acceptance of every locale name was measured and recorded (ADR-0363).
4. GoogleTest by CMake with the toolchain file; `gtrun`, a described POSIX program that executes the test programs, because
   `posixrun` gives no `/tmp` and no broker for `fork`. A line typed while a program runs goes to its terminal, so the scripts
   wait for each program's last line.
5. libc++'s subset: `tests/cxx/libcxx-build.sh` reads lit's directives the simple way; one test hung on musl's self-join
   (patch `llvm/0003`); two need `AF_UNIX` (excluded); the support code needs `aligned_alloc`, which moved the aligned
   allocation from a libc++ patch into libposix; the filesystem helper's `std::system` became `std::filesystem` (patch
   `llvm/0004`). A lenient run (every test, status recorded) passed 450 of 481; the 31 that failed are filesystem tests
   that rely on enforced mode bits, FIFOs outside `/tmp`, hard links, non-lexical `..`, file times before now or `statvfs`,
   known properties of the filesystem service (ADR-0013, ADR-0014), excluded with their reasons; `weakly_canonical` and
   `relative` of a path with missing components end in `ENOENT`, not analysed further (an open item).
6. The independent review (`docs/tasks/TASK-30-review.md`): 11 findings, 10 fixed or documented, one (the artifact's size)
   open for the first CI run.

## Open items for the architect

- The ADRs above are proposed; none changes an accepted ADR. ADR-0366's change of `malloc`'s header touches every program
  (reviewed in `docs/tasks/TASK-30-review.md`).
- `std::filesystem::weakly_canonical` and `relative` of a path with missing components fail with `ENOENT`; lexical `..` in
  libposix is the likely cause, not confirmed.
- The build artifact grows by the libc++ test binaries (about 240 MB before compression) and the LLVM tree; to be measured on
  the candidate run (review finding 11).
- TCG times of the three C++ steps are unmeasured; CI runs both accelerators in one job of 45 (subset, GoogleTest) or 30
  (`test-cxx`) minutes.
- The libc++ version follows CI's clang: with Ubuntu's `clang-19` (or later) installed in the CI image, LLVM 21 or later could be pinned.
