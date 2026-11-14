# TASK-31, part "poll": `poll` and `ppoll` in libposix (backlog, "Three requests from outside work", item 2)

Branch `work/poll`, from `origin/main` e4891a3. ADR-0398 (proposed) is the number the task reserved. Plan part F1 of
`docs/research/browser-engine/plan.md` (`poll`, `ppoll`, `select`, `eventfd`) is split: this part builds `poll` and `ppoll` only.

## What exists

- `crates/zoraya-posix/src/poll.rs`: `Posix::poll_once(&mut [PollFd], deadline)` (the readiness look, over the same `wait_for` as
  `pselect`, so a threaded process defers the wait, ADR-0315), `PollFd` and the `POLL*` constants, and three pure functions for the
  boundary: `array_len` (`nfds` above `OPEN_MAX` is `EINVAL`, NULL with entries `EFAULT`), `deadline_after_ms`,
  `deadline_after_timespec` (saturating, `EINVAL` for a bad field).
- `crates/zoraya-posix/src/ffi/procs.rs`: `poll` (exported through `export_name`, because the file has a signal helper called
  `poll`), `ppoll`, and `poll_wait`, which has `pselect`'s loop (mask swap with `SIG_SETMASK`, handler ran is `EINTR`, an interrupt
  with none is another look, mask restored on every exit, `testcancel` first).
- Host tests: `crates/zoraya-posix/tests/poll.rs` (18 tests), two in `tests/net.rs` (sockets). The fake kernel
  (`tests/kernel/mod.rs`) gained `signals_of`, a `signals_unknown` switch, and a `wait_any` that checks every source and ends a wait with
  a deadline as `DeadlineExceeded` when nothing can wake it (before: first source only).
- C test `tests/posix-core/t_poll.c` (cases `pipe select files socket nval timeout hostile mask` in `libposix.script`, `threads` in
  `threads.script`), built with `-Wall -Wextra -Werror` in `make test-libposix` and `make test-posix-threads`. No new step, so
  `tools/ci-tests.txt`, `tools/test-lanes.txt` and `tools/test-map.txt` already cover it (`tests/posix-core/` is mapped to both steps; a
  line for `docs/adr/0398-*` was added).
- ADR-0398 (proposed): the choices ADR-0342 and ADR-0414 leave open, the `posix-surface-added` block (`poll t_poll`, `ppoll t_poll`),
  invariants INV-0398-1 to -3 (rows in `invariants.md`; a row in `index.md`). `ports/Makefile` (`ADDED_ADRS`) and `.github/workflows/test.yml`
  (the two ports-cache lists) name the ADR. `docs/design.md` says what exists.

## What was run (all in the worktree, `PATH` with `~/.cargo/bin`, `CARGO_BUILD_JOBS=3`)

| Command | Result |
|---|---|
| `cargo test -p zoraya-posix` | all suites ok (poll: 18 passed; net: 16 passed; the others unchanged) |
| `make check` (twice, last after the final test changes) | 23 of 23 passed (fmt, clippy, adr-check 0 errors, the `check-*`) |
| `make test-libposix` | exit 0, `PASS: 85 steps` under HVF; `t_poll: pipe select files socket nval timeout hostile mask passed`; libc.a export check passed with `poll`, `ppoll` admitted from ADR-0398 |
| `make test-posix-threads` | exit 0, `PASS: 100 steps`; `t_poll: threads passed` |
| `make test-posix` | exit 0 (hello, w5, w8, w9, w10, invariants, scenarios, threads, zosh, tour: all PASS), the regression of the changed `ffi/procs.rs` |
| `make mutants-host CRATE=zoraya-posix MUTANTS_HOST_ARGS="--in-diff <diff of poll.rs>"` | first run 95 mutants: 65 caught, 10 missed, 3 timeouts (caught: loops), 17 unviable; after four more tests: 69 caught, 6 missed (all `\|` to `^` on constants whose bits are disjoint, equivalent), 3 timeouts, 17 unviable |

`make test-changed` was **not** run: the change touches `crates/zoraya-posix` and `Makefile`/`ports/Makefile`, which the map sends to
every step (`DRY=1 make test-changed` lists 23: apps, gui, net, netsurf, cxx, python-skipped...), hours of QEMU and ports builds that this session
(one QEMU at a time, 16 GB) does not run; the three steps that run the changed code (`test-libposix`, `test-posix-threads`, `test-posix`) were run
instead. CI's candidate run (not started by this part) covers the rest.

## Decisions (where the ADRs are silent; all in ADR-0398)

- `POLLIN` with `POLLHUP` at end of file (a read does not block, INV-0414-1; `select` says readable); `POLLERR` with the requested write bits
  on a write end whose reader closed; only the requested bits are reported, plus `POLLHUP`/`POLLERR`/`POLLNVAL` always.
- Ready at once, as `select` has it: files, directories, `/dev/null`, a socket with no connection, a terminal (until part 10c), and a
  direction a descriptor has no end for (`POLLOUT` on a read end).
- `nfds` limit is `OPEN_MAX` (512), checked before reading the array, no truncation of a 64-bit count; `ppoll` does not write the timeout back.
- `select` is not exported by this part: ADR-0342 item 3 retires `zshim.c`'s `select` with F1.

## Limits

- Terminals are ready at once (so a program polling a terminal for input spins); part 10c replaces that branch of `Posix::probes`.
- No socket pair, loopback, listener, Unix socket, memory descriptor or `eventfd` exists to poll (F1, F2, ADRs 0412 to 0414); connecting
  and connected TCP sockets are tested on the host against the fake `netd`, not from C. INV-0414-1 and INV-0342-2 stay "not yet applicable"
  in `invariants.md` (they name kinds and the `select` name this part does not build); INV-0398-1 to -3 are what is checked.
- A non-NULL array that is not mapped faults the process (as `pselect`'s sets do); `poll` is linear in the descriptors per call.
- `pselect` still has its own look (`select_once` in `ffi/procs.rs`); `select` and `poll` become one code in F1. The C test compares
  `pselect` and `poll` on pipes, `/dev/null` and a socket (`select` case).
- ADR-0414 item 4's cost measurement (a `poll` over 64 descriptors in a threaded process) was not made.
- For the browser-engine design (F1 and D4): GLib's `g_poll` and Ladybird's `ppoll` loop link against these two names; `eventfd` is still
  needed for GLib's wake-up, and `ppoll`'s mask semantics here are `pselect`'s.
- Stage 10 part 10c ("the readiness request" for terminals) builds the terminal side of `probes`; what this part leaves for it is the
  `Desc::Stream { tty: true }` arm (today ready at once, host test `a_terminal_is_ready_at_once_until_it_has_a_readiness_request` to be
  replaced) and, in the library, a source for the terminal's readiness in `wait_for`.
