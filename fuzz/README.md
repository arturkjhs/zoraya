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
| `editor_keys` | arbitrary key bytes to the line editor at any width (ADR-0154, INV-0154-1); the deterministic test is `crates/zoraya-shell/src/editor_tests.rs` |
| `shell_parse` | arbitrary UTF-8 text as a shell line or script (ADR-0150, INV-0150-3); the deterministic test is `crates/zoraya-shell/src/lang_tests.rs` |
| `describe` | arbitrary bytes as a command description (ADR-0156, INV-0156-1); the deterministic test is `crates/zoraya-describe/src/tests.rs` |
| `net_rx` | arbitrary bytes as an Ethernet frame for the ARP parser and as a script for a hostile virtio-net device and a hostile service (ADR-0221, INV-0221-2); the deterministic tests are `crates/zoraya-virtio-net/src/tests.rs` and `crates/zoraya-proto-nic/src/lib.rs`. Not run by the agent that wrote it (no nightly or cargo-fuzz on the host); its body was compiled and driven with 3000 random inputs in a scratch crate |
| `spawn_request` | `spawn`'s request (as it is, and a valid one with bytes changed) and the startup block (ADR-0298, Q7) |
| `keeper_messages` | messages from the three kinds of channel to the room keeper, on a model of the kernel's jobs (ADR-0269, INV-0269-2) |
| `broker_tty` | requests to the POSIX broker on a model of the kernel, and to the terminal server, with keys, output and resizes (ADR-0104, ADR-0105) |
| `compositor_protocol` | a client's messages (windows, sheets, buffers, drops) and input frames to the compositor on a fake platform (ADR-0093, ADR-0094) |
| `file_protocol` | requests to `fsd` over the fake transport, with channels, tokens and memory objects as handles; ends in a walk of the service's counts (ADR-0055) |

Each input is mounted read-only and read-write, walked through the request
API (every directory, name, file and link target) and checked with `fsck`.
A panic, an out-of-bounds access, an overflow (debug assertions are on) or
a timeout is a finding.

The targets of Q7 (ADR-0298) take their bytes as messages: `wire.rs` splits the input into `sel, flags, ordinal,
length, bytes` and wraps the bytes in a valid header unless a flag says they are the whole message. `make test-fuzz` and
`make test-fuzz-net` build them with the nightly toolchain and LLVM's coverage instrumentation (`tools/run-fuzz.sh`, no
cargo-fuzz) and run each for `FUZZ_SECONDS` (20 locally, 600 in CI), output in `target/test-logs/fuzz-TARGET.log`.

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
