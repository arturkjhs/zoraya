# ADR-0366: The names `libc.a` adds for C++, and the gaps libc++ fills in its own archive

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0005, ADR-0013, ADR-0100, ADR-0141, ADR-0227, ADR-0308, ADR-0310, ADR-0320, ADR-0360, ADR-0361
- **Evidence:** CP-27

## Context
TASK-30 builds LLVM's libc++, libc++abi and libunwind (ADR-0360) against `libc.a`. `libc.a` exports the tier list and the
added blocks (ADR-0141); the rest of musl is hidden as `__zoraya_*`. Measured by linking a program with the whole of the three
archives (`--whole-archive`, so that every member's references count, not only those `hello` reaches): 53 names are undefined.
46 of them are musl's own code in the directories `libc.a` already compiles (the wide-character and `*_l` locale functions,
`strtod_l` and its two siblings, `swprintf`, `vsscanf`, `getwc`, `ungetwc`, `fputwc`, `btowc`, `wctob`, `mbtowc`, `mbsnrtowcs`,
`wcsnrtombs`, `isspace`, `getentropy`) or in a file of a directory it does not compile (`dl_iterate_phdr`, `src/ldso`); none
makes a system call that libposix does not serve (`getentropy` calls `getrandom`, ADR-0310 item 2). Seven are not musl's to give
here: `link` (hard links are never created, ADR-0013), `statvfs` and `pathconf` (libposix has neither), `truncate` (libposix has
`ftruncate` only), `sched_yield` (libposix's is `zoraya_posix_sched_yield`, ADR-0308 item 4), `sendfile` (Linux's, for
`std::filesystem::copy_file`), and `aligned_alloc` (libposix's `malloc` is its own and had no aligned form, nor
`posix_memalign`). The tests add three more of musl's: GoogleTest's `wcsstr`, `towlower` and `execv` (`src/process/execv.c`,
`execve` with `environ`, for its death tests), and libc++'s own tests' support code calls `aligned_alloc` and `posix_memalign`
directly (`count_new.h`, 22 tests).

Running libc++'s tests found one more gap that is not a name: musl's `pthread_join` of the calling thread waits for ever (POSIX
leaves it undefined, and allows `EDEADLK`), while C++ requires `std::thread::join` to throw `resource_deadlock_would_occur`
(`thread.jthread/join.deadlock.pass.cpp` hung).

## Decision drivers
1. The surface of `libc.a` changes only by ADR, with a program behind each name (INV-0005-1, CP-27).
2. A name `libc.a` exports must do what POSIX or C says, not report a success that did not happen (ADR-0320 item 3's rule for shims).
3. Every C++ program links the same runtime: a gap filled for one is filled for all, unlike CPython's `libpyshim.a`, which
   only `python3` links.
4. The fewer changes to LLVM's sources, the cheaper the next release (CP-27).

## Considered options
### Option A: musl's names and libposix's aligned allocation into `libc.a`, the rest in a weak shim inside `libc++.a`, two patches (the recommendation)
- **Description:** the 49 names of musl's code are added in the block below (and `src/ldso/dl_iterate_phdr.c` and
  `src/process/execv.c` are compiled, `ports/musl/musl.mk`). libposix gains `aligned_alloc` and `posix_memalign`: `malloc`'s
  16-byte header keeps the size asked in its first word and now the distance from the heap block's start to the pointer in its
  second, which is also the block's alignment (16 for `malloc`); `free` reads both and gives the block back with the layout it
  was allocated with. `ports/cxx/zoraya-shim.c`, a member of `libc++.a`, defines `sched_yield` (libposix's own), `truncate`
  (`open` and `ftruncate`), `pathconf` (`PATH_MAX` and `NAME_MAX` of `<limits.h>`, `EINVAL` for the others), `link` and
  `statvfs` (`ENOTSUP`), each weak so that a later `libc.a` definition wins. Patch `llvm/0002` makes `copy_file` take
  libc++'s `fstream` copy instead of `sendfile`; patch `llvm/0003` makes `std::thread::join` of the calling thread throw
  `resource_deadlock_would_occur` before it calls `pthread_join`.
- **Evidence for:** drivers 1 to 4; every name has a user (libc++'s locale, `iostream`, `filesystem`, `random_device`, libunwind,
  aligned `new`, GoogleTest); C programs gain aligned allocation too.
- **Evidence against:** two places a name can live; `std::filesystem::create_hard_link` and `space` always fail; `malloc`'s header
  changed for every program (unsafe code in the allocator, reviewed with this part).
- **Cost:** *(measured)* 70 lines of C, 60 lines of Rust in libposix, two patches of 13 and 22 lines, 51 names.
- **Precedent:** ADR-0310 (a block of musl's names for ports) and ADR-0320 (a shim beside the block).
### Option B: everything in `libc.a`
- **Description:** also `link`, `statvfs`, `pathconf`, `truncate` and `sched_yield` exported by libposix.
- **Evidence for:** one place for every name.
- **Evidence against:** `link` and `statvfs` would be exported names that always fail, which ADR-0310's rule avoids; the shim's
  five are what libc++ needs, and a libposix `truncate` and `pathconf` are a follow-up of their own.
- **Cost:** *(estimate)* 60 lines of Rust and a libposix review.
### Option C: patch libc++ until it names nothing outside the list, aligned allocation over `malloc` in libc++'s helpers
- **Evidence against:** the locale code of libc++ is written against the `*_l` functions; patching it would be most of the
  file and be re-done at each release (driver 4). An aligned-allocation patch of libc++'s helpers was built and worked, but left
  C code and libc++'s own tests without `aligned_alloc`; it was dropped for Option A's libposix functions.
### Option D: the Linux baseline: glibc's or musl's whole surface exported
- **Evidence against:** INV-0005-1; exports for calls libposix cannot serve.
- **Precedent:** every Linux distribution.

## Recommendation
**Recommendation (not a decision): Option A.** The names below join `libc.a`'s exports; the shim and the two patches are the
port's. musl's self-join is left as it is for C programs (POSIX allows either); only C++'s `join` checks first.

### Names `libc.a` adds
```posix-surface-added
aligned_alloc
btowc
dl_iterate_phdr
execv
fputwc
freelocale
getentropy
getwc
isdigit_l
islower_l
isspace
isupper_l
iswalpha_l
iswblank_l
iswcntrl_l
iswdigit_l
iswlower_l
iswprint_l
iswpunct_l
iswspace_l
iswupper_l
iswxdigit_l
isxdigit_l
mbsnrtowcs
mbtowc
posix_memalign
strcoll_l
strftime_l
strtod_l
strtof_l
strtold_l
strxfrm_l
swprintf
tolower_l
toupper_l
towlower
towlower_l
towupper_l
ungetwc
vsscanf
wcscoll_l
wcsnrtombs
wcsstr
wcstod
wcstof
wcstold
wcstoll
wcstoul
wcstoull
wcsxfrm_l
wctob
```

## Consequences
- **Positive.** `iostream`, `locale`, `regex`, `filesystem`, `random_device` and exceptions link and run; C programs gain the
  same wide-character and `*_l` functions and aligned allocation.
- **Negative.** 51 more exports; `std::filesystem::create_hard_link` and `space` fail with `ENOTSUP`.
- **Follow-up.** `statvfs` when the filesystem service reports free space; `truncate` and `pathconf` in libposix.

## Invariants
- **INV-0366-1.** A program linked with the whole of `libc++.a`, `libc++abi.a` and `libunwind.a` has no undefined name.
  *Check:* `make -f ports/Makefile cxx` (the `--whole-archive` link of `ports/cxx/cxx.mk`).
- **INV-0366-2.** The shim's names are weak and are only `sched_yield`, `truncate`, `pathconf`, `link`, `statvfs`.
  *Check:* `make -f ports/Makefile cxx` (`nm` of the shim's member).
- **INV-0366-3.** `aligned_alloc` and `posix_memalign` give blocks of the alignment asked that `free` takes back.
  *Check:* `make test-cxx` (`cxxtest alloc`) and `make test-cxx-libcxx` (`support.dynamic`'s aligned `new` and `delete`,
  through `count_new.h`'s `aligned_alloc`).

## Open questions
- Whether libposix should report free space (`statvfs`); the filesystem service's `status` has the numbers (ADR-0210).

## Revisit triggers
- libposix gains `statvfs`, `truncate` or `pathconf` (the shim's member goes).
- A libc++ release that names more of libc.

## Architect's decision
