# ADR-0331: A terminal for POSIX programs that `zosh` starts: the shell hosts the line discipline

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0096, ADR-0101, ADR-0104, ADR-0105, ADR-0106, ADR-0107, ADR-0152, ADR-0153, ADR-0154, ADR-0158, ADR-0200
- **Evidence:** CP-25, CP-27, DM-069, DM-070, DM-072, DM-073, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#qnx, stage-10/survey.md#a-pty-pair-as-capabilities, stage-10/survey.md#ptys-without-a-global-name, stage-10/survey.md#ssh-and-the-terminal, stage-10/survey.md#the-name-devtty, stage-10/survey.md#the-terminal-as-built, stage-10/survey.md#the-tty-service-as-built, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal, stage-10/survey.md#what-ncurses-needs-from-libc, stage-10/survey.md#which-programs-need-a-pty, stage-10/survey.md#window-size

## Context

Stage 10 wants `vi`, `less`, an interactive `sh`, `git` asking for a password and `ssh` in a desktop terminal. Today
none of them can run there. A POSIX program that `zosh` starts gets three streams and no terminal, so `isatty` is
false and keys reach it a line at a time, cooked by the shell (`stage-10/survey.md#the-terminal-as-built`). The
terminal service of ADR-0105, `ttyd`, runs only in the POSIX session that replaces `zosh` for the tests. ADR-0153
recorded this as a known gap, and the architect accepted the note on ADR-0105 item 9 with the condition "to be
revisited before the first interactive POSIX program is wanted from `zosh`". This ADR is that revisit. It decides
who gives a program of a `zosh` line its terminal, what the terminal then promises (raw mode, timed reads, the window
size, `SIGWINCH`, `/dev/tty`), and how the shell gets a usable screen back.

**What is known.**

- Every program in stage 10's list uses a terminal it is given; none creates one. `vi`, `less`, `git`, the ssh
  client and Python's REPL set raw mode with `tcsetattr`, read the size with `TIOCGWINSZ` and catch `SIGWINCH`
  (`stage-10/survey.md#which-programs-need-a-pty`, `stage-10/survey.md#ssh-and-the-terminal`,
  `stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`).
- curses waits for input with `poll` or `select` while it draws, and takes `SIGWINCH` and `SIGTSTP`
  (`stage-10/survey.md#what-ncurses-needs-from-libc`).
- `less`, git's and ssh's password prompts and Python's `getpass` open `/dev/tty` by name, each with a fallback
  (`stage-10/survey.md#the-name-devtty`).
- The discipline is a `no_std` crate, `zoraya-tty`, with `ttyd` as a thin host. Non-canonical mode exists in it;
  `VMIN` and `VTIME` are never read; `pselect` calls a terminal ready at once; `open("/dev/tty")` is `ENXIO`;
  `tcsetattr` and `poll` are not exported (`stage-10/survey.md#the-tty-service-as-built`).
- `zosh` already holds its window's channel, relays every byte of its programs' output, knows which job is in front
  (ADR-0158) and stops and continues POSIX elements through the broker (ADR-0200).

**What is assumed.** That the copy through `zosh` is fast enough for a full-screen repaint. Nothing measures the
terminal's throughput today (`stage-10/survey.md#the-terminal-as-built`); item 12 makes it a measurement of the
first implementation part.

**Forces from the catalog.** Terminal policy stays out of the kernel (DM-069, ADR-0060 and ADR-0105 decided it).
Sessions and controlling terminals were designed for one physical terminal (DM-070). No path may put bytes into
another program's input (DM-072). CP-25 wants the rights of a terminal separated and a VT translation for POSIX
programs; a typed console that unmodified programs cannot use needed a translator in the end (DM-073).

## Decision drivers

1. **Unmodified programs.** The ports call termios and expect a VT byte stream (DM-073, CP-27: the scope grows by
   named programs, not by rewriting them).
2. **No terminal policy in the kernel, and no global terminal name** (DM-069, DM-070,
   `stage-10/survey.md#a-pty-pair-as-capabilities`).
3. **One implementation of the discipline**, host-tested, for the window, the serial console and the test session
   (`stage-10/survey.md#the-tty-service-as-built`).
4. **The shell stays where the person's authority is**: it relays, it knows the job in front, and ADR-0337 needs a
   key that no program sees (`stage-10/survey.md#the-terminal-as-built`).
5. **The person always gets a working screen back**, whatever a program left behind.
6. **Nothing typed for one program reaches another** (DM-072, CP-25).

## Considered options

### Option A: A kernel pseudo-terminal between the terminal and the shell (Linux)

- **Description:** The kernel gains pty pairs with termios, the line discipline and the foreground group. `term`
  holds the master, `zosh` and its programs the slave, and every program inherits the slave as fds 0 to 2.
- **Evidence for:** Exactly what the ports expect, with nothing in the shell; it works while every user process is
  stuck; every Unix terminal is built this way (`stage-10/survey.md#which-programs-need-a-pty`).
- **Evidence against:** DM-069 and DM-070 in the kernel, against ADR-0060, ADR-0096 Option A and ADR-0105 Option A,
  each of which the architect accepted with that reasoning. The kernel has no process groups or signals to send
  (ADR-0004). The history of the pty's global name is a history of repairs
  (`stage-10/survey.md#a-pty-pair-as-capabilities`).
- **Cost:** *(estimate)* Thousands of lines of kernel code and a second discipline beside `zoraya-tty`.
- **Precedent:** Unix; it works, and its layer is the one DM-069 describes.

### Option B: `zosh` hosts the discipline for its own window

- **Description:** `zosh` links `zoraya-tty` and serves `zoraya-proto-tty` terminal connections to the POSIX programs
  of its lines, as `ttyd` does in a session. Output stays a stream whose read end the shell holds; input and control
  are requests the shell answers from the keys `term` sends it.
- **Evidence for:** Drivers 1 to 6. The shell is already on the path of every byte and already decides which job is
  in front, so nothing new is trusted and no process is added per window. The crate and its host tests are reused
  (driver 3). The split of a stream for output and a connection for input is ADR-0105's, which followed QNX's
  resource managers (`alternatives-postmortem.md#qnx`).
- **Evidence against:** The shell grows, and a fault in the discipline is a fault in the shell. Output is copied
  through the shell, unmeasured. Two hosts of one crate must be kept alike. ADR-0105 pictured "several `ttyd`
  instances" for several windows.
- **Cost:** *(estimate)* 600 lines in `zosh` and `zoraya-shell`, 300 in `zoraya-tty` (timed reads, flush), 400 in
  `libposix` (the readiness request, `/dev/tty`, the hidden termios names), tests.
- **Precedent:** ADR-0105's `ttyd` is this code under another host. Redox keeps the discipline in a user daemon,
  Fuchsia in a service, and both hand it out as handles (`stage-10/survey.md#ptys-without-a-global-name`).

### Option C: A `ttyd` per window between `term` and the shell

- **Description:** `init` or the panel starts `term`, a `ttyd` and `zosh` for each window. `zosh` becomes a client of
  `ttyd` in raw mode for its own editor, clones a connection for each POSIX program, and tells `ttyd` the foreground
  through the control connection that the broker holds in a session.
- **Evidence for:** It is ADR-0105's picture for target 3. The discipline lives outside the shell, so a shell fault
  leaves it running; output goes from the program to `ttyd` without passing the shell.
- **Evidence against:** One more process per window. The shell must still see every stream of a pipeline that mixes
  native and POSIX programs (ADR-0152) and must still own job control for mixed lines (ADR-0200), so it leaves only
  half of the path. The control connection belongs to the broker (INV-0105-2), and `zosh` has no pid: either the
  broker learns the shell's jobs or `ttyd` takes a second kind of controller. ADR-0337's key and status line would
  have to be `ttyd`'s and travel on to the shell, which holds the grants.
- **Cost:** *(estimate)* Option B's library work, plus a controller protocol between `zosh`, the broker and `ttyd`,
  plus the start-up wiring per window.
- **Precedent:** Every Unix terminal with a shell on a pty; ADR-0105's session.

### Option D: No termios: a native console that programs are ported to (Plan 9's `rio`)

- **Description:** A window serves text and a small control request ("raw on", "raw off"), as `rio` serves
  `/dev/cons` and `consctl`; each port is changed to use it.
- **Evidence for:** No discipline at all, no in-band state to restore, the smallest surface (CP-25's native half).
  Plan 9 ran its own editors this way (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Plan 9 needed a VT emulator, `vt`, for every foreign program
  (`stage-10/survey.md#ptys-without-a-global-name`). `vi`, `less`, curses, git and ssh would each need a patch set
  that follows them for ever (driver 1, CP-27). Windows went the other way and built ConPTY (DM-073).
- **Cost:** *(estimate)* Small in the system, unbounded in the ports.
- **Precedent:** Plan 9, where the model held for native programs and foreign ones lived in `vt`.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The shell is the terminal service of its window.** `zosh` holds a `zoraya_tty::Terminal` and answers
   `zoraya-proto-tty` requests in its event loop, on the window and on the serial console alike. `ttyd` stays the
   host for the session of `/etc/zoraya/session` and for the tests that drive it. Both hosts get the same crate, so
   items 4 to 6 hold in a session too.
2. **What a POSIX program receives.** Each of fds 0, 1 and 2 that the line does not redirect or pipe is a tty
   description (ADR-0103, ADR-0105 item 6): a terminal connection tagged with the program's group, and the write end
   of the line's output stream. `isatty` is true. The namespace table's `Terminal` entry (ADR-0107 item 3) is present
   when at least one of the three is a terminal, and then `open("/dev/tty")` clones the program's own connection and
   `ttyname` answers `/dev/tty`, as ADR-0105 items 4 and 6 decided and the code does not do yet. With no terminal
   `open("/dev/tty")` fails with `ENXIO`, as today; `less`, git and ssh each have a fallback for that
   (`stage-10/survey.md#the-name-devtty`). Children get clones through `libposix`, as in a session.
3. **It is not a new grant.** `grants` already prints `in, out, err   the terminal`. What the line shows is what the
   program has: with `< file` or in a pipe that end is the file or the pipe, and nothing else lets it read keys.
4. **Raw mode is complete.** `zoraya-tty` implements `VMIN` and `VTIME` in POSIX's four cases; `libposix` sends
   `PollReadable`, so `pselect`, `select` and `poll` on a terminal wait until a read would not block;
   `tcsetattr`'s three actions differ as POSIX says (`TCSADRAIN` waits for the output to be shown, `TCSAFLUSH` also
   drops unread input); `tcflush` exists. The names enter `libc.a` by ADR-0342.
5. **The foreground.** The discipline's foreground job is the job `zosh` has in front (ADR-0158). The signal
   characters act as ADR-0105 item 3 says: INTR and QUIT are one `job_signal` on that job, SUSP goes through the
   broker for POSIX elements and `job_suspend` for native ones (ADR-0200). The broker's control connection for this
   window ends at `zosh`, so a POSIX shell running in front (`sh`) can hand the terminal to its own jobs with
   `tcsetpgrp`: the request is accepted only for a group inside the line that is in front. A read from a job that is
   not in front is `NotForeground` (ADR-0105 item 8).
6. **The window size.** `term`'s `Size` message sets the discipline's size; the shell then sends
   `job_signal(front, Custom(28))`, `SIGWINCH`, as `ttyd` does. `TIOCGWINSZ` answers rows and columns; the pixel
   fields are 0. A program cannot set the size (`TIOCSWINSZ` is `ENOTTY`): only the window does, as only the server
   side may on Fuchsia (`stage-10/survey.md#ptys-without-a-global-name`). The shell never puts `LINES` or `COLUMNS`
   in a program's environment, because programs read them before the real size and then miss every change
   (`stage-10/survey.md#window-size`).
7. **The screen comes back.** When the job in front ends, stops or goes to the background, the shell (a) sets the
   discipline's attributes back to its defaults, keeping a stopped job's attributes to restore at `fg`, and (b)
   sends `term` a new out-of-band message, `Restore` (a new ordinal of `zoraya-proto-term` under ADR-0052), which
   ends any unfinished escape sequence, leaves the alternate screen, shows the cursor, and clears the scroll region,
   the keypad, paste and mouse modes and the SGR state of ADR-0332, without erasing the primary screen. After `fg`
   the shell sends the job `SIGWINCH` so that a full-screen program repaints. `Restore` is a message, not bytes,
   because the program's output may have stopped in the middle of a sequence.
8. **Keys.** In front, a POSIX program with the terminal gets every key as the bytes of ADR-0332, through the
   discipline. The shell keeps one chord for itself, ADR-0337's, which `term` reports in a message of its own and
   never as bytes. Nothing a program writes becomes input except the replies ADR-0332 names, and only for the job in
   front (INV-0331-4).
9. **Native programs are unchanged.** They get streams, and the shell cooks their lines (ADR-0152). A native
   full-screen interface is not decided here.
10. **Background output** is shown as it comes (`TOSTOP` stays off, ADR-0105 item 8), above the line being edited,
    as ADR-0158 already does.
11. **`TERM`.** The shell sets it, as ADR-0333 says, for a POSIX program that receives the terminal, and leaves it
    unset for one that does not.
12. **Measured before it is relied on.** The first implementation part records, under HVF and TCG, the time for
    `cat` of 1 MiB to the window and for 100 full repaints of an 80 by 24 screen by a test program, with the share
    spent in `zosh`. If the shell's copy is more than half of either, Option C is reconsidered.

**Examples.**

```
~/proj> sh
$ test -t 0 && echo a terminal
a terminal
$ exit
~/proj> echo x | sh -c 'test -t 0 || echo not a terminal'
not a terminal
~/proj> less notes.txt
```

`less` takes the whole window; `q` leaves it and the prompt and what was on the screen before are back. With Ctrl-Z:

```
[1] stopped: less notes.txt
~/proj> fg
```

and `less` is on the screen again, repainted. If a program dies in raw mode with the cursor hidden, the next prompt
is still typed and shown normally (item 7). On the desktop the same lines run in the terminal window; dragging the
window's corner while `less` is open redraws it at the new size.

**Tensions with accepted ADRs.**

- *ADR-0105 item 9 and its note of 2026-11-03*: "`zosh` owns the terminal and gives POSIX programs streams, not a
  tty connection". This ADR replaces that sentence for programs in front; it is the revisit the note asks for.
- *ADR-0105 item 1 and INV-0105-2*: `ttyd` is the service and only the broker's control connection changes the
  foreground. In a window `zosh` is the service and the foreground is the shell's own job in front; the broker's
  control connection ends at `zosh` and is the only way a POSIX program changes it (item 5). INV-0105-2's wording
  holds with "the terminal service" read for `ttyd`; its test gains the shell as a second host.
- *ADR-0105 items 4 and 6* (`/dev/tty`, `ttyname`) are carried out, not changed.
- *ADR-0105's deferred list* (non-canonical mode, readiness on a tty) is taken up; `TOSTOP` and `SIGTTOU` stay
  deferred.
- *ADR-0096's protocol* gains `Restore` (item 7) and, by ADR-0337, two more messages. Its "no answers" rule is
  ADR-0332's subject.
- *ADR-0153*'s consequence "interactive POSIX programs do not run from `zosh` yet" ends; its revisit trigger said
  "lend it `ttyd` (ADR-0105) by a superseding ADR", and this ADR lends the discipline, not the daemon.
- *ADR-0005* lists raw mode, `SIGWINCH` and job control beyond Ctrl-C as outside the acceptance tests. ADR-0342
  brings them in with the programs that need them.

### Effect of the vision

For target 2 this keeps the split ADR-0105 wanted: a program in an untrusted room holds a connection that can read
keys only while its job is in front and can never inject them, and the service on the other end is the room's own
shell, which holds no more than the room (ADR-0172). A supervisor of an agent would be one more client of the same
connection kind. For target 3 each terminal window is self-contained: one `term`, one `zosh`. Networking: the ssh
client needs exactly items 2, 4 and 6. Threads: a tty read is a wait point of the thread that makes it (ADR-0304).

## Consequences

- **Positive.** `vi`, `less`, `sh`, password prompts and ssh become possible in every terminal window and on the
  serial console, with one discipline, no kernel change and no new process. The shell can always restore the screen.
- **Negative.** `zosh` carries the discipline. Output is copied through the shell. A native program still cannot
  draw a full screen. The session's `ttyd` and the shell must be kept in step by shared tests.
- **Follow-up.** `VMIN`/`VTIME`, flush and drain in `zoraya-tty`; the readiness request, `/dev/tty` and `ttyname` in
  `libposix`; `Restore` in `zoraya-proto-term` and `term`; the measurement of item 12; `dash.cmd`'s sentence on having
  no terminal; `docs/design.md`.

## Invariants

- **INV-0331-1.** A POSIX program of a `zosh` line has a terminal on exactly those of fds 0, 1 and 2 that the line
  neither redirects nor pipes, and `open("/dev/tty")` succeeds exactly when it has one. *Check:* a host test of the
  shell's plan for lines with every combination of redirection and pipe, and an in-QEMU script running a C program
  that prints `isatty` of each fd and the result of the open, on the serial console and in the window.
- **INV-0331-2.** With `ICANON` off, a read obeys `VMIN` and `VTIME` in the four cases POSIX defines, and `pselect`
  on a terminal fd returns ready only when a read would not block. *Check:* host tests of `zoraya-tty` with a fake
  clock for the four cases, and an in-QEMU C program that selects with a 200 ms timeout on an idle terminal and
  reports the elapsed time.
- **INV-0331-3.** After the job in front ends or stops, whatever it wrote and whatever attributes it set, the
  discipline has its default attributes and the terminal is on the primary screen with the cursor shown and no
  mode of ADR-0332 set. *Check:* a host test feeding `zoraya-term` each mode-setting sequence and then `Restore`;
  an in-QEMU script that kills a program which set raw mode, the alternate screen and a hidden cursor, then types
  a command and reads the screen.
- **INV-0331-4.** No byte reaches a program's terminal read that was not a key pressed while its job was in front
  or a reply of ADR-0332 to that same job. *Check:* a host test of the shell's input path with two jobs, keys and
  replies arriving around a change of the job in front; the test of INV-0096-2 as amended by ADR-0332.
- **INV-0331-5.** A change of the window's size reaches the job in front as one `SIGWINCH`, and `TIOCGWINSZ`
  then gives the new size; no program request changes the size. *Check:* a host test of the shell's handling of
  `Size`; the GUI test resizes the window over QMP while a C program with a handler prints each size it reads.
- **INV-0331-6.** `zoraya-tty` is the only implementation of the discipline: `ttyd` and `zosh` both link it, and
  neither tests a termios flag itself. *Check:* a lint (`grep` for the flag names outside `crates/zoraya-tty` in
  `user/src/bin/ttyd.rs`, `user/src/bin/zosh.rs` and `crates/zoraya-shell`), in `make check`.

## Open questions

- Whether the copy through the shell is fast enough (item 12); resolved by the measurement.
- Whether a stopped full-screen program's screen should be kept and shown again at `fg` instead of asking it to
  repaint; a second alternate buffer per job would do it, at a cost in memory.
- Whether native programs should get the same connection, so that a native full-screen program can exist; no
  native program needs it yet.
- `TOSTOP` and `SIGTTOU`: still no program in scope depends on them.

## Revisit triggers

- The measurement of item 12 shows the shell's copy dominating.
- A second kind of shell hosts a window (a POSIX shell without `zosh`): Option C's `ttyd` per window becomes the
  simpler shape.
- A program in scope needs to set the window size, or needs a terminal that is not a window (ADR-0338).

## Architect's decision
