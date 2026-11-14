# TASK-30 review: C++ for Zoraya programs

Independent review by the `opus-review` subagent of `git diff origin/main...work/cxx` at the commit "std::thread::join of the
calling thread throws" (2026-11-14). The reviewer ran `make adr-check`, `make check-ci-plan` and `make check-ci-jobs` (pass), and
no QEMU. The reviewer's findings follow, with what the implementing session did about each (**Resolution**).

## Findings

1. **warning** `crates/zoraya-posix/src/ffi.rs` (`alloc_aligned`): a safe `fn` that is sound only for `align >= HEADER`. With
   `align = 8` it writes the header 8 bytes before the heap block. Every caller clamped; a later `memalign` or `valloc` added the
   same way would corrupt the heap without any `unsafe` at its call site.
   **Resolution:** fixed. `alloc_aligned` now clamps `align.max(HEADER)` itself; the callers pass `align` as it is.
2. **warning** `tests/cxx/cxxtest.cpp` (`alloc`) and INV-0366-3: nothing checks that `free` gives an aligned block back with the
   layout it was allocated with. `free` falling back to the old `layout(n)` would pass every alignment check.
   **Resolution:** fixed. 4,000 rounds of `aligned_alloc(4096, 8192)` and `posix_memalign(256, 1000)` with `free` (a wrong layout
   runs the heap out or corrupts it), and `realloc` of an aligned block keeps its bytes. A host test of the heap with
   `layout_at`'s layouts is not added: `ffi.rs` is target-only.
3. **warning** `tools/zcc`: `--eh-frame-hdr` was added only with the default libraries. A `-nodefaultlibs` or `-nostdlib` C++
   link that names the runtime itself had no `PT_GNU_EH_FRAME`, so its first `throw` ended in `std::terminate` (INV-0361-1).
   **Resolution:** fixed. Every C++ link gets `--eh-frame-hdr`; `ports/cxx/cxx.mk`'s check fails if a `-nodefaultlibs` link of
   `zc++` lacks it.
4. **warning** `Makefile`: the rule `$(BUILD)/ports/obj/cxx.stamp:` had no prerequisites, so the libc++ test binaries were
   judged on an old stamp, and a changed patch, shim or `libc.a` ran old binaries under `make test-changed`.
   **Resolution:** fixed. `cxx-libcxx-tests` builds the runtime first, then the stamp. The stamp depends on the four archives.
5. **warning** INV-0364-1: "`__cplusplus` is `202302L`" was not tested; `atomic::wait` needs only C++20.
   **Resolution:** fixed. `tests/cxx/hello.cpp` (built without `-std=`) has `static_assert(__cplusplus == 202302L)`.
6. **warning** `ports/Makefile` `check-ports` (INV-0109-2, INV-0143-1): it grepped only the C ports' logs and probed `zcc`, not
   `zc++`, `cxx.log` or GoogleTest's CMake compile lines.
   **Resolution:** fixed, in the C++ rules: `check-ports` runs before them in `make ci-build`.
   - `cxx.mk` refuses a `zc++` that accepts `-D__linux__` or predefines a Linux macro in C++.
   - The `gtest` rule builds with `VERBOSE=1`. It requires `zc++` in `gtest.log` and no `-D__linux` in `gtest.log` or `cxx.log`.
7. **warning** `tests/cxx/gtest.sh`, `tests/cxx/libcxx-build.sh`: a failing program made the runner wait its whole timeout.
   **Resolution:** partly fixed.
   - The libc++ script forbids `posixrun: exit 1`, `exit 2`, `ended` and `Assertion failed`.
   - The GoogleTest script does not. It has no pattern that every program can carry: `gtest_stress_test` fails on purpose and
     prints `FAILED TESTS`, and GoogleTest prints its list of failures after the summary line. A failing GoogleTest program is
     still seen, by its status or at the runner's timeout (1500 s, below the job's 45 minutes); this is recorded in `gtest.sh`.
8. **note** `ports/cxx/zoraya-shim.c`: "weak so that libc.a's definition wins" is not what a linker does: an archive member is
   never pulled to replace a weak definition already there.
   **Resolution:** the shim's comment and ADR-0366 now state the real condition: a name libposix comes to export is removed
   from the shim.
9. **note** `tools/zcc`: only a separate `-x` was seen. `-xc` got `-std=c++23` (clang refuses that for C), and `-x c++` got no
   standard.
   **Resolution:** fixed. `-x LANG` and `-xLANG` are read. `-std=c++23` is added unless a standard is named or the language is
   not C++ (checked by hand: `-x c`, `-xc` and `-x assembler-with-cpp` give no `__cplusplus`; `-x c++` and `-xc++` give
   `202302L`; `-std=c++17 -x c++` gives `201703L`).
10. **note** `docs/adr/index.md`, ADR-0366: "51 names of musl's code and libposix's" miscounted (49 and 2), and the patch count
   left out `llvm/0001`.
   **Resolution:** fixed.
11. **note** `.github/workflows/test.yml`: the build artifact grows by the libc++ test binaries (about 240 MB before
   compression), the LLVM archive and its unpacked tree. The ports cache kept `googletest-build` (203 MB).
   **Resolution:** `googletest-build` is now left out of the cache, as it already was out of the artifact. The rest is open:
   the test jobs' `make` may need the tree to decide that nothing is out of date. This is to be measured on the first candidate
   run (TASK-30 report, open items).

## Found by the implementing session after the review (running libc++'s tests)

- `filesystem_test_helper.h` makes and removes its directories with `std::system("mkdir -p")` and `rm -rf`, and the test image
  has no shell. Patch `llvm/0004` (test support only) does the same with `std::filesystem`.
- 31 filesystem tests fail on differences of the platform, not of the runtime. The lenient run under HVF passed 450 of 481.
  The failures and their causes:
  - No enforced mode bits (ADR-0014).
  - FIFOs only in a `/tmp` entry, and no hard links (ADR-0013).
  - Lexical `..` (ADR-0013).
  - File times that never go back (ADR-0014 rule 3).
  - No `statvfs`.

  They are listed in `tests/cxx/libcxx-excluded.txt` with these reasons. One case is not fully analysed: `weakly_canonical` and
  `relative` end in `ENOENT` for a path with missing components. It is recorded in the report.

## Checked and found sound (the reviewer's list, condensed)

- **The allocator.** `malloc`'s layout is unchanged (`n + 16`, alignment 16). `free` and `realloc` rebuild the layout that was
  allocated. On overflow `malloc` returns null with `ENOMEM`; `n = 0` gives a unique block; an alignment larger than a page
  goes through the heap's large path. `posix_memalign` leaves `errno` alone. No other path in libposix allocates or frees C
  blocks.
- **The C path of `zcc`** is unchanged, and the `__linux__` refusals cover `zc++` too.
- **The port.** musl's static `dl_iterate_phdr` works with libposix's auxiliary vector, and the join patch is correct.
- **The build and CI.** INV-0365-1's exclusion holds. The stamps rebuild in order. CI's lists agree.
- **The libc++ subset.** The lit handling is conservative.
- **Grants.** The descriptions grant nothing beyond the POSIX default set, and the untrusted gate is untouched.
- **The ADRs.** ADR-0360 to ADR-0367 are all `proposed`, with an empty decision section.
