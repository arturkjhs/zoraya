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

## Second review

Independent review by the `opus-review` subagent (2026-11-14) of the delta since the first review, `git diff a8b2106 e455f78`
(the order of a thread's exit for LWG 3343: `__cxa_thread_atexit_impl` in `libc.a`, musl patch `0003`, libc++abi built with
`HAVE___CXA_THREAD_ATEXIT_IMPL`, INV-0366-4; the TCG split of the libc++ subset; the CI and Makefile changes), and again of
`malloc`'s offset header (`crates/zoraya-posix/src/ffi.rs`), the 52 added names and the weak shims, after `origin/main` was
merged into `work/cxx`. The reviewer ran `make check` (pass), `make test-cxx` (HVF, 87 steps), `make test-cxx-gtest` (166) and
`make test-cxx-libcxx` (1357), all passing after the fixes, and `make test-changed`: 15 steps passed; `test-boot` timed out once
on the console's `bytes dropped]` under a load average of 10 and passed alone; `test-hand-run` (and, in its lane, the apps,
NetSurf and Python steps), `test-posix-suites` and `test-posix-threadtest3` did not run, since this worktree's port cache lacks
the curl 8.23.0 and libc-test archives (`make fetch-ports` was not run; the candidate CI run covers them). The reviewer read
the patched musl sources in `build/ports/work/musl-1.2.5` and `libc.a`'s symbols (`nm`, `objdump -t`).

No must-fix finding.

### Findings

1. **should-fix (fixed)** `tests/cxx/cxx.script`, `ports/musl/zoraya/0003-thread-local-destructors-first.patch:23`,
   `ports/musl/zoraya/thread_atexit.c:43`: two behaviours the change claims had no test. `exit` running the calling thread's
   `thread_local` destructors first was never exercised: `cxxtest tls` and `exit-order` end worker threads only, and the main
   thread's `thread_local`s in those modes are trivial or untouched; a `libc.a` whose `exit.o` kept the weak dummy (for example a
   rename of the hidden name that missed one member) would pass every step. Nor was "a destructor may construct another
   `thread_local`" (the loop that takes the list from its head). Scenario: the main thread's `thread_local` destructors run
   after the static destructors, against [basic.start.term] p1, and nothing fails.
   **Resolution:** `cxxtest exit-tls`: a worker whose `thread_local`'s destructor constructs a second one (both destroyed before
   `join` returns), and the main thread, which constructs a `thread_local` first and then a static and an `atexit` handler; the
   script requires `outer`, `inner`, `atexit`, `static` in that order. libc++abi's fallback (the main thread's list as a static
   destructor registered at the first `thread_local`) would print `atexit` and `static` first. Passed under HVF; not run against
   a `libc.a` without the patch.
2. **should-fix (fixed)** `ports/musl/zoraya/thread_atexit.c:26` (was: `errno = ENOMEM; return -1`): the compiler's call to
   `__cxa_thread_atexit` ignores the result, so an allocation failure lost the destructor in silence. Scenario: under a full heap,
   a `thread_local` holding a lock or a `std::unique_lock` handed to the thread's exit is never destroyed, and another thread waits
   for ever. **Resolution:** `abort()`, as glibc's `__libc_fatal`.
3. **note** `thread_atexit.c:43`: a `thread_local` first used after the thread's list ran (from a pthread key destructor, an
   `atexit` handler or a static destructor) registers a destructor that never runs; its 24-byte node leaks. glibc behaves the
   same. Recorded in `docs/design.md`'s known limits.
4. **note** `thread_atexit.c:45`: a `thread_local` destructor that calls `pthread_exit` (or re-enables cancellation and is
   cancelled) re-enters `__pthread_exit`, which continues the list from the next node: every other destructor runs once and no
   node is freed twice, but the current node leaks and the rest of that destructor never runs. Undefined in C++; no fix.
5. **note** patch `0003:23`: `exit` does not disable cancellation (musl and glibc do not either), so a pending cancel acted on
   at a cancellation point inside a `thread_local` destructor run by `exit` ends that thread in the middle of `exit`; the
   `atexit` handlers then never run while other threads live. The same was already true of `atexit` handlers. Recorded in
   `docs/design.md`. (In `__pthread_exit` the destructors run with `canceldisable = 1`, set before the cleanup handlers, so
   `pthread_cancel` during them only interrupts a sleep or futex wait: a spurious wakeup the callers loop on.)
6. **note** `crates/zoraya-posix/src/ffi.rs:632`: `aligned_alloc(A, n)` allocates `n + A` at alignment `A` (the header sits just
   before the pointer). Correct, but `posix_memalign(4096, 4096)` costs two pages, and an alignment above a page also pays the
   large path's gap when the heap grows. Recorded in `docs/design.md`; a header placed at the block's end would not cost this.
7. **note** `ffi.rs:742`: `realloc` of an `aligned_alloc` block returns a 16-byte-aligned block. C and POSIX allow it, glibc does
   the same; nothing in the tree depends on otherwise.
8. **note** `Makefile:1021`, `tests/cxx/libcxx-tcg-excluded.txt`: CI's hosts have no HVF, so the libc++ subset runs there only
   with the TCG script, and upstream's `notify_all_at_thread_exit_lwg3343` never runs in CI, only on a Mac with HVF. INV-0366-4
   stays checked in CI by `cxxtest exit-order` (and now `exit-tls`) under TCG, so this is a loss of the second witness only.
9. **note** `ports/Makefile:470`: the LLVM tree no longer depends on the patch directory, so a removed `llvm/` patch is not seen
   (the comment says so). Acceptable while patches are only added; a list file of the patches' names would catch removal.
10. **note** ADR-0366 lines 41 and 59, and its index row: "51 names (49 of musl's, 2 of libposix's)", while its
    `posix-surface-added` block now holds 52 (`__cxa_thread_atexit_impl` is Zoraya's own C, built with musl's objects). The ADR
    is accepted, so an agent does not edit it; for the architect. `docs/design.md` and the report now say 52.

### Checked and found sound

- **Order of a thread's exit.** `__pthread_exit` (patched source read): cancellation cleanup handlers, then the `thread_local`
  list, then `__pthread_tsd_run_dtors` (where libc++'s `__thread_struct` notifies and makes states ready), as glibc's
  `start_thread` runs them. `exit`: the caller's list, then `__funcs_on_exit` (`atexit` and static destructors), `__libc_exit_fini`,
  `__stdio_exit`. `exit` from a thread other than the main one runs only that thread's list, as glibc. The last thread's
  `pthread_exit` calls `exit(0)` with its list already empty. `quick_exit`, `_Exit` and `abort` run none.
- **Linking.** `exit.o` and `pthread_create.o` define `__zoraya_thread_dtors` weak, `thread_atexit.o` strong, and libc++abi's
  `cxa_thread_atexit.o` pulls `thread_atexit.o` through `__cxa_thread_atexit_impl`, so the strong one wins whenever a
  destructor is registered; `thread_dtors` is in `.tbss`. The call log wraps `__cxa_thread_atexit_impl` only.
- **Concurrency and memory.** The list is reached only through the thread's own TLS, so it needs no lock; `malloc` and `free`
  take `MEM` themselves, and nothing calls `__cxa_thread_atexit_impl` with the library lock held. The head is advanced before a
  destructor is called, so each node is freed once, also when a destructor registers another. A `fork` child keeps the forking
  thread's list and runs it at its own exit (glibc does the same); a detached thread's list is freed before its TLS goes.
- **The allocator.** `layout_at` uses `checked_add`, and `Layout::from_size_align` refuses any size that rounds above
  `isize::MAX` (`aligned_alloc(1 << 63, 1)` returns null with `ENOMEM`); a non-power-of-two alignment is `EINVAL`;
  `posix_memalign(8, ...)` is clamped to 16 inside `alloc_aligned`; `free` rebuilds exactly the layout allocated; `calloc` checks
  its product; the heap's large path checks its own rounding (`crates/zoraya-heap`, `span_fits`, `take_span`).
- **The shims and CI.** INV-0366-2's `nm` filter keeps defined global names, so a weak name stays `W` and a local helper is not
  counted. The test jobs give every tracked file an old time, so the stamps of the libc++ tests hold without LLVM's tree in the
  artifact. `CXX_LIBCXX_TCG` picks the TCG script on any host that is not a Mac.
- **No trust boundary is touched.** The delta changes no description's grants (`cxxtest.cmd` gains a mode in its usage only),
  no service, and not the untrusted gate (`make check`'s `check-untrusted-gate` passes); C++ programs remain POSIX programs
  started by the shell.
