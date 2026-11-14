# ADR-0365: Where the C++ runtime and headers live: the sysroot now, `/usr/include/c++/v1` and `/usr/lib` when Zoraya builds C++

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0108, ADR-0109, ADR-0310, ADR-0320, ADR-0360
- **Evidence:** CP-27

## Context
`build/sysroot` is what `zcc` builds against on the host; `make ports` copies its `usr/include` and `usr/lib` into
`build/rootfs-posix` as `/usr`, for `tcc` inside Zoraya (ADR-0109 item 6). TASK-30 installs libc++'s headers (12 MB, about
1,100 files) into `build/sysroot/usr/include/c++/v1`, libunwind's four headers into `usr/include`, and the three archives
(3.2 MB) into `usr/lib`. Nothing inside Zoraya compiles C++: `tcc` is a C compiler, and no clang runs in the guest.

## Decision drivers
1. The image holds what a program in it can use (ADR-0310's rule for `build/rootfs-apps`).
2. One layout, the one clang and CMake look for, so that a later compiler inside Zoraya finds the runtime without flags.
3. The volume's size: the POSIX image's `/usr` is copied into every test volume that boots the ports.
4. The POSIX layer's image stays as small as its scope (CP-27).

## Considered options
### Option A: the sysroot's standard places; out of the POSIX image until a C++ compiler is in it (the recommendation)
- **Description:** headers in `usr/include/c++/v1`, archives in `usr/lib`, in `build/sysroot` (host builds). `make ports` copies the
  C sysroot into `build/rootfs-posix` without the C++ files, which are removed after the copy. When a clang runs inside Zoraya,
  the same paths under `/usr` go into its image part, with that compiler.
- **Evidence for:** drivers 1 to 3; the layout clang's `-stdlib=libc++` and CMake expect on every Unix.
- **Evidence against:** a C++ program cannot be built inside Zoraya yet (no compiler).
- **Cost:** *(measured)* one line of `ports/Makefile`.
- **Precedent:** FreeBSD and macOS (`/usr/include/c++/v1`, `/usr/lib/libc++`).
### Option B: everything into `build/rootfs-posix` now
- **Evidence against:** 15 MB on every POSIX test volume for a compiler that is not there (drivers 1, 3).
### Option C: a separate sysroot (`build/sysroot-cxx`)
- **Evidence against:** two sysroots for one target; CMake's `CMAKE_FIND_ROOT_PATH` and `zcc` would name both.
### Option D: under the compiler's own directory (`/usr/lib/clang/N/`, as Fuchsia's SDK ships its runtime beside the toolchain)
- **Evidence against:** ties the runtime to one compiler version; Fuchsia's SDK (alternatives-postmortem) is a host artefact, not
  an image layout.

## Recommendation
**Recommendation (not a decision): Option A.** The sysroot holds the runtime at `usr/include/c++/v1` and `usr/lib/libc++.a`,
`libc++abi.a`, `libunwind.a`; the POSIX image does not, until a C++ compiler is ported into Zoraya.

## Consequences
- **Positive.** The volumes are unchanged in size; the paths are the standard ones.
- **Negative.** No C++ built inside Zoraya.
- **Follow-up.** clang inside Zoraya (a later port) takes `/usr/include/c++/v1` and the archives into its image part.

## Invariants
- **INV-0365-1.** `build/rootfs-posix/usr` holds no C++ header or archive. *Check:* `make -f ports/Makefile check-ports`.

## Open questions
- Whether a clang inside Zoraya should rather find the runtime through its resource directory (Option D).

## Revisit triggers
- A C++ compiler inside Zoraya.

## Architect's decision
