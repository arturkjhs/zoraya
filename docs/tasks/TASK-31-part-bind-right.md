# TASK-31 part `work/bind-right`: the BIND right (ADR-0466 proposed)

Closes finding 3 of `docs/tasks/TASK-31-review-own-sockets.md`: the place rule of `bind` was enforced only by `libposix`.

## What changed

- `crates/zoraya-proto-file`: `rights::BIND = 1 << 9`, outside `rights::ALL`; `rights::WITH_BIND`; the `Rights` type's mask is `WITH_BIND`.
- `crates/zoraya-fs-service`: a service root holds `WITH_BIND`; a create of an `S_IFSOCK` node (named or unnamed) needs `BIND` on the connection; a node, or a directory that holds one, does not move by `rename`, `exchange` or `publish` into a directory without `BIND`.
- Minting: `zosh` asks for `BIND` for the own directory and the line's `/tmp` only; `init`, the panel (`gui.rs`), the broker, `namespace::record` (shell records only) and `posixrun` (test `/tmp`) pass it on; `libposix` `clone_tmp` keeps it for a line's children; a child's own directory is `READ | CLONE`.
- ADR-0466 (proposed), INV-0466-1 to -3, `docs/design.md`, `docs/architect-queue.md`; ADR-0465's open question notes the part.
- Tests: `crates/zoraya-fs-service/tests/socket.rs` (7 new), `crates/zoraya-proto-file/tests/bind_sites.rs` (list of places that name the bit), `user/src/bin/t_storage.rs` `socket_nodes_need_bind` (raw requests, no `libposix`, QEMU: refused in a `+w` directory, made in the own directory, refused through a clone, a narrowing and a `READ | CLONE` child). `tools/test-map.txt`: `user/src/namespace.rs` mapped (it had no line, so a change ran every step). The raw test is in `t_storage`, because a new user program changes `USER_PROGRAMS` and reaches every step.

## Review

`docs/tasks/TASK-31-review-bind-right.md`: two errors found and fixed (a node moved out of the place by `rename` and `publish`; the `files` application's record got `BIND`), warnings fixed, a second look at the fixes.

## Known limits

`BIND` is on a connection, not a path (the whole own tree and `/tmp`). A moved directory is searched for nodes to a depth of 16 and 4096 entries; past that it is held back. The `into_source` half of an exchange has no test. Per-room counts of nodes are F11's.

## Runs

See the end of this file.

- `make check`: exit 0 on the head.
- `make test-changed` (HVF, serial lanes of `test-par`), `build/test-changed-3.log`, at `39308583`: RC=2. Passed: host tests (46 binaries), test-boot, test-libposix, test-posix-threads, test-files, test-crash, test-keeper, test-hand-run, test-posix (with `zosh-sockets.script`), test-posix-gui, test-tour-gui, test-tour-terminal, test-gui, test-posix-suites, test-posix-threadtest3, test-apps, test-netsurf, test-cxx, test-cxx-libcxx, test-cxx-gtest, test-python-serve. Failed: test-qemu and test-net, both on the new `t_storage` case (an empty path gives `INVALID_ARGUMENT`, not `ACCESS_DENIED`; the test's error). The four python steps were skipped by the harness (`PAR_LONG=1`).
- After the fix (`284e451d`): `make test-qemu-one` (`build/test-qemu-one.log`), RC=0, "135 passed, 10 expected failures", `t_storage: socket nodes need BIND: ok`. test-net was not rerun (the same kernel test); no other step was run after the fix.
