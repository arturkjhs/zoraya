# TASK-30: C++ for Zoraya programs

## Where, who and how

On the Mac mini, run by the TASK-27 dispatcher as its next part beside 9f (two building parts at most, `CARGO_BUILD_JOBS=3`, disk ≥ 25 GB before each build, worktrees removed after merge). Same models and rules as TASK-27: a Sonnet subagent per step in its own worktree, `opus-review` for the review and for bugs two attempts did not fix, Haiku for CI and logs. The architect is travelling: **do not ask; decide, record, continue.** ADR numbers **0360 to 0379**. Branch `work/cxx`, tag `stage-cxx`.

## Why

Every large program still missing is C++: a modern browser engine (WPE WebKit or Ladybird), LLVM itself, CMake, Ninja, many libraries (ICU, HarfBuzz, Skia). `zcc` builds C only. This part gives Zoraya a C++ toolchain and runtime, and a CMake toolchain file, so the next ports start from a working base.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (libposix, ports, `zcc`, threads), ADR-0100 to ADR-0110, ADR-0140 to ADR-0144 (libposix, the ports' method, `zcc`), ADR-0300 to ADR-0309 and ADR-0313 to ADR-0315 (threads, TLS, signals, `fork`), ADR-0320 (how CPython was ported: patches, shims, the surface block), `ports/Makefile`, `tools/zcc`.

## What to build

1. **The runtime:** LLVM's libc++, libc++abi and libunwind, static, from a pinned LLVM release (`ports/sources.lock` with SHA-256; choose the newest release the toolchain's clang builds, and record why), built by `zcc` into `build/sysroot`. Exceptions and RTTI on; threads over libposix's pthreads; `thread_local` with destructors (`__cxa_thread_atexit`); the C locale only; `std::filesystem` over libposix. Licence Apache-2.0 with LLVM exception (the permissive rule of `docs/dependencies.md`).
2. **`zc++`:** the C++ driver beside `zcc` (same target and sysroot, links the three libraries, `-std=c++23` by default unless a build says otherwise).
3. **A CMake toolchain file** (`ports/cmake/zoraya.cmake`) for `zcc`/`zc++`, so CMake projects cross-build without edits.
4. **Gaps found by running:** names libc++ or the tests need that `libc.a` lacks are added under the surface rules (a `posix-surface-added` block in this part's ADR), or shimmed in the port as CPython did, whichever the ADRs prefer.

Decisions to record as proposed ADRs (one per decision, template-exact): the library (libc++ over libstdc++), the unwinder and how a static program finds its `.eh_frame` (`dl_iterate_phdr` or `__register_frame`), what `std::terminate` and an uncaught exception print and return, locale support, the default standard, where the runtime and headers live in the image (if C++ programs are built inside Zoraya later).

## Done when

- Small test programs in `tests/cxx/`, built by `zc++` and run inside Zoraya: an exception thrown and caught across frames, across a `std::thread` boundary by `std::exception_ptr`, and uncaught (a message and the recorded status); RTTI and `dynamic_cast`; static constructors and destructors in order; `std::thread`, `mutex`, `condition_variable`, `future`; `thread_local` destructors; `iostream`; `std::filesystem` listing a directory the line grants (`+r`) and refused one it does not.
- A chosen subset of libc++'s own test suite passes inside Zoraya (record which and why; at least the exception, RTTI, thread and filesystem groups).
- GoogleTest, built by CMake with the toolchain file, runs its own tests inside Zoraya (death tests included, they use `fork`).
- `make test-cxx` under HVF and TCG, in CI; host tests for any new crate; the review by `opus-review` with findings in `docs/tasks/TASK-30-review.md`; candidate CI green; merged; tagged.

## Report

`docs/tasks/TASK-30-report.md` with a status block at the top: what is merged, the proposed ADRs with one-line recommendations, the sizes (`hello` in C++, the runtime libraries), the build times, what libc++ needed from `libc.a`, the known limits (what C++ programs still cannot do here), and what the next port (a browser engine's dependencies: ICU, HarfBuzz, FreeType, Skia) will need that this part did not add.
