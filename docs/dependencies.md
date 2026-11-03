# External dependencies

Every external crate used by the kernel or by a `no_std` crate is listed here with its exact version, license,
purpose, and the reason it is not written in-house. Only permissively licensed (MIT, Apache-2.0, BSD, ISC, Zlib)
`no_std` crates are allowed. Adding one is a decision (see `CLAUDE.md`): record it here in the same change.

| Crate | Version | License | Used by | Purpose | Why not in-house |
|---|---|---|---|---|---|
| *(none)* | | | | | |

The kernel uses `core` and `alloc`, and user programs `core`. The crates under `crates/` use only `core` (and `alloc`:
`zoraya-fs`), and their tests use `std`, with seeded generators instead of a property-testing crate. `zoraya-fs-testkit`,
`zoraya-mkfs` and `zoraya-fsck` (TASK-07) use only `std` and the workspace's own crates. The GUI's crates (TASK-14: `zoraya-virtio-gpu`, `zoraya-virtio-input`, the four `zoraya-proto-*` of
the GUI, `zoraya-keymap`, `zoraya-font`, `zoraya-term`, `zoraya-compositor`) use only `core` and `alloc` and each
other. The font in `zoraya-font` was drawn for Zoraya (ADR-0095) and is under the repository's BSD-2-Clause licence: no
font file and no rasteriser crate is embedded. The host-side tools
(`zoraya-test-runner`, `zoraya-adr-check`, `zoraya-mkramdisk`, `zoraya-mutants`) use
only `std`, and the test runner also the workspace's `zoraya-font`, `zoraya-term` and `zoraya-compositor`, to read the
screen as the terminal draws it. `Cargo.lock` has no registry entries, so no build needs network access once the toolchain in
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
| `cargo-mutants` (only for `make mutants-host`) | 27.1.0, MIT, <https://github.com/sourcefrog/cargo-mutants> | `cargo install cargo-mutants --version 27.1.0 --locked` |
| Rust nightly and `cargo-fuzz` (only for `fuzz/`) | nightly 2026-10-29 (`rustc 1.101.0-nightly`), cargo-fuzz 0.13.2 | rustup, `cargo install cargo-fuzz` |

`dtc` (Device Tree Compiler) is useful for inspecting the fixtures but is not needed by any build or test.

### The cross toolchain (stage 6b, ADR-0109 item 10)

Needed by `make fetch-ports` and `make ports` only; `make test` needs none of them except the POSIX tools it already
uses (its `check-fetch-ports` step runs `curl` on a local `file://` URL and `shasum` or `sha256sum`). `make ports`
prints the clang it uses.

| Tool | Version used | Where it comes from |
|---|---|---|
| clang, any version that targets `aarch64-unknown-none-elf` and has `--ld-path` (`tools/zcc`) | Apple clang 21.0.0 (clang-2100.0.123.102) on the M4; on Linux CI the runner's `clang` (unverified here) | Xcode Command Line Tools / distribution |
| `rust-lld` (through the toolchain's `gcc-ld/ld.lld`) | LLD 23.1.1, from Rust 1.99.1 | rustup, with the pinned toolchain; nothing to add |
| `core` and `compiler_builtins` for `aarch64-unknown-none` (in `libc.a`'s stub, ADR-0140) | from Rust 1.99.1 | the pinned toolchain's target; not a crate from a registry |
| the host's C compiler (`cc`): dash's helpers, tcc's `c2str` and the host tcc of the link check | Apple clang 21.0.0 on the M4 | Xcode Command Line Tools / distribution |
| `curl`, `shasum` or `sha256sum` (`make fetch-ports`) | curl 8.7.1 | macOS / distribution |
| `patch`, `tar` with gzip and bzip2, `awk`, `sed` | the host's | macOS / distribution |
| GNU sed, as `gsed` or `sed` (toybox's build scripts) | GNU sed 4.10 (Homebrew `gnu-sed`) on the M4 | `brew install gnu-sed` on macOS; the default `sed` on Linux |

`rust-objcopy`, which ADR-0109 item 10 lists, is not used: it needs the `llvm-tools` component, which the minimal
profile of `rust-toolchain.toml` does not install, so `posix-surface-check` renames symbols itself (ADR-0141).

## Ported programs (third-party C, stage 6b)

Not crates: C sources built by `ports/Makefile` into `build/sysroot` and `build/rootfs-posix`, fetched by
`make fetch-ports` and never committed. Versions and hashes are in `ports/sources.lock`, licences and patches in
`ports/README.md`. Why they are not written in-house: ADR-0100 (musl's pure C) and ADR-0005 (the programs are the
measured T1 workloads).

| Program | Version | Licence | Used for |
|---|---|---|---|
| musl | 1.2.5 | MIT (a few files public domain or under other permissive terms, its `COPYRIGHT`) | the retained C of `libc.a` and the sysroot's headers (ADR-0100) |
| dash | 0.5.13.5 | BSD-3-Clause | `/bin/sh` |
| GNU make | 4.4.1 | GPL-3.0-or-later | `/bin/make` |
| toybox | 0.8.15 | 0BSD | `cat head ls mv rm sleep touch true uname sort wc yes` |
| busybox | 1.38.0 | GPL-2.0-only | `tr`, `diff` |
| TinyCC | `e23dabed154a8495f3ac1ff3cc9d04fa1c207893` | LGPL (`COPYING` 2.1, the LGPL files version 2 or later; some files permissive) | `/bin/tcc`, `libtcc1.a` |

### Mutation testing

`make mutants` runs `tools/mutants` (`zoraya-mutants`, `std` only): hand-written mutations, each with the tests that
should catch it, including the in-QEMU ones. `make mutants-host CRATE=name` runs `cargo-mutants` on one host crate.
It is a separate program on the host, not a dependency: nothing in the workspace links it, `Cargo.lock` does not list
it, and `make test` does not run it. It makes its mutants from the code's syntax (operators, return values, deleted
statements), which a hand-written list cannot do at that scale: 168 mutants of `zoraya-virtio` in 53 s on an M4,
25 missed, when evaluated on 2026-11-02. It only runs `cargo test`, so it cannot reach the kernel, the user programs or
QEMU; those stay with `tools/mutants`. Many of its misses are equivalent mutants, for example a layout constant the
fake device reads from the same expression, so its `missed.txt` goes to the `mutant-triage` subagent, not straight
into new tests. Installing it needs network access once.

