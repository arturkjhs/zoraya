# TASK-31, part work/test-fixes-2: report

Branch `work/test-fixes-2`, from `origin/main` 46287f1. Machine: Mac mini, `CARGO_BUILD_JOBS=3`, one QEMU at a time.

## What changed

1. **T1, the `libc.a` relink** (`ports/Makefile`). `libc.a` depended on `libzoraya_posix.a`, which cargo writes anew in every
   job, so a `libc.a` restored from the ports cache was always older and every port relinked. It now depends on a stamp,
   `build/ports/obj/libzoraya_posix.sum` (inside the cached `build/ports/obj`), rewritten only when the `cksum` of the
   library changes. Hand-run (musl tarball copied into `build/ports/src`): `make -f ports/Makefile libc` (first build);
   the same again (cargo reruns, `libc.a` keeps its time); `touch` the library and the same again (no `libc.a: hid` line,
   `libc.a` unchanged); writing a different value into the stamp makes the next run relink. Not run: CI's build job. The
   estimate (build about 5 to 6 min on a cache hit) is the audit's, not measured.
2. **`make test-cxx-libcxx` under TCG.** `tests/cxx/libcxx-tcg-excluded.txt` now takes a name ending in `/` as a directory
   (`tests/cxx/libcxx-build.sh`) and lists `std/thread/thread.semaphore/`, `thread.barrier/` and `thread.latch/`. Decision:
   latch is included because it waits through the same atomic wait; `std/atomics` (the atomic wait's own tests) is not in
   `libcxx-groups.txt`. HVF runs everything. `tools/test-map.txt`: `test-cxx-libcxx` no longer follows `ports/` and
   `tools/zcc`; it follows `ports/cxx`, a new `ports/musl/` line, `crates/zoraya-posix/`, `tests/cxx/`, and (kept, they are
   libc++'s own) `ports/patches/llvm/`, `tools/zc++`, ADR-0366's line and the Makefile/runner lines. `ci-plan.sh` and
   `ci-tests.txt` needed no change (the plan reads the map). Run: `make check` (includes `check-test-lanes`, `check-ci-plan`),
   the awk matching by hand on five paths. Not run: `libcxx-build.sh` itself (no LLVM tree here) and the job.
3. **`t_quota` handshake** (`user/src/bin/t_quota.rs`): a channel at slot 9 replaces `nap(150)` and `nap(400)`: the child
   says `ready`, the parent narrows the limit, sends `go`; both poll with a 10 s bound. Run: `make test-qemu-one` rc 0, 119
   passed, `quotas_from_a_process` ok.
4. **`adr-check`: the `checked:` rule (V40)** (`tools/adr-check/src/main.rs`): a `checked:` status must name at least one
   command, and every `make TARGET` (Makefile, or `ports/Makefile` after `-f`), `cargo test -p PACKAGE` and `tools/` or
   `.github/` path in it must exist. All other rows passed. INV-0042-1 ("no command fails if it is broken") is now
   `not yet applicable (until a command checks it ...)`. Test `checked_names_a_command_that_exists`; with the "names no
   command" case disabled it fails, restored it passes (13 tests).
5. **INV-0230-1 check (C3):** `tools/check-net-open.sh`, `make check-net-open` (a step of `make check`; wired in the
   Makefile, `test-par.sh`, the CI checks matrix and the map): the printed netdev has `restrict=on` for a test and not with
   `--net-open` or in a person's run; every Makefile definition passing `--net-open` is listed in
   `tools/net-open-allowed.txt`; `tools/test-httpd` binds `127.0.0.1` only. Shown by hand: removing `restrict=on` fails it,
   a new `--net-open` recipe fails it. The Linux network namespace of the invariant's text is not done (said in the row).

## Finding, not fixed (INV-0172-4)

I added to `tests/shell/rooms.script`, inside the entered room, `room new refused-room +w /t` and `+w /etc`, expecting
`outside what this shell holds`. `make test-files` showed both rooms were created: the console's shell keeps the root at
slot 5 (`Held::Root`, `user/src/bin/zosh.rs:4900`), so entering a room does not narrow what it can give. The refusal
(`zosh.rs:3660`) only bites a shell that holds a table (a terminal's shell). Whether the console's shell is meant to be
exempt is the architect's question; the script change is reverted and INV-0172-4's row is unchanged. A test needs a
terminal shell (GUI QEMU) or a decision.

## What was run

`make check` after each commit (rc 0); `cargo test -p zoraya-adr-check` (13 passed); `make test-qemu-one` (pass);
`make test-files` (failed only on the added INV-0172-4 lines, reverted; the other scripts before `rooms` passed, the ones
after it did not run). Not run: `make test-changed`, CI.

## What remains

The 55 stale `not yet applicable` rows (C1; not touched, to keep clear of the other lane's rows), a check for INV-0172-4
(above), the remaining vacuous-test findings, V4, libc++'s atomic wait over the futex (ADR-0301), 10e's `config.sub sun4`.
