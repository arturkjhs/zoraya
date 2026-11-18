# ADR-0389: The terminal calls of `libposix` where ADR-0331 and ADR-0342 are silent

- **Status:** accepted
- **Date:** 2026-11-15
- **Depends on:** ADR-0105, ADR-0302, ADR-0331, ADR-0333, ADR-0338, ADR-0342, ADR-0386, ADR-0387, ADR-0388, ADR-0398
- **Evidence:** CP-27, stage-10/survey.md#the-name-devtty, stage-10/survey.md#what-ncurses-needs-from-libc, stage-10/survey.md#the-tty-service-as-built

## Context

ADR-0331 items 2 and 5 and ADR-0342 items 3 to 5 say what `libposix` gives a program with a terminal: the readiness
request, `/dev/tty`, `ttyname`, `tcsetattr`'s three actions, `tcflush`, and the termios names. They leave open the
details a program can see: what an idempotent readiness request is when a wait is deferred (ADR-0302 item 9), which
errno each status of the service becomes, where `/dev/tty`'s output end comes from when the program has closed its
own, and which names the part exports. This ADR records the choices made while building them. It changes no accepted
decision.

**What is known.**

- `poll` and `ppoll` exist (ADR-0398) and call a terminal ready at once, an interim that ADR-0398 says part 10c
  replaces. A threaded process runs the same look again after each deferred wait, so a look that sent a request each
  time would send many (ADR-0398's note on 10c).
- `ttyd`'s `PollReadable` is held by the service until a read would not block (and, in a window, until the
  connection's line is in front), at most one at a time per connection: a second is `Busy`
  (`crates/zoraya-tty/src/lib.rs`).
- The namespace table's `Terminal` entry holds a connection (ADR-0331 item 2, `posix_child.rs`); the program's tty
  descriptions each hold a connection and the write end of the output stream.
- `less`, git and ssh open `/dev/tty` by name, each with a fallback (`stage-10/survey.md#the-name-devtty`); curses
  waits with `poll` or `select` and sets modes with `tcsetattr` (`stage-10/survey.md#what-ncurses-needs-from-libc`);
  the discipline's calls are those of `stage-10/survey.md#the-tty-service-as-built`; a name is exported when a named
  program imports it (CP-27).
- musl's `src/termios` and `src/unistd` are not retained (`ports/musl/musl.mk`): every name below comes from
  `libposix`, hidden until a block names it (ADR-0141).

## Decision drivers

1. A program that polls a terminal must wait, and must not spin or starve a socket it polls beside it (ADR-0398).
2. The same look, run again, must change nothing (ADR-0302 item 9).
3. A refusal means one thing to the program: `EIO` for "your line is not in front", never `EINVAL`.
4. No name is exported without a committed program that imports it (ADR-0342 item 1).
5. Nothing that ADR-0342 item 12 guards comes in through a block.

## Considered options

### Option A: one outstanding readiness request per connection, kept in the library

- **Description:**
  1. **The request.** `poll`, `ppoll` and `select` look at a terminal's input with `Posix::tty_input_ready`: bytes the
     library kept from an earlier answer are ready; a held `Read` is ready when its answer is on the channel;
     otherwise a `PollReadable` is sent once and remembered (`tty_polls`, by connection and transaction). Each look
     reads the channel without blocking; the answer for the remembered transaction is "ready" (an error status too: a
     read then says why), the channel closing is "ready with `POLLHUP`" (a read gives end of file), nothing is "not
     yet" and the connection's channel, `READABLE | PEER_CLOSED`, joins the wait's sources. A look that finds its
     request outstanding sends nothing, so the look run again after a deferred wait, or by the next `poll`, is the
     same look. An answer that another request's wait or a `read` takes off the channel is noted (`tty_poll_ready`)
     for the next look, and a read spends it.
  2. **A terminal's output** is polled as a pipe's write end (the stream's signals), in `poll` and `select` alike.
  3. **`tcsetattr`.** `TCSANOW` sends `SetAttr`; `TCSADRAIN` sends `Drain` first (answered when everything written
     has been shown); `TCSAFLUSH` sends `Drain`, then `Flush` of the input (the library's kept bytes go too), then
     `SetAttr`. Another `when` is `EINVAL` and sends nothing. `ioctl(TCSETS, TCSETSW, TCSETSF)` are the three.
  4. **Statuses.** `IO` is `EIO` for every request that has one; `NotAllowed` is `EPERM`; any other refusal is `EINVAL`.
  5. **`tcflush`.** `TCIFLUSH` and `TCIOFLUSH` send `Flush`; `TCOFLUSH` sends nothing (output is a stream the library
     already wrote into) and only checks the descriptor. `tcdrain` sends `Drain`. `tcflow` and `tcsendbreak` check the
     descriptor (and `tcflow`'s action) and do nothing.
  6. **`/dev/tty`.** `open` clones the namespace entry's connection (`Clone`, tagged with the caller's own group) and
     takes the output end from a terminal description the process still holds, duplicated; with none it opens for
     reading and its writes fail with `EBADF`. `ttyname`, `ttyname_r` and `ctermid` answer `/dev/tty`
     (`ttyname_r`: `ERANGE` below 9 bytes). `ioctl(TIOCSWINSZ)` and `tcsetwinsize` on a terminal are `EPERM`;
     `ioctl(TIOCSTI)` is `ENOTTY` on any descriptor, a terminal included (never offered).
  7. **The speeds** are `c_cflag`'s baud bits, stored and without effect; `cfmakeraw` is musl's.
  8. **`select`** is exported, as `pselect` with a `struct timeval` and no mask; the timeout is not written back.
     `ports/apps/zshim.c`'s `select` and `tcsetattr` are gone.
  9. **The guarded names** of ADR-0342 item 12 (the pseudo-terminal functions and `setsid`, beside `ptrace`,
     `dlopen` and System V IPC) are on `posix-surface-check`'s exclusion table; a block that names one is an error
     (`added_names`), and so is a `libc.a` that exports one.
  10. **The suspend character with a job-control shell.** When a POSIX shell has chosen a group with `tcsetpgrp`
      that is not one of the line's own, Ctrl-Z is `SIGTSTP` to that group through the broker and the line stays in
      front (`sh` says `Stopped` itself); the line is stopped as ADR-0200 says only when the group is one of the
      line's own (the shell back at its prompt, which ignores it: `did not stop`).
  11. **`TERM`, `keys` and `console.term`.** `zoraya_shell::terminal` holds the rules: `terminal_fds` (the fds of an
      element that are the terminal), `posix_env` (`PATH`, `HOME`, then `TERM` for a program with the terminal on any
      fd: `xterm-256color` in a window, the setting `console.term` on the serial console, default `dumb`; never
      `TERMINFO`, `TERMINFO_DIRS`, `LINES`, `COLUMNS` or `COLORTERM`), and the `keys` row of `grants`. A program
      whose three streams are elsewhere has no `TERM` from the shell and no `keys` row. `sh.cmd` and `dash.cmd` no
      longer say there is no terminal.
  12. **`term.scrollback`.** Declared by `zosh` (number, 100 to 10,000, default 2,000); the shell tells the window's
      `term` with a new message `Scrollback { lines }` (ordinal 8 of `zoraya-proto-term`) at start and after each
      change of the settings file. `ttyd` ignores it.
  13. **The table's terminal.** A namespace record (`Bootstrap::decode`) names a terminal only as `/dev/tty`, and
      nothing else has that name (INV-0338-2).
  14. **The names** this part adds, behind `t_tty` (`tests/posix/t_tty.c`):

```posix-surface-added
tcsetattr t_tty
tcflush t_tty
tcdrain t_tty
tcflow t_tty
tcsendbreak t_tty
cfmakeraw t_tty
cfgetispeed t_tty
cfgetospeed t_tty
cfsetispeed t_tty
cfsetospeed t_tty
ctermid t_tty
ttyname_r t_tty
tcgetwinsize t_tty
tcsetwinsize t_tty
select t_tty
```

- **Evidence for:** Drivers 1 to 5. The service already holds one request a connection, so the library keeps the
  same one; the deferred protocol needs no new state in the process.
- **Evidence against:** A program that polls a terminal and never reads leaves a request in the service until the
  connection closes; a second `poll` finds it outstanding and uses it (no leak, one slot). Two threads polling one
  terminal share one request. A `poll` while a `read` of another thread is held waits on the read's answer, so it
  may report ready for bytes that the other thread then takes (as on Linux).
- **Cost:** *(measured)* `jobs.rs` 200 lines, `poll.rs` 25, `termios.rs` 200, `terminal.rs` 190, the FFI 200; 27 host tests.
- **Precedent:** Linux's `n_tty_poll` is the same rule in the kernel; musl implements `tcsetattr`'s actions as three
  `ioctl` codes.

### Option B: send a `PollReadable` for every look and cancel it afterwards

- **Description:** Each look sends and each wait end cancels.
- **Evidence for:** No library state.
- **Evidence against:** The service has no cancel, so the second look of a deferred wait is `Busy` (driver 2).
- **Cost:** *(estimate)* a new message in `zoraya-proto-tty` and both hosts.
- **Precedent:** none surveyed.

### Option C: poll a terminal by reading it (a non-blocking read that keeps its bytes)

- **Description:** A look reads with `max` of 1 and keeps the byte in `tty_rest`.
- **Evidence for:** No new message, as ADR-0105 first sketched.
- **Evidence against:** In canonical mode a read answers a whole line, not "a byte is there", and the service's
  `NotForeground` would stop a background program that only polled (POSIX does not).
- **Cost:** *(estimate)* small.
- **Precedent:** none.

## Recommendation

**Recommendation (not a decision): Option A.**

**Tensions with accepted ADRs.** ADR-0105 item 6's "the terminal is stage 6d's" is carried out. ADR-0398's interim
arm for terminals is replaced as it says; INV-0414-1 and INV-0342-2 then hold for terminals.

### Effect of the vision

Target 2: a program of another line cannot learn that keys arrived: its `PollReadable` is `IO` and a held one is not
answered while its line is not in front (ADR-0331 item 4). Target 3: nothing per machine. Networking: `ssh`'s relay
loop polls a terminal and a socket together; this is what lets it wait on both. Threads: the request is one per
connection, shared by the process's threads under the library's lock.

## Consequences

- **Positive.** `vi`, `less`, curses and `ssh` can wait for a key and a signal or a socket; `sh` has job control in
  a window (`/dev/tty`); `tcsetattr` works for the ports that were refused (`curl`'s password prompt).
- **Negative.** A request stays outstanding in the service after a `poll` that ended otherwise; the library holds
  three small vectors of state per terminal connection. Seventeen names enter `libc.a`.
- **Follow-up.** `poll` of a terminal in a window is tested through `t_tty` in the shell's script; the GUI scripts
  of part 10c read it in the window.

## Invariants

- **INV-0389-1.** A terminal's input is reported ready by `poll` only when the service answered the readiness
  request, bytes the library kept, or the service went away (`POLLHUP`), and the same look run again sends no second
  request. *Check:* `cargo test -p zoraya-posix --test poll`.
- **INV-0389-2.** `tcsetattr` sends `SetAttr` alone for `TCSANOW`, `Drain` then `SetAttr` for `TCSADRAIN` and `Drain`,
  `Flush` then `SetAttr` for `TCSAFLUSH`; the service's `IO` is `EIO`. *Check:* `cargo test -p zoraya-posix --test
  tty`.
- **INV-0389-3.** `open("/dev/tty")` makes a connection of its own tagged with the caller's group and fails with
  `ENXIO` when the table's entry has none; `ttyname` is `/dev/tty` for a terminal and `ENOTTY` for anything else.
  *Check:* `cargo test -p zoraya-posix --test tty` and `make test-posix` (`t_tty names`).

## Open questions

- Whether `TIOCSTI` should be `EPERM` rather than `ENOTTY` (Linux without the capability gives `EPERM`); `ENOTTY`
  is what a kernel built without it gives, and no program here reads the difference.

## Revisit triggers

- A port that polls a terminal and is slow to answer a key because its `PollReadable` is outstanding behind a
  `Read` of another thread.

## Architect's decision

Accepted 2026-11-18. Decided by the architect; recorded by an agent at the architect's request.

Option A.
