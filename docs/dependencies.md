# External dependencies

Every external crate used by the kernel or by a `no_std` crate is listed here with its exact version, license,
purpose, and the reason it is not written in-house. Only permissively licensed (MIT, Apache-2.0, BSD, ISC, Zlib)
`no_std` crates are allowed. Adding one is a decision (see `CLAUDE.md`): record it here in the same change.

| Crate | Version | License | Used by | Purpose | Why not in-house |
|---|---|---|---|---|---|
| *(none)* | | | | | |

The kernel uses `core` and `alloc`. The crates under `crates/` use only `core` (and `alloc`: `zoraya-fs`), and their
tests use `std`, with seeded generators instead of a property-testing crate. `zoraya-fs-testkit`, `zoraya-mkfs` and
`zoraya-fsck` (TASK-07) use only `std` and the workspace's own crates. The host-side tools (`zoraya-test-runner`, `zoraya-adr-check`) use
only `std`. `Cargo.lock` has no registry entries, so no build needs network access once the toolchain in
`rust-toolchain.toml` is installed.

## Fuzzing (outside the workspace)

`fuzz/` is a separate cargo project (its own `[workspace]`, excluded from the main one) for coverage-guided fuzzing
of `zoraya-fs` with cargo-fuzz (TASK-07). It needs a nightly toolchain and network access to fetch its crates, so
`make test` does not build it; the workspace runs a deterministic fuzz test on the pinned toolchain instead. Its
versions are pinned in `fuzz/Cargo.lock`.

| Crate | Version | License | Used by | Purpose | Why not in-house |
|---|---|---|---|---|---|
| `libfuzzer-sys` | 0.4.10 | (MIT OR Apache-2.0) AND NCSA (the bundled LLVM libFuzzer sources) | `fuzz/` only (host) | links LLVM's libFuzzer, the coverage-guided fuzzing engine cargo-fuzz drives | a coverage-guided fuzzer is compiler instrumentation plus an engine; not a small thing |

`libfuzzer-sys` brings in `arbitrary` 1.5.0, and to build libFuzzer `cc` 1.10.0, `jobserver` 0.1.35, `libc`
0.2.191, `shlex` 2.0.1, `cfg-if` 1.0.5, `getrandom` 0.4.3 and `find-msvc-tools` 0.1.15, each MIT OR Apache-2.0 per
its `Cargo.toml`; `r-efi` 6.0.0 is in the lock file for UEFI targets only and is not built on Linux or macOS.

## Toolchain and host tools

These are not crates, but the build depends on them:

| Tool | Version used | Where it comes from |
|---|---|---|
| Rust | 1.99.1 (stable), pinned in `rust-toolchain.toml` with `rustfmt`, `clippy` and the `aarch64-unknown-none` target | rustup |
| QEMU | 11.1.2 (`qemu-system-aarch64`) | Homebrew on macOS; distribution package on Linux |
| GNU make, POSIX `sh` | the host's | macOS Command Line Tools / distribution |
| Rust nightly and `cargo-fuzz` (only for `fuzz/`) | nightly 2026-10-29 (`rustc 1.101.0-nightly`), cargo-fuzz 0.13.2 | rustup, `cargo install cargo-fuzz` |

`dtc` (Device Tree Compiler) is useful for inspecting the fixtures but is not needed by any build or test.
