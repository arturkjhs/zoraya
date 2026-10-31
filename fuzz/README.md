# Fuzzing zoraya-fs

Coverage-guided fuzz targets for the filesystem decoder (ADR-0012 rule 4,
INV-0012-3, INV-0012-4). They need a nightly toolchain and
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), so this directory is
outside the workspace and `make test` does not run it. The workspace runs a
deterministic fuzz test on the pinned toolchain instead
(`crates/zoraya-fs/tests/decoder_fuzz.rs`).

| Target | Input |
|---|---|
| `mount_walk` | byte edits applied to a valid seed image (two-level tree, directories, files, a link object, an orphan), optionally with every checksum up to the root record recomputed |
| `raw_image` | a mask XORed onto the first bytes of the seed image (root slots and early blocks), no checksums recomputed |

Each input is mounted read-only and read-write, walked through the request
API (every directory, name, file and link target) and checked with `fsck`.
A panic, an out-of-bounds access, an overflow (debug assertions are on) or
a timeout is a finding.

## Running

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run mount_walk -- -max_total_time=600 -timeout=10 -len_control=0 -max_len=256
cargo +nightly fuzz run raw_image -- -max_total_time=600 -timeout=10 -len_control=0 -max_len=65536
```

Findings land in `fuzz/artifacts/<target>/`. Each one found so far is fixed
and kept as a regression test in `crates/zoraya-fs/tests/regressions.rs`.
The corpus (`fuzz/corpus/`) is not committed.

## Runs so far (TASK-07, 2026-10-31)

Four cores, the two targets in parallel, `cargo-fuzz` 0.13.2, `rustc 1.101.0-nightly`:

| Round | Target | Time | Executions | Coverage (libFuzzer `cov` / `ft`) | Findings |
|---|---|---|---|---|---|
| 1 (default length control) | `mount_walk` | 601 s | 18,846 | 3,622 / 7,948 | none |
| 1 (default length control, raw prefix input) | `raw_image` | 601 s | 17,584 | 2,049 / 2,089 | none |
| 2 (`-len_control=0`) | `mount_walk` | 601 s | 18,464 | 3,646 / 8,688 | none |
| 2 (`-len_control=0`, XOR input) | `raw_image` | 601 s | 16,189 | 2,076 / 2,186 | none |

`cov` counts instrumented edges reached, over the whole binary (engine, test kit and libFuzzer's Rust glue); no
per-line coverage report was made. Before these runs the workspace's deterministic fuzz test found an overflow in root
selection, and reviewing that code found an ancestor walk bounded only by an on-disk value; both are fixed and in
`regressions.rs`. A first attempt
at the seed image failed with `NoSpace` in the harness itself (the seed did not fit 128 blocks), fixed by a 256-block
seed; that was not an engine finding.
