# ADR-0454: Rust panics unwind through the sysroot's libunwind, the one unwinder C++ uses; a stack overflow is a fault

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0106, ADR-0360, ADR-0361, ADR-0362, ADR-0450, ADR-0451, ADR-0452
- **Evidence:** alternatives-postmortem.md#theseus, rust-std/survey.md#building-on-stable, rust-std/survey.md#the-probes, rust-std/survey.md#what-std-links

## Context

Rust's `std` for `aarch64-unknown-linux-musl` is built with `panic=unwind`; its `unwind` crate links `-lunwind`. With
`-C link-self-contained=no` (ADR-0452) that is the sysroot's LLVM 20.1.8 `libunwind.a`, the unwinder of C++ programs
(ADR-0360), which finds `.eh_frame_hdr` through `PT_GNU_EH_FRAME` and musl's static `dl_iterate_phdr` (ADR-0361).
`rustc` passes `--eh-frame-hdr` itself (rust-std/survey.md#building-on-stable). Measured
(rust-std/survey.md#the-probes): `catch_unwind` of a panic 20 frames down ran all 20 destructors; `resume_unwind` kept
its payload; a panic in a thread reached `join`; a lock was poisoned; an uncaught panic printed its message and ended with
exit status 101; `process::abort` ended "by signal 6"; unbounded recursion ended "crashed (data abort, class 0x24)" with
no Rust message, because `std`'s guard handler needs `sigaltstack` and a `SIGSEGV` handler and Zoraya runs no handler on
a fault (ADR-0106 item 12; ADR-0451 item 4 makes `sigaltstack` fail). `Backtrace::force_capture` captured six frames
without names in a release build without debug information. Panic messages print the thread id as
18446744073709551615 until `gettid` is answered (ADR-0451) (rust-std/survey.md#what-std-links).

Theseus recovers from faults by unwinding, and failed to recover from 31% of the injected hardware faults that
manifested, mainly because Rust and LLVM emit only synchronous unwind tables (alternatives-postmortem.md#theseus).

## Decision drivers

1. **One unwinder per process**: Rust and C++ frames in one program (Ladybird) must be unwound by the same library
   with the same frame lookup.
2. **Behaviour a person can read**: the shell's existing words for an exit status, a signal and a fault (ADR-0362).
3. **No fault handlers** (ADR-0106): a fault ends the process; nothing runs on it.
4. **Measured** (rust-std/survey.md#the-probes).

## Considered options

### Option A: Rust's own bundled `libunwind.a` (the Linux musl default)

- **Description:** `link-self-contained` left to `rustc`, which on musl brings its own `crt` objects, `libc.a` and
  `libunwind.a`.
- **Evidence for:** What the target does on Linux.
- **Evidence against:** A second `libc.a` (not Zoraya's) and a second unwinder beside C++'s (driver 1).
- **Cost:** None to build; a broken program.
- **Precedent:** Linux musl programs.

### Option B: `panic=unwind` through the sysroot's `libunwind.a`

- **Description:** Items 1 to 5 below.
- **Evidence for:** Measured working; one unwinder with C++ (driver 1).
- **Evidence against:** A stack overflow gives a fault, not Rust's message.
- **Cost:** None beyond ADR-0452's flags.
- **Precedent:** C++ on Zoraya (ADR-0361).

### Option C: `panic=abort` for every Rust program

- **Description:** No unwinding; a panic aborts.
- **Evidence for:** Smaller programs; no unwind tables needed.
- **Evidence against:** `catch_unwind`, thread panics seen by `join` and test harnesses stop working; crates rely on them.
- **Cost:** None.
- **Precedent:** embedded Rust.

### Option D: Unwinding as fault recovery (Theseus)

- **Description:** Catch faults and unwind the faulting thread, as Theseus does.
- **Evidence for:** A stack overflow or a bad access could become a panic.
- **Evidence against:** Needs fault handlers (driver 3); 31% of manifested faults were not recovered in Theseus
  (alternatives-postmortem.md#theseus).
- **Cost:** *(estimate)* A kernel and libposix design of its own.
- **Precedent:** Theseus.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`panic=unwind` by default**, through `build/sysroot/usr/lib/libunwind.a`; `link-self-contained=no` is what keeps
   Rust's bundled copy out (ADR-0452 item 2). A program may choose `panic=abort` in its profile.
2. **An uncaught panic** prints `std`'s message on standard error and exits with status 101; the shell says "exit status
   101". `abort` is `SIGABRT` ("ended by signal 6"), as for C++ (ADR-0362).
3. **A stack overflow** is a fault ("crashed (data abort, ...)"), with no Rust message; recorded as a known limit.
4. **A panic across a C or C++ frame** follows Rust's rules for `extern "C"` (it aborts) `[unverified: Rust's reference
   was not fetched]`; a C++ exception through Rust frames is not supported. Mixed programs catch at their boundaries.
5. **Backtraces** have names only with symbols in the program (a build with debug information or without stripping);
   measured without names in the release build.

### Effect of the vision

For target 2, a panicking agent tool ends with a status the supervisor can read, never in a handler. Target 3: Rust and
C++ in one engine share one unwinder. Networking: none. Threads: a thread's panic reaches `join`.

### Examples

```
$ rust-tool
thread 'main' (7) panicked at src/main.rs:12:5:
called `Option::unwrap()` on a `None` value
$ status
exit status 101
```

## Consequences

- **Positive.** `catch_unwind`, `join` errors and poisoning work; one unwinder for Rust and C++.
- **Negative.** No stack-overflow message; unnamed backtraces in stripped programs.
- **Follow-up.** The panic checks of `make test-rust-std`; the limits in `docs/design.md`.

## Invariants

- **INV-0454-1.** A Rust program's unwinder is the sysroot's `libunwind.a`, never Rust's bundled one. *Check:*
  `make test-rust-std` reads the link line `zcargo -v` printed and fails if a library from the toolchain's
  `self-contained` directory appears in it.
- **INV-0454-2.** A panic unwinds with every destructor run and ends an uncaught program with exit status 101.
  *Check:* `make test-rust-std` (the probe's `panic` and `uncaught` checks, asserted).

## Open questions

- Whether `std`'s backtrace finds names through `/proc/self/exe` when the program has symbols; one build with debug
  information answers it.

## Revisit triggers

- Zoraya gains fault handlers (ADR-0106 revisited): the stack-overflow message becomes possible.
- A port needs C++ exceptions through Rust frames.

## Architect's decision
