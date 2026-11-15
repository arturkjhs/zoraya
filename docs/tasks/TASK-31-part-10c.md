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

## Third agent (Sonnet): the library and test half (ADR-0389, proposed)

Merged `origin/main` into the branch first (poll and ppoll, for the readiness request). What was done, each with the command run in this session:

- **Readiness request** (`jobs.rs` `tty_input_ready`, `poll.rs`, `select` in `ffi/procs.rs`): one `PollReadable` per connection, idempotent across deferred waits; replaces the interim arm. `cargo test -p zoraya-posix` (tests/poll.rs: 7 terminal tests), `t_tty poll` in QEMU.
- **`tcsetattr`'s actions, `tcflush`, `tcdrain`, `IO` as `EIO`, `open("/dev/tty")`, `ttyname(_r)`, `ctermid`, `cf*`, `tcflow`, `tcsendbreak`, `tcgetwinsize`, `tcsetwinsize`, `TIOCSWINSZ` (`EPERM`), `TIOCSTI` (`ENOTTY`), `select`**: `tests/tty.rs`, `termios.rs`; `t_tty names` in QEMU. The shim's `select` and `tcsetattr` are gone; the deny list (23 names) and "a block naming a guarded name fails" are in `posix-surface-check`. `t_posix rawkeys` uses `tcsetattr`.
- **`sh` has job control in a window**: found on the way that Ctrl-Z went to the line (sh ignores it); `zosh` now sends `SIGTSTP` to the group `sh` chose with `tcsetpgrp`. `zosh.script`: Ctrl-C, Ctrl-Z, `jobs`, `kill`, `exit` in `sh` (PASS, 194 steps, HVF).
- **`grants` `keys` line, `TERM`, `console.term`, dash.cmd and sh.cmd, `term.scrollback`** (`Scrollback` ordinal 8, `term` applies it): `zoraya_shell::terminal`, host-tested; `TERM` and the `keys` line in QEMU.
- **GUI**: `tests/gui/posix-terminal.script`, `make test-tour-terminal` (a new step: map, lanes, ci-tests, plan-lib): `sh` with Ctrl-C and Ctrl-Z; `t_tty box` (alternate screen, raw mode, box on the border) follows a drag of the window's edge (90 then 80 columns, `winch 1`, `winch 2`), prints the bytes of F5, F1 and Ctrl+Right (they equal `kf5`, `kf1`, `kRIT5` of the pinned entry), takes a bracketed paste (mode 2004), stops with Ctrl-Z, and `fg` puts it back (`winch 3`, keys still raw). PASS at scale 1, scale 2, 1600x1000 and 1920x1200 under HVF. Rows are never counted (24 rows here, fewer on large displays: the program prints them).
- **Invariants**: INV-0331-1, -3, -6, INV-0333-2, -3, INV-0338-1, INV-0342-3, INV-0389-3 `checked`; INV-0331-4, INV-0332-5, INV-0338-2, INV-0342-2 `checked (partly: ...)` with what is missing; INV-0331-2, -5, -7 unchanged.

### Relay measurement (ADR-0331 item 14; HVF, one run, 1024x768, `t_tty bench`, result read from the window's screen)

| path | result |
|---|---|
| `t_tty bench bytes 4 \| wc -c` (a pipe between two programs, no shell relay) | 4 MiB in 12 ms (350 MB/s) |
| `t_tty bench bytes 1` to the window (relay by `zosh`, drawn by `term`) | 1 MiB in 457 ms (2.3 MB/s) |
| 100 repaints of 80x24 (2,079 bytes each) to the window | 57 ms, 0.57 ms a repaint (3.6 MB/s) |
| `write` of one byte then `tcdrain` (time until the shell has shown it), 200 times | mean 133 us, worst 351 us |

Not measured: a second and third run (the GUI step takes minutes per run; the numbers are one run, not a distribution), TCG, the share of CPU spent in `zosh` as against `term` (the guest has no per-process CPU read for a native program here: `CLOCK_PROCESS_CPUTIME_ID` of the writer equals its wall time), and `ttyd` per window. Reading: a full repaint costs 0.6 ms and a drained byte 0.13 ms, which a person cannot see; a 1 MiB `cat` takes 0.46 s, of which the window's drawing is a share that this run cannot separate from the shell's copy. **Recommendation: keep the shell (ADR-0331 Option B).** Option C would move the same copy and the same drawing behind one more process and one more hop; the numbers give no sign that the copy through `zosh` is more than half of either case, but they do not prove it either: the ADR's own condition ("more than half") needs the split, which is a follow-up (a counter of the time `pump_relay` spends, read by `statistics`). Recorded for the architect.

### Same discipline features in `ttyd`

None added: `ttyd` is `zoraya-tty`'s host for the session, so `VMIN`/`VTIME`, `Flush`, `Drain`, `PollReadable` and the line tag are the same code. It ignores `Restore`, `Reinstate`, `Scrollback`, as ADR-0331 item 1 says. `t_tty` and the sh job-control script run under `zosh` only; the session's `ttyd` was not exercised with the new calls (no script runs `t_tty` there).

### Mutation run

See the end of this file.

## Remains (in this order)

1. GUI: the kill of a raw-mode program followed by a typed command (INV-0331-4's wording), the paste to a program without mode 2004 (held, question in the title, Enter sends), and INV-0331-7 in a window are not read; the last line of `t_tty box` starts where the shell's cursor was after the alternate screen (a leading run of blanks): the cursor position `Restore` leaves is worth a look.
2. INV-0331-2's password case (a background program tries to turn echo on while another reads a password) and INV-0331-5 in QEMU; `select`/`pselect` are not host-tested (FFI file).
3. The relay measurement's split of CPU between `zosh` and `term`, more runs, and TCG; `ttyd` runs of `t_tty`.
4. The Ctrl-C defect on a continued job (`docs/backlog.md`), not looked into.
5. Not done in `zosh`: a native first element that reads the keys keeps them (ADR-0387 item 2); no kill offer for a line with the terminal; the serial console's carriage return (ADR-0331 item 13).
6. A `tcsetpgrp` to the line's own group and a later `Ctrl-Z` while `sh` is itself foreground still says `did not stop: sh` (sh ignores `SIGTSTP`), which is what ADR-0200 prescribes.
7. `make test-changed` was not run (it selects every step); `make check`, `cargo test` of the changed crates, `make test-posix` (zosh.script only) and `make test-tour-terminal` (four sizes) were.

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
