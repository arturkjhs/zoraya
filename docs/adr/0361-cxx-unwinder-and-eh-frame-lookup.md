# ADR-0361: The unwinder and how a static program finds its `.eh_frame`: libunwind over `dl_iterate_phdr` and `PT_GNU_EH_FRAME`

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0033, ADR-0100, ADR-0180, ADR-0303, ADR-0360, ADR-0366
- **Evidence:** CP-27

## Context
An exception unwinds the stack frame by frame with the DWARF call-frame information of `.eh_frame`. The unwinder must find
the frame description of each return address. A static program has one module, the program itself. Three ways exist:
(1) `dl_iterate_phdr` reports the program headers, and the `PT_GNU_EH_FRAME` header names `.eh_frame_hdr`, a sorted table of
the frame descriptions (what Linux and the BSDs do); (2) the program calls `__register_frame` at start with the address of
`.eh_frame` (GCC's old crtbegin did, through `__EH_FRAME_BEGIN__`); (3) linker-script symbols `__eh_frame_start` and
`__eh_frame_hdr_start` (libunwind's bare-metal configuration, `_LIBUNWIND_IS_BAREMETAL`).

What exists: libposix lays out the auxiliary vector with `AT_PHDR`, `AT_PHENT` and `AT_PHNUM` (ADR-0180; `zoraya_posix_start`),
and rust-lld emits `PT_PHDR`, so musl's static `dl_iterate_phdr` (`src/ldso/dl_iterate_phdr.c`, which reads only that vector
and `__tls_get_addr`) works unchanged. lld writes `.eh_frame_hdr` and `PT_GNU_EH_FRAME` with `--eh-frame-hdr`, which clang's
bare-metal driver does not pass. Measured: `hello` has a `PT_GNU_EH_FRAME` of 28 KiB; exceptions thrown 20 frames down, out of a
`std::thread` and through `std::async` are caught (`make test-cxx`). musl's own objects are built with
`-fno-asynchronous-unwind-tables` and have no frame descriptions.

## Decision drivers
1. No new mechanism where the platform's own works (CP-27): the auxiliary vector already says where the program headers are.
2. No linker script: every program is linked the same way (ADR-0033, `zcc`).
3. A lookup that is a binary search, not a linear walk, for programs of browser size.
4. Threads unwind at once (libunwind's lock-free `dl_iterate_phdr` path, no registration lock at start).

## Considered options
### Option A: `dl_iterate_phdr` from musl, `--eh-frame-hdr` in `zc++` (the recommendation)
- **Description:** `ports/musl/musl.mk` compiles `src/ldso/dl_iterate_phdr.c`; `libc.a` exports `dl_iterate_phdr` (ADR-0366);
  `zc++` links every C++ program with `-Wl,--eh-frame-hdr`; libunwind is built in its default ELF configuration
  (`_LIBUNWIND_USE_DL_ITERATE_PHDR`, `_LIBUNWIND_SUPPORT_DWARF_INDEX`) with `_LIBUNWIND_USE_DLADDR=0` (no `dladdr`: there is no
  loader, so `unw_get_proc_name` finds no names).
- **Evidence for:** drivers 1 to 4; unchanged libunwind and musl code; the Linux path, so libunwind's own tests apply.
- **Evidence against:** one more export; a program linked by `zcc` (C) with C++ objects needs `--eh-frame-hdr` itself (CMake
  links C++ with `zc++`).
- **Cost:** *(measured)* one line of `musl.mk`, one of `zcc`, one export.
- **Precedent:** Linux, FreeBSD, Fuchsia (its libunwind finds modules through `dl_iterate_phdr` in its libc).
### Option B: `__register_frame` from a start-up object
- **Description:** a crt object placed first registers the program's `.eh_frame` with libunwind's `__register_frame`.
- **Evidence against:** needs the start of `.eh_frame` (a symbol the linker does not give without a script or a section-order
  trick); a linear list searched under a lock; GCC dropped it for `PT_GNU_EH_FRAME`.
### Option C: libunwind's bare-metal configuration with linker-script symbols
- **Evidence against:** a linker script for every program (driver 2); `INSERT` scripts with lld are fragile across versions.
### Option D: ARM's EHABI or SJLJ exceptions
- **Evidence against:** EHABI is 32-bit ARM's; SJLJ costs every `try` at run time and is not clang's default for AArch64.

## Recommendation
**Recommendation (not a decision): Option A.** libunwind finds `.eh_frame_hdr` through `dl_iterate_phdr` and
`PT_GNU_EH_FRAME`; `zc++` passes `--eh-frame-hdr`. An exception thrown through a C function of `libc.a` (a `qsort`
comparator that throws) ends in `std::terminate`, because musl's objects have no frame descriptions; this is recorded as a limit.

## Consequences
- **Positive.** Exceptions and `std::exception_ptr` work across frames and threads; no start-up registration.
- **Negative.** No unwinding through `libc.a`'s callbacks; no function names in libunwind's backtraces.
- **Follow-up.** musl with unwind tables if a port throws through `qsort` or `bsearch` (about 10% more text in `libc.a`, not measured).

## Invariants
- **INV-0361-1.** Every C++ program `zc++` links has a `PT_GNU_EH_FRAME` header. *Check:* `make test-cxx` (an exception thrown
  and caught in each test program; without the header the first throw ends in `std::terminate`).

## Open questions
- Whether libunwind's frame-header cache (`_LIBUNWIND_USE_FRAME_HEADER_CACHE`) is worth turning on for a browser (not measured).

## Revisit triggers
- A dynamic loader (ADR-0100 changes): `dl_iterate_phdr` would report several modules.
- A port that throws through C callbacks.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

As recommended.
