# ADR-0452: Rust POSIX programs are built on the host by `tools/zcargo`, which links through `zcc`

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0033, ADR-0109, ADR-0143, ADR-0360, ADR-0365, ADR-0367, ADR-0450, ADR-0451
- **Evidence:** CP-13, CP-27, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#redox, rust-std/survey.md#building-on-stable, rust-std/survey.md#crates-that-bypass-libc, rust-std/survey.md#how-other-systems-did-it, rust-std/survey.md#measurements, rust-std/survey.md#route-a-and-inv-0109-2, rust-std/survey.md#the-probes

## Context

The probes built a Rust `std` program for Zoraya with stable flags only (rust-std/survey.md#building-on-stable):
`--target aarch64-unknown-linux-musl`, `-C linker=tools/zcc`, `-C relocation-model=static` (the target defaults to
static-pie, which `zcc` refuses, ADR-0033), `-C link-self-contained=no` (so `-lc` and `-lunwind` find the sysroot's
`libc.a` and `libunwind.a`, not Rust's bundled copies) and `+crt-static`. `rustc` adds `-Wl,-z,relro,-z,now` after
`zcc`'s `-z norelro`; `zoraya-elf-check` accepts the result. An incremental release build took 2.2 s on the M1
(rust-std/survey.md#measurements). The probe's flags lived in a `.cargo/config.toml` and a script; a port or an outside
project needs one tool instead. C programs have `zcc` and C++ `zc++` (ADR-0109, ADR-0360); CMake projects
`ports/cmake/zoraya.cmake` (ADR-0367). A crate's own C code is compiled by the `cc` crate, which picks its compiler from
`CC_<target>` and may add a `--target` that `zcc` refuses (rust-std/survey.md#route-a-and-inv-0109-2; not tested).

## Decision drivers

1. **One way to build** each language, as `zcc` and `zc++` are (ADR-0109).
2. **Flags that cannot drift**: the five flags above decide whether a program loads and which unwinder it has.
3. **Checked output** (CP-27): every program links only the listed surface and contains no foreign `svc`
   (rust-std/survey.md#crates-that-bypass-libc).
4. **Built on the host**: no compiler of any language runs inside Zoraya for C++ (ADR-0365); Rust has the same reason.
5. **The toolchain pinned once** (CP-13): `rust-toolchain.toml` is the one place.

## Considered options

### Option A: A `.cargo/config.toml` per project with `zcc` as linker (the Linux cross-compiling way)

- **Description:** Document the flags; each project carries its own configuration, as cross-compiling to Linux musl is
  usually done.
- **Evidence for:** No tool to write; it is what the probe did (rust-std/survey.md#the-probes).
- **Evidence against:** Each port can get a flag wrong (driver 2); `RUSTFLAGS` in the environment silently replaces a
  configuration's flags; nothing checks the output.
- **Cost:** A page of documentation.
- **Precedent:** cross-compiling for Linux musl generally; Redox's target page asks the user to put relibc in the
  linker path (rust-std/survey.md#how-other-systems-did-it, alternatives-postmortem.md#redox).

### Option B: `tools/zcargo`, a wrapper around cargo that sets and guards everything

- **Description:** Items 1 to 7 below.
- **Evidence for:** Drivers 1 to 3; the same shape as `zcc` (a script that sets the target and refuses what would undo
  it).
- **Evidence against:** A wrapper to keep in step with cargo's environment variables.
- **Cost:** *(estimate)* 150 lines of shell, a host test, the checks.
- **Precedent:** `zcc`, `zc++` (ADR-0109, ADR-0360).

### Option C: A linker wrapper only (`aarch64-unknown-zoraya-rustc-ld`)

- **Description:** Projects set `linker` to a wrapper that also adds the archive and checks the output; the other flags
  are documented.
- **Evidence for:** Smaller; works under any build system that drives `rustc`.
- **Evidence against:** The relocation model and `link-self-contained` are `rustc` flags, not linker flags, so the
  wrapper cannot set them, only detect their effects.
- **Cost:** *(estimate)* 80 lines.
- **Precedent:** none here.

### Option D: A Zoraya SDK for outside projects (the Fuchsia way)

- **Description:** Ship the sysroot, `zcc`, the archive and a toolchain configuration as one bundle that an outside
  build system (GN, CMake, cargo) points at, as Fuchsia's SDK "provides the tools, libraries, and binaries required to
  build and link programs for Fuchsia" (rust-std/survey.md#how-other-systems-did-it).
- **Evidence for:** Outside projects need no checkout of this repository.
- **Evidence against:** Zoraya builds every port from this repository (ADR-0109); a bundle is a release artefact with
  its own versioning, which nothing here needs yet; the flags still need a tool that guards them.
- **Cost:** *(estimate)* A packaging step and its versioning rules.
- **Precedent:** Fuchsia's SDK (alternatives-postmortem.md#fuchsia--zircon-incl-starnix for the system).

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`tools/zcargo ARGS...`** runs the toolchain of this repository's `rust-toolchain.toml`, forced by
   `RUSTUP_TOOLCHAIN` set to its channel, whatever the project's own `rust-toolchain.toml` says (Ladybird pins 1.98.0)
   and whatever the caller's `RUSTUP_TOOLCHAIN` was, so that no build fetches another toolchain; its `cargo` runs with `CARGO_BUILD_TARGET=aarch64-unknown-linux-musl`,
   `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=tools/zcc` and, in `CARGO_ENCODED_RUSTFLAGS` (cargo's
   highest-precedence source, which with a `--target` applies to the target's crates and not to build scripts),
   `-C relocation-model=static -C link-self-contained=no -C target-feature=+crt-static -C link-arg=-lzoraya-rust`
   (ADR-0451's archive) followed by the caller's `RUSTFLAGS`, which `zcargo` then unsets, so that no caller's
   variable replaces its flags. A project's own `rustflags` in its `.cargo/config.toml` are replaced by these (cargo
   uses the highest-precedence source only); `zcargo` prints them with a note, and a port that needs one passes it in
   `RUSTFLAGS`, where item 2's refusals see it.
2. **It refuses** a `-Z` argument, a `+toolchain` argument, a set `RUSTC_BOOTSTRAP` (INV-0450-1), another `--target`,
   and in the caller's flags `link-self-contained=yes`, a `relocation-model` other than `static`, `-crt-static` and
   `panic=` other than `unwind` or `abort` (ADR-0454).
3. **C parts.** It sets `CC_aarch64_unknown_linux_musl=tools/zcc`, `CXX_...=tools/zc++` and `AR_...=tools/zoraya-ar`.
   If the `cc` crate passes `--target=aarch64-unknown-linux-musl`, `zcc` accepts exactly that one value when called
   with `ZORAYA_RUST=1` set by `zcargo` and drops it; every other `--target` is refused as today, and `__linux__` stays
   undefined (INV-0109-2).
4. **After a link**, `zcargo` runs `zoraya-elf-check` and its `--no-foreign-svc` check (INV-0450-2) on each executable
   it produced. That is also what catches link arguments a build script emits (`cargo:rustc-link-arg`), which item 2
   does not see: a static-pie, a foreign `libc.a` or a missing `PT_GNU_EH_FRAME` fails there.
5. **The archive.** `make -f ports/Makefile rust` builds `libzoraya-rust.a` into `build/sysroot/usr/lib`; `make ports`
   includes it.
6. **The toolchain.** `rust-toolchain.toml`'s `targets` lists `aarch64-unknown-linux-musl` beside
   `aarch64-unknown-none`; CI's image installs it; `make test` stays offline once it is installed (INV-0109-3).
7. **CMake and Corrosion.** `ports/cmake/zoraya.cmake` sets `Rust_CARGO_TARGET` and `CARGO` to `tools/zcargo`, so a
   CMake project with Rust parts (Ladybird) uses the same flags.
8. **The test step.** `make test-rust-std` builds `tests/rust-std/` (the probe of TASK-35 turned into a test that
   asserts each check, `docs/research/rust-std/probes/`) with `zcargo` and runs it through the broker under its own
   description, on the normal kernel, with a network step like `make test-net-posix`.

### Effect of the vision

For target 2, a Rust program has passed the same link gate and one more check (no foreign `svc`) before it reaches the
image. Target 3: an outside Rust project builds with one command. Networking and threads: none of its own.

### Examples

```
$ tools/zcargo build --release
$ tools/zcargo build -Zbuild-std
zcargo: refusing -Zbuild-std: Rust POSIX programs use the stable toolchain's prebuilt std (ADR-0450)
```

## Consequences

- **Positive.** One command, guarded flags, checked output; the same flags for cargo and CMake projects.
- **Negative.** A wrapper to follow cargo's variables; `zcc` gains a narrow `--target` exception for the `cc` crate.
- **Follow-up.** `tools/zcargo`, `ports/rust/`, `tests/rust-std/`, `make test-rust-std` and its line in
  `tools/test-map.txt`.

## Invariants

- **INV-0452-1.** Every Rust POSIX program is linked by `tools/zcargo` with `relocation-model=static`,
  `link-self-contained=no` and `+crt-static`, and loads: `zoraya-elf-check` accepts it. *Check:* a host test of
  `zcargo` that builds a small crate and reads `rustc`'s link line (`-v`), and refuses each forbidden flag of item 2;
  `make test-rust-std` runs `zoraya-elf-check` on its programs.
- **INV-0452-2.** `zcc` accepts `--target` only as `--target=aarch64-unknown-linux-musl` with `ZORAYA_RUST=1`, and never
  defines `__linux__`. *Check:* `ports/compare-predefs.sh` runs the predefinition comparison also in that mode; a host
  test that any other `--target` is refused.

## Open questions

- Whether the `cc` crate adds `--target` for `zcc` at all (it detects clang by its predefined macros); the part's first
  build of a crate with C code answers it, and item 3's exception is built only if needed.
- Whether a cargo workspace with build scripts (`build.rs`, run on the host) needs anything more than cargo's own
  host/target split; Ladybird's first part answers it.

## Revisit triggers

- Cargo gains a stable way to pin a project's target flags that `RUSTFLAGS` cannot replace.
- ADR-0450 moves to a Zoraya target: `zcargo` then sets that target instead.

## Architect's decision
