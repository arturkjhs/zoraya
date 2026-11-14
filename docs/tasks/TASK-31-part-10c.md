# TASK-31 part 10c: a POSIX program has a terminal from `zosh` (report at a step boundary)

Branch `work/10c`, from `work/10b` at `b3989d2`. Written by an agent (Sonnet) that stopped at a clean boundary: the
library halves are done and host-tested; the wiring that makes `sh` work in a window is not. Another agent continues from
the "Remains" list. ADR numbers reserved: 0386 to 0389; 0386 is written (proposed), 0387 to 0389 are free.

## Done (each step committed and pushed)

1. **`zoraya-tty`** (commit `d4d8247`, then `7488919`). `VMIN` and `VTIME` in POSIX's four cases with a host clock
   (`set_time`, `deadline`, `tick`; `Discipline::read_ready`, `read_deadline`, `read_raw`); the poll rule of Linux
   (`readable`); `Flush` (ordinal 7) and `Drain` (8) in `zoraya-proto-tty` and the status `IO` (7); a line tag on every
   connection (`add_terminal_in`; a clone carries its parent's line whatever group it declares); `set_front_line`
   (lines are told apart from the first call; with none in front nobody reads; a read held by a line that left is
   answered `NotForeground`); `Read` from another line is `NotForeground`, `PollReadable`, `SetAttr` and `Flush` are
   `IO`; `GetAttr` and the size are answered for any connection; `set_foreground`, `clear_foreground_group`,
   `end_line` (defaults back, unread input dropped, the old attributes returned), `continue_line`, `set_output_pending`
   (releases held `Drain`s); `Termios::raw()`, `Discipline::onlcr()`, `set_attr_request()` so that hosts and their
   tests name no flag. 40 tests in `cargo test -p zoraya-tty` (21 are new: the four cases with a fake clock, the poll
   rule, flush, drain, two lines, clones, held reads, end and return of a line). The fuzz target `broker_tty` drives the
   new calls (`cargo check --offline` in `fuzz/` builds; it was not run under the nightly toolchain this session).
2. **The shell's rules** (`crates/zoraya-shell/src/tty_host.rs`, 14 tests; `cargo test -p zoraya-shell tty_host`):
   `Host` with lines, `bring_front`, `stop_front`, `end_line`, `size` (one `SIGWINCH`), `tcsetpgrp` (only for a group
   the broker says was created in the front line), `keys` (nothing typed with no line in front reaches the
   discipline; a change of the front line drops unread input), `relay` (the carriage return from the attributes), and
   the paste to a program (`Bracket` reads 2004 from the output; a newline without 2004 is held, the question goes in
   the window title, Enter sends, any other key drops; no `ESC` and no signal character can be in the text).
3. **`zoraya-proto-term`**: `Restore { keep }` (6) and `Reinstate` (7); `term` calls `restore` and `reinstate`
   (`user/src/bin/term.rs`, `kept: Option<Restored>`). `ttyd` ignores them. Nothing sends them yet.
4. **ADR-0386** (proposed) with its index and invariant rows (INV-0386-1, -2; counts 758). `make adr-check`: 0 errors.
5. **INV-0331-8** is a lint: `make check-kernel-tty` greps `ttyd.rs`, `zosh.rs` and `crates/zoraya-shell/src` for flag
   names outside comments. The Status of INV-0331-2, -3, -4, -5, -6 says "partly: host tests" with what is missing.
6. `docs/design.md` (terminal paragraph) says what exists and what not.

Files of 10a or 10b touched: `crates/zoraya-shell/src/crlf.rs` (a test message reworded so the lint passes),
`crates/zoraya-shell/src/lib.rs` (one `pub mod`), `crates/zoraya-shell/Cargo.toml` and `Cargo.lock`/`fuzz/Cargo.lock`
(two path dependencies), `docs/adr/invariants.md` (Status of eight rows), `docs/design.md`.

## Remains (in this order)

1. **`zosh` calls `Host`** (`user/src/bin/zosh.rs`, about 5,300 lines; the places are `read_keys`, `wait_foreground`,
   `Relay`/`Feed`, `Ready::Posix`): a `Host<Handle>` per window; each POSIX program with a terminal on fd 0, 1 or 2
   gets a tty description (a `Host::connect(line, group)` server end watched on the shell's port, the write end of the
   line's output stream, ADR-0105 items 4 and 6); `Effect`s are carried out (`Tty(Reply)` as channel writes, `Show` as
   output to `term`, `Signal` as `job_signal`, `Suspend` through the broker's suspend for POSIX elements, `Restore` and
   `Reinstate` as `zoraya-proto-term` messages, `Question` as `Title`); `Size` calls `Host::size`; the output of a POSIX
   program with a terminal goes through `Host::relay` instead of the fixed `onlcr`; `Paste` while a program is in
   front goes to `Host::paste`. **Remove `zoraya_shell::reset` and its call (line about 5195) when `Restore` is sent**,
   and the "Stop-gap" notes in `docs/design.md` and `docs/backlog.md`. `term.scrollback` and `console.term` are read here.
   `time`: call `Host::tty.set_time(clock_monotonic() ms)` before each call and use `deadline()` in `port_wait`.
2. **The broker's control channel to `zosh`** (`user/src/beside.rs`, `crates/zoraya-broker`): a POSIX shell's
   `tcsetpgrp` is passed to `zosh` as `Host::tcsetpgrp`; the broker also tells `zosh` each group it creates in a line
   (`Host::group_created`). Today the broker beside `zosh` answers "no terminal".
3. **`libposix`**: the readiness request (`PollReadable`, idempotent: the look is re-run after each deferred wait;
   replaces the "ready at once" arm in `pselect`; `work/poll`'s `poll`/`ppoll` use it when merged), `Flush` for
   `tcflush` and the three `tcsetattr` actions (Drain, then Flush for `TCSAFLUSH`, then `SetAttr`), the status `IO` as
   `EIO`, `open("/dev/tty")` (clone the program's own connection, `ENXIO` with no terminal), `ttyname`, the terminal
   names of ADR-0342 items 4 and 5 with a committed C test program, retire the shim's two names, the deny list
   (INV-0342-2, -3), `ioctl(TIOCSTI)` is `ENOTTY` (INV-0338-1), a table test for INV-0338-2.
4. **`grants`' `keys` line** (`crates/zoraya-shell/src/grants.rs`, INV-0331-1 with a host test over every combination of
   redirection and pipe), `TERM` for programs with a terminal (ADR-0333 items 1, 4, 5), `dash.cmd` no longer says it
   has no terminal.
5. **The in-QEMU and GUI checks**: a C program printing `isatty` of each fd and the open of `/dev/tty`; the 200 ms
   `select` program; a raw-mode demo that draws a box following `SIGWINCH` and prints the keys it reads (F-keys,
   Ctrl+arrows); the GUI scripts at 1920x1200, 1600x1000, scales 1 and 2 (fewer than 24 rows); `sh` with Ctrl-C and
   Ctrl-Z; a background program that cannot read what is typed. These make INV-0331-1 to -6 fully checked and
   INV-0332-5, INV-0333-2, -3, INV-0338-1, -2, INV-0342-2, -3 checked; then change the Status cells from "partly".
6. **The relay measurement** (ADR-0331 item 14: `cat` of 1 MiB and 100 full repaints, under HVF and TCG, with the
   share spent in `zosh`) and the recommendation between the shell and a `ttyd` per window, for the architect. No
   number has been measured: none is claimed.
7. **The same features in the session's `ttyd`** (`user/src/bin/ttyd.rs`): `set_time`/`tick`, `Flush`, `Drain`
   (`set_output_pending`), `add_terminal` stays line 0 because `ttyd` never calls `set_front_line`.
8. **Mutation run** of the second half; see below.
9. The "Ctrl-C defect on a continued job" (10c row of `docs/backlog.md`) and the 10b review's finding on hidden
   characters were not looked at.

## Decisions taken where the ADRs are silent (ADR-0386)

Restore and Reinstate as two messages; mode 2004 read from the program's output; the held paste's question in the title
until ADR-0337's status line exists; newlines of a paste sent as carriage returns; a paste past the discipline's 16 KiB
pending bound is cut; with no line in front nobody reads; a change of the front line drops unread input.

## Mutation run (`make mutants-host CRATE=zoraya-tty`, `--in-diff` the 10c diff of the crate)

`zoraya-tty`'s changed code (`lib.rs`, `discipline.rs`): 168 mutants, first run (82 s): 103 caught, 31 missed, 34
unviable. Holes killed by new tests: `Terminal::front_line`, `set_foreground`, `clear_foreground_group`,
`set_attr_request`, `Termios::raw` (exact flags), `read_ready`'s canonical arm, `read_deadline`'s canonical guard
(`raw_is_cfmakeraw_and_nothing_else`, `set_attr_request_is_the_message_a_client_sends`,
`the_host_calls_move_the_front_line_and_the_foreground`, `read_ready_and_deadline_in_canonical_mode_follow_lines_only`).
Second run (83 s): 126 caught, 8 missed, 34 unviable. **The 8 are equivalent:** seven are `|` read as `^` between the
disjoint bits of the masks in `Termios::raw`; one is a `|` read as `&` between `INLCR` and `IGNCR` (it changes the
precedence to `ICRNL | (INLCR & IGNCR) | IXON`, and the default `iflag` has neither bit, so the result is the same).
The `zoraya-shell` `tty_host.rs` (new, 14 tests) has not had a mutation run: do it with the rest of step 1 of "Remains"
(`make mutants-host CRATE=zoraya-shell`, `--in-diff`).
