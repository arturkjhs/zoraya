# ADR-0418: Toolchain for the engine: stay on clang 18 and libc++ 20 while the pinned WPE supports them; no Rust `std` and no Swift for now

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0109, ADR-0141, ADR-0360, ADR-0364, ADR-0365, ADR-0367, ADR-0410
- **Evidence:** CP-13, CP-27, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#servo, browser-engine/survey.md#the-engines-compared, browser-engine/survey.md#the-window-text-and-the-network, browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach, browser-engine/survey.md#wpe-webkit-in-detail

## Context

CI builds with Ubuntu's clang 18, so libc++ is held at 20.1.8, the newest that supports clang 18 (ADR-0360;
browser-engine/survey.md#the-window-text-and-the-network); the development Mac has Apple clang 21. C++23 is the default
(ADR-0364). WPE 2.54.2 sets C++23, accepts GCC 12.2 or newer (13.1 on `main`) and keeps workarounds because "Clang 18
does still need to be supported for the GTK and WPE ports", with a FIXME to drop them; it needs CMake 3.20, Ruby 2.5,
Perl 5.10, gperf 3.0.1 and Python (browser-engine/survey.md#wpe-webkit-in-detail). Ladybird requires clang 19 or GCC
14 and Rust 1.98 for every build; Swift was dropped (browser-engine/survey.md#ladybird). Servo and Gecko need Rust
`std` for the target; Chromium needs its own clang (browser-engine/survey.md#servo,
browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach). WPE's tarball holds 40,034 lines of Rust inside ANGLE
and Skia, which a build with WebGL off and Skia's default options does not compile (inference from the build options
read, browser-engine/survey.md#the-engines-compared).

Rust programs on Zoraya are native and `no_std`; there is no Rust `std` for the POSIX personality.

## Decision drivers

1. **One compiler for every port**, the one CI has (ADR-0360); a compiler change is a part of its own.
2. **Every language runtime is a decision** (CP-27): Rust `std` and Swift for POSIX programs widen what the POSIX layer
   must hold.
3. **Upgrade by a measured step** (CP-13): the engine's toolchain floor rises over time; the trigger must be written.
4. **Host tools** the engine's build needs exist in CI's image and on the Mac.

## Considered options

### Option A: Whatever the engine's upstream CI uses (the Linux distribution way)

- **Description:** Build with the newest clang and libc++ the engine is tested with (for Ladybird gcc-16 and clang-22).
- **Evidence for:** The engine's own configuration; fewest surprises.
- **Evidence against:** The rest of Zoraya's ports are on clang 18; two compilers in CI.
- **Cost:** *(estimate)* A second toolchain in the CI image.
- **Precedent:** Chromium's bundled clang; Linux distributions.

### Option B: Stay on clang 18 and libc++ 20 while the pinned WPE line supports them; raise both in one step when it stops

- **Description:** Pin WPE 2.54.x; when a WPE line Zoraya needs drops clang 18, CI moves to Ubuntu's clang 19 or newer
  and libc++ to the newest that supports it (ADR-0360's own revisit), in one part, before the engine moves.
- **Evidence for:** Drivers 1, 3 and 4; WPE 2.54 states clang 18 support.
- **Evidence against:** The FIXME means the next WPE line may require clang 19; the step then blocks an engine upgrade.
  The stated support is for clang 18 with libstdc++ (the workaround's own comment is about libstdc++'s `<expected>`,
  and it defines `__cpp_concepts` for every compile); clang 18 with libc++ 20 is not tested upstream on Linux
  (browser-engine/survey.md#wpe-webkit-in-detail), so the plan's E1 builds JavaScriptCore with LLVM 18 (CI's version,
  installed on the Mac beside Apple clang 21, which would not exercise the clang-18 workaround) and libc++ 20.1.8
  before anything else depends on it.
- **Cost:** *(estimate)* CMake, Ruby, gperf and unifdef in the CI image; nothing else now.
- **Precedent:** Genode, which pins its browser to a Qt release and moves engine and toolchain together
  (browser-engine/survey.md#how-other-systems-got-a-browser).

### Option C: Raise CI to clang 19 or newer now and add Rust `std` for POSIX programs, so Ladybird and Servo stay possible

- **Description:** Move CI to a newer clang and libc++ now; build a Rust `std` for an `aarch64-unknown-zoraya` POSIX
  target over `libc.a`.
- **Evidence for:** Keeps ADR-0410's alternatives open; Rust `std` would let Rust POSIX tools be ported.
- **Evidence against:** Driver 2: Rust `std` uses system calls through `libc`'s `syscall()` in places, which Zoraya's
  shim routes for eleven calls only (ADR-0141); a `std` port is weeks of work for an engine not chosen.
- **Cost:** *(estimate)* A compiler step (days) and a Rust `std` port (weeks).
- **Precedent:** Redox's Rust `std` over relibc (`[unverified]`: no Redox page was fetched).

### Option D: Swift for Ladybird's earlier plan

- **Description:** A Swift toolchain for POSIX programs.
- **Evidence for:** None now.
- **Evidence against:** Ladybird removed Swift in February 2026 (browser-engine/survey.md#ladybird).
- **Cost:** A whole runtime.
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **Compiler.** clang 18 in CI and the host's clang on the Mac, libc++ 20.1.8, `-std=c++23` (ADR-0364); the engine and
   its C++ dependencies through `ports/cmake/zoraya.cmake` (ADR-0367).
2. **Host tools** added to `.github/ci/Dockerfile` and the Mac's prerequisites: CMake 3.20 or newer, Ninja, Ruby 2.5 or
   newer, Perl, Python 3, gperf 3, unifdef, Meson (GLib, libsoup, fontconfig, HarfBuzz build with it).
3. **The trigger.** A WPE stable line that errors on clang 18 is not taken until a part moves CI to Ubuntu's newest
   packaged clang and libc++ to the newest that supports it, with every port's tests passing; that part supersedes
   ADR-0360's version.
4. **No Rust `std`, no Swift** for POSIX programs now; Rust `std` returns with Ladybird (ADR-0410's revisit) or a Rust
   POSIX tool worth porting, as its own ADR.
5. **Build cost.** The first engine part records the clean build's time, peak memory and disk on the M4 with
   `-j6` *(estimate: 1 to 2 hours, 10 to 20 GB)* and on CI; if CI cannot build it within a job's limit, the engine is
   built in a cached job of its own.

### Effect of the vision

For target 2, a smaller trusted toolchain: no second compiler and no second language runtime in the POSIX layer.
Target 3: the browser arrives without waiting for a Rust port. Networking: none. Threads: libc++'s threads as today.

### Examples

```
$ make -f ports/Makefile wpewebkit     # on the M4: configures with ports/cmake/zoraya.cmake, builds three programs
```

## Consequences

- **Positive.** No toolchain change before the first page renders.
- **Negative.** The engine's version can be held back by CI's compiler; a later forced step.
- **Follow-up.** The CI image's tools; the build-cost record.

## Invariants

- **INV-0418-1.** In an image built by CI, every port, the engine included, is compiled by the CI image's one clang
  and links the sysroot's libc++. *Check:* `zoraya-elf-check` gains a check, run in CI's build job, of the `.comment`
  section's compiler string of each program in the apps volume against the CI image's clang version.

## Open questions

- Whether libc++ 20 with clang 18 builds WPE 2.54 at all (the upstream combination is libstdc++), and whether the
  global `-D__cpp_concepts=202002` workaround is harmless under libc++; E1 answers both.
- CI's job time and disk for the engine.

## Revisit triggers

- The FIXME in WPE's compiler flags lands upstream in a line Zoraya needs.
- Ladybird or another Rust-heavy port is chosen.

## Architect's decision
