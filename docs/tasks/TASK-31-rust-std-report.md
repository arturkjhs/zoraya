# TASK-31 part `work/rust-std`: Rust `std` for POSIX programs (ADR-0450 to ADR-0454), report

Branch `work/rust-std`, from `origin/main` `4b7eabfd`. Implements ADR-0450, ADR-0451, ADR-0452, ADR-0453 (with ADR-0227 item 3) and ADR-0454 as
accepted on 2026-11-15; ADR-0455 (the engine choice) needs nothing of this part but its gate (a), whose status is below. One new proposed ADR:
**ADR-0468**. No ADR status was set or changed; no section "Architect's decision" was edited. The independent review is
`docs/tasks/TASK-31-review-rust-std.md`.

## What was built

| Piece | Where | ADR |
|---|---|---|
| `syscall()` is `long` and answers `futex` (98: `WAIT`, `WAKE`, `WAIT_BITSET` with the full mask and an absolute `CLOCK_MONOTONIC` deadline, each with or without the private flag), `getrandom` (278) and `gettid` (178); `statx`, `copy_file_range`, `splice`, `pidfd_open`, `pidfd_send_signal` are `ENOSYS` without a report; every other number `ENOSYS` and one `SyscallReport` per number on the `REPORT` channel | `crates/zoraya-posix/src/rust_std.rs` (the table, the futex forms, the deadline, once per number; host tests), `ffi.rs` (`syscall`, `futex_wait_until`), `crates/zoraya-abi/src/posix.rs` (`SyscallReport`), `user/src/bin/zosh.rs` and `posixrun.rs` (shown) | 0451 item 1 |
| `ioctl(FIONBIO)` as `fcntl(F_SETFL)` keeping the other flags | `process.rs` `set_nonblocking`, `ffi/procs.rs` | 0451 item 2 |
| Ten names in `libc.a`: `bcmp`, `getauxval`, `pause`, `pipe2` (hidden until now), `sched_yield`, `clock_nanosleep`, `dirfd`, `gettid`, `posix_spawn_file_actions_addchdir` and `_np` (a `Chdir` file action resolved in the parent's table, relative paths following the one before, a child with a chosen table gets the path among its own entries); ADR-0451's block joins the tier list; the non-`_np` prototype is in musl patch 0005; `libpyshim.a` lost `sched_yield`, `pause`, `dirfd` | `ffi.rs`, `ffi/procs.rs`, `jobs.rs`, `spawn.rs`, `ports/musl/zoraya/rust_std.c`, `ports/Makefile` (`ADDED_ADRS`), `.github/workflows/test.yml` (the two ports-cache lists), `ports/apps/pyshim.c` | 0451 item 3, 0320 note |
| `libzoraya-rust.a`: `setuid`, `setgid`, `setgroups`, `setsid`, `chroot`, `sigaltstack`, `sched_getaffinity`, `sendfile`, `splice`, `waitid`, each `ENOSYS` (`socketpair`, `sendmsg`, `recvmsg`, `posix_spawnp` were exported already) | `ports/rust/zoraya-rust.c`, `ports/rust/names.txt`, `make -f ports/Makefile rust` | 0451 item 5 |
| libposix's `__rustc` runtime names are hidden like any name outside the tier list; `check` fails on one | `tools/posix-surface-check` (`is_rustc_runtime`) | 0451 item 6 |
| `/proc/self/cgroup`, `mountinfo`, `maps`: `ENOENT`, no hint | `process.rs` `is_probe` | 0451 item 7 |
| `tools/zcargo` (guarded flags, the toolchain forced, refusals), `tools/zcargo-link` (the linker: `zcc` with a map, the link line kept, then `zoraya-elf-check --no-foreign-svc`), `tools/rust-musl-target.sh` (the toolchain's own `rust-std` component into `build/` by the channel manifest's URL and sha256, for a host that may not `rustup target add`), `zcc` under `ZORAYA_RUST=1`, `rust-toolchain.toml` lists the target | `tools/`, `ports/compare-predefs.sh` | 0450, 0452 |
| `zoraya-elf-check --no-foreign-svc MAP FILE`: an `svc` in an executable segment that the link map does not attribute to a member of `libc.a` | `tools/zoraya-elf-check` | 0450 item INV-0450-2 |
| Every POSIX child has a random connection: the broker beside a native shell holds one of a shell's kind (slot 10, from `init` or the panel), answers the new message `RandomConnection`, and libposix asks once per `posix_spawn` and `exec` without `GRANT_RANDOM` | `crates/zoraya-proto-posix`, `crates/zoraya-broker`, `user/src/bin/broker.rs`, `user/src/beside.rs`, `init.rs`, `panel.rs`, `jobs.rs` (`broker_random`), `spawn.rs` (`grant_connections`) | 0453 item 3, 0227 item 3 |
| `getpeername` finishes a non-blocking connect `netd` has answered (Rust's `connect_timeout` trusts `POLLOUT` and then asks `peer_addr`) | `net.rs` | a bug found by the test, not an ADR item |
| Checks and tests: `make check-rust-stable`, `check-rust-archive`, `check-zcargo` (in `make check` and CI's checks); `make test-rust-std` (`tests/rust-std/`: `rstest`, and `rsnet` under its own description, `rust-std.script`; `tools/test-zcargo.sh build`); `t_syscall` modes `futex`, `random`, `tid`, `fionbio`, `names`, `chdir`, `forkexec`, `report`, and a `sendmsg` width check in `libposix.script`; host tests in `zoraya-posix`, `zoraya-abi`, `zoraya-broker`, `zoraya-elf-check`, `posix-surface-check` | see the files | INV-0450-1 to INV-0454-2 |
| Registered: `Makefile` (`TEST_STEPS`, `CHECK_STEPS`), `tools/ci-tests.txt`, `tools/test-lanes.txt`, `tools/test-map.txt`, `tools/plan-lib.sh`, `tools/test-par.sh`, the CI checks matrix | | repo conventions |
| Docs: `docs/design.md` ("Rust `std` for POSIX programs"), `docs/dependencies.md` (the target of the toolchain row; no crate added), `docs/adr/invariants.md` (the rows and the Counts), rule 6 notes in ADR-0288, 0310, 0320, 0411, 0418, 0450 to 0454 | | |

## Proposed ADR

**ADR-0468** (proposed): Rust 1.99.1's `Command::spawn` makes a `SOCK_SEQPACKET` socketpair for its exec-error channel before it forks
(`library/std/src/sys/process/unix/unix.rs`); libposix's `fork` refuses a process that holds a packet socket (a channel end cannot be duplicated,
ADR-0190, ADR-0412 item 6), so every `Command` that takes the fork path (a bare program name with `PATH` set, `uid`, `gid`, `pre_exec`) fails with
`ErrorKind::Unsupported`. ADR-0453 and INV-0453-1 assumed it worked; TASK-35's probes ran `posix_spawn` only. The recommendation is the least
committing one: keep the refusal (Option A), record and test it, and revisit when channel ends are duplicable (Option C). INV-0468-1 is the row.
INV-0453-1's fork-path clause is checked by a C `fork` and `exec` (`t_syscall forkexec`) instead.

## Results (every command was run in this session; the runner's logs are in `target/test-logs/`)

| Command | Result |
|---|---|
| `make check` (before every commit) | 30 of 30 steps pass (fmt, clippy, adr-check 0 errors, the new `check-rust-stable`, `check-rust-archive`, `check-zcargo`, `check-libposix-widths` against the built headers and archive, `check-ci-plan`, `check-test-lanes`, ...) |
| `cargo test -p zoraya-posix` | all suites pass (new: `rust_std` unit tests, `tests/rust_std.rs`, five chdir and two net tests) |
| `cargo test -p zoraya-broker`, `-p zoraya-proto-posix`, `-p zoraya-abi`, `-p zoraya-elf-check`, `-p posix-surface-check` | pass |
| `make test-rust-std` (HVF) | RC=0, `PASS: 62 steps`, fsck clean (`rstest hello`, `syscall`, `time`, `hash`, `threads` (condvar ping-pong 1000 rounds in 8 ms; `wait_timeout` timed out after 203 ms), `fs /tmp`, `fs` in a `+w` directory, `denied`, `process`, `panic`, `uncaught` (exit status 101), `abort` (signal 6), `overflow` (data abort), `rsnet` by address and by name (`connect_timeout`, `set_nonblocking`, another port `EACCES`)); `tools/test-zcargo.sh build` ok |
| `make test-libposix` (HVF) | RC=0: `PASS: 199 steps`, 17 (maps), 66 (top), 43 (room image); `t_syscall` `syscall`, `stubs` (7 checks), `wide`, `futex` (27), `tid` (6), `fionbio` (11), `names` (20), `random` (6), `chdir` (20), `forkexec` (3), `report` (9) and the shell's note; `t_spawn` `table`, `table2`, `default`, `refuse`, `grants`, `unix`, `memfd`, `addopen`, `inherited`, `queued`, `spawnp` pass with the child's `getrandom` now 0 |
| `ports/compare-predefs.sh` with the built host `tcc` | "zcc and tcc predefine the same identity: 15 macros, no Unix or Linux", with the `ZORAYA_RUST=1` mode and the refusals |
| `make test-changed` (HVF, `PAR_QEMU=1`), at `0d4e64e5` | `make check` 30 of 30; host tests 71 test binaries pass (111 s); the QEMU steps were stopped by me before they finished (they build curl, wget, git, NetSurf and more from source on this seeded tree); the steps that cover this part were run on their own: `test-rust-std`, `test-libposix` (above, rerun after the review fixes: `PASS: 62`, `PASS: 199, 17, 66, 43`). Not run here: test-apps, test-netsurf, test-gui, test-posix*, test-cxx*, test-net, test-hand-run, test-keeper, test-boot, test-files |
| after the review fixes | `make check` 30 of 30; `make test-rust-std` RC=0 (62 steps); `make test-libposix` RC=0 |

## What is not done, and limits

- **The fork path of `Command`** (above, ADR-0468). `CommandExt::groups` and `chroot` are unstable in Rust 1.99.1, so INV-0453-2 is checked for `uid` and
  `gid` (the names the unstable ones reach fail in the archive too).
- A **session broker** (`init`'s `start_session`, whose first process is a POSIX shell) is given no random connection; a `fork` child before `exec`
  has none. ADR-0455 item 0 (a link-only build of Ladybird's 433 crates) was not attempted; ADR-0452 item 7 (CMake and Corrosion) is not built; no crate
  with C code was built, so whether the `cc` crate adds `--target` is unmeasured (the `zcc` exception is in and checked by the predefinition comparison).
- No TCG run (`make test-rust-std` runs `rust-std-tcg` only under `FULL=1` or on CI's TCG host); no Rust toolchain step beyond 1.99.1.
- A `Command` child that hashes is measured through `posix_spawn` only; the 64-connection limit of the broker and the service's 256 were not exercised.
- `getauxval(AT_HWCAP)` is 0; the main thread's `gettid` is 1; `std` backtraces have no names without symbols; `metadata().created()` is an error;
  `canonicalize` of a path in a granted directory whose parent is not granted is `ENOENT`; the main stack is 64 KiB.
- The checks `check-rust-archive` and `check-zcargo` run in `make check` offline; `tools/test-zcargo.sh build` needs the sysroot and the target's std
  (here the component fetched by `tools/rust-musl-target.sh` into `build/rust-musl-sysroot`; CI's image gets the target from `rust-toolchain.toml`,
  whose `targets` now lists it, which changes the image's tag).
- This machine's `build/ports` was seeded with `cp -Rp` of `sysroot`, `ports` and `cxx-libcxx` from the first worktree's `build/` (the
  libc++ build would otherwise have taken the first hour); nothing was written there.
