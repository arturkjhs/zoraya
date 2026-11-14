# TASK-31 part 10c: a POSIX program has a terminal from `zosh` (report at a step boundary)

A second agent (this report's items 7 to 11) wired the library halves into `zosh` and the broker; see the
Remains list for what is still open.

Branch `work/10c`, from `work/10b` at `b3989d2`. Written by an agent (Sonnet) that stopped at a clean boundary: the
library halves are done and host-tested; the wiring that makes `sh` work in a window is not. Another agent continues from
the "Remains" list. ADR numbers reserved: 0386 to 0389; 0386 to 0388 are written (proposed), 0389 is free.

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

7. **`zosh` hosts the discipline** (commit `3fc4c80`, REMAINS 1 of the first report). Each POSIX program whose fd 0,
   1 or 2 is the terminal (the keys, or the terminal's output, neither redirected nor piped; a background line keeps
   its empty fd 0) gets a tty description: a connection made by `Host::connect`, served on the shell's port
   (`TTY_KEY`), and the write end of a stream to the terminal; `/dev/tty` is in its table
   (`posix_child::Launch::tty`). Keys of a line with the terminal go through the discipline, `Size` calls
   `Host::size`, such a line's output is relayed by `Host::relay`, a paste goes to `Host::paste`; the effects are
   carried out in `Z::carry` (replies, echo, `job_signal`, the shell's own stop for `Suspend`, `Restore`,
   `Reinstate`, the title). A line with the terminal ends with `Restore`, a native line before the next prompt (so `cat x.rec; sleep 9` keeps its screen for the `sleep`, as `tests/gui/terminal.script` reads it); `zoraya_shell::reset` and its
   notes in `docs/design.md` and `docs/backlog.md` are gone. The time is set before each call and `deadline()` is in
   the wait. Before taking a request the shell relays what the line wrote (so `Drain` is answered when the output is
   shown). A line's connections are closed at its end. `zoraya-tty` gains `set_foreground_line` (the whole line
   reads until `tcsetpgrp` chooses). Proposed **ADR-0387** with INV-0387-1 and -2.
8. **The broker's notices** (commit `597eb34`, REMAINS 2). `HostTerminal`, `GroupMade`, `ForegroundAsked` in
   `zoraya-proto-posix` (16 to 18); the broker gives `zosh` the channel on its client connection; no reply either
   way. A group made inside a client's line is made inside the line's job; join and `tcsetpgrp` across lines are
   `EPERM`. `zosh` takes `ForegroundAsked` only for the line in front (`Host::foreground_asked`). Proposed
   **ADR-0388** with INV-0388-1.
9. **The 10b hidden-characters finding** (REMAINS 9, its second half): the paste to a program goes through the same
   `paste::Feed` and `zoraya_proto_term::paste::clean` as the prompt's; `tty_host`'s
   `a_paste_to_a_program_is_cleaned_as_at_the_prompt` checks U+200B, U+202E, U+2066 and U+FEFF are gone.
10. **In-QEMU checks** in `tests/posix/zosh.script` (serial console, `make test-posix`): `isatty` of each fd with and
   without a pipe (`t_posix rawkeys`), Ctrl-C reaches the line in front only, a background program's read of the
   terminal is refused and it stops (`t_posix readerr`), raw mode survives Ctrl-Z and `fg`, and `sh` typed at the
   prompt gives `$ `, runs a command and exits. `sh` has no job control yet (it needs `open("/dev/tty")`).
   Run in this session: `make test-posix` with `zosh`, `zosh-userland`, `tour` and `scenarios` (PASS, HVF), `make
   test-gui GUI_RUNS="terminal:auto:1 terminal:auto:2 terminal:auto:1600x1000 terminal:auto:1920x1200
   selection:auto:1"` (PASS), `cargo test -p zoraya-tty -p zoraya-shell -p zoraya-broker`, `make check`. Not run:
   the rest of `make test-changed`'s steps (boot, files, tour-gui, net, keeper, hand-run, apps, netsurf, posix-gui)
   and every TCG run.
11. A defect found on the way: a job-end packet of a job whose number a new job took was taken for the new job's end
   (a running background job was once reported `done`); `take_ended` now takes only a job whose `TERMINATED` is set.

Files of 10a or 10b touched: `crates/zoraya-shell/src/crlf.rs` (a test message reworded so the lint passes),
`crates/zoraya-shell/src/lib.rs` (one `pub mod`), `crates/zoraya-shell/Cargo.toml` and `Cargo.lock`/`fuzz/Cargo.lock`
(two path dependencies), `docs/adr/invariants.md` (Status of eight rows), `docs/design.md`.

## Remains (in this order)

1. **GUI checks of the wiring** (the old REMAINS 5's GUI half): GUI scripts at 1920x1200, 1600x1000, scales 1 and 2
   (fewer than 24 rows) for `sh` in a window with Ctrl-C and Ctrl-Z, a resize reaching a raw-mode program
   (`SIGWINCH`, INV-0331-6), the screen's modes after a stop and `fg` (INV-0331-4), a paste to a program with and
   without 2004 (INV-0332-5's program half). Not written: no GUI run was made in this session.
2. **`libposix`** (the old REMAINS 3, unchanged): the readiness request, `Flush`, the three `tcsetattr` actions, `IO`
   as `EIO`, `open("/dev/tty")`, `ttyname`, the terminal names with a committed C test program, the shim's two names,
   the deny list, `TIOCSTI`, INV-0338-2's table. Until `open("/dev/tty")` works dash has no job control, so a POSIX
   shell's `tcsetpgrp` through ADR-0388 is host-tested only. `t_posix rawkeys` uses `ioctl(TCGETS/TCSETS)` because
   `tcsetattr` is not a name of `libc.a`; change it to `tcsetattr` then.
3. **`grants`' `keys` line, `TERM`, `dash.cmd`** (the old REMAINS 4, unchanged), and `console.term`.
4. **`term.scrollback`**: `term` cannot read the settings; it needs a message from `zosh` (a new `zoraya-proto-term`
   ordinal) or a setting given at launch. Not done; no ADR written for it.
5. **The relay measurement** (the old REMAINS 6, unchanged; nothing measured).
6. **The same features in `ttyd`** (the old REMAINS 7, unchanged).
7. **Mutation run** of `tty_host.rs` and of the broker's new code (`make mutants-host CRATE=zoraya-shell` and
   `CRATE=zoraya-broker`, `--in-diff`); not run.
8. **The Ctrl-C defect on a continued job** (`docs/backlog.md`, found in the POSIX session with `ttyd`): not
   reproduced or looked into in this session. What was read: the broker resumes a suspended group job as a whole
   (`signal_pids`, review R1) and the kernel's `Job::signal` walks the whole tree, so a later child of the group
   should be reached; the next step is the slow-host reproduction of `scenarios.script` step V6 under TCG with a
   print of the job handle `ttyd` signals.
9. Not done in `zosh`: a line whose first element is native and reads the keys keeps them (ADR-0387 item 2); no kill
   offer for a line with the terminal (item 4); the serial console's discipline echo goes through the kernel's
   carriage return (ADR-0331 item 13).

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
