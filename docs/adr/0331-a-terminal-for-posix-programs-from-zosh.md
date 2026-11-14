# ADR-0331: A terminal for POSIX programs that `zosh` starts: the shell hosts the line discipline

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0060, ADR-0096, ADR-0101, ADR-0104, ADR-0105, ADR-0106, ADR-0107, ADR-0152, ADR-0153, ADR-0154, ADR-0158, ADR-0200
- **Evidence:** CP-25, CP-27, DM-069, DM-070, DM-072, DM-073, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#qnx, stage-10/survey.md#a-pty-pair-as-capabilities, stage-10/survey.md#ptys-without-a-global-name, stage-10/survey.md#ssh-and-the-terminal, stage-10/survey.md#the-name-devtty, stage-10/survey.md#the-terminal-as-built, stage-10/survey.md#the-tty-service-as-built, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal, stage-10/survey.md#what-ncurses-needs-from-libc, stage-10/survey.md#which-programs-need-a-pty, stage-10/survey.md#window-size

## Context

Stage 10 wants `vi`, `less`, an interactive `sh`, `git` asking for a password and `ssh` in a desktop terminal. Today
none of them can run there. A POSIX program that `zosh` starts gets three streams and no terminal, so `isatty` is
false and keys reach it a line at a time, cooked by the shell (`stage-10/survey.md#the-terminal-as-built`). The
terminal service of ADR-0105, `ttyd`, runs only in the POSIX session that replaces `zosh` for the tests. ADR-0153
recorded this as a known gap, and the architect accepted the note on ADR-0105 item 9 with the condition "to be
revisited before the first interactive POSIX program is wanted from `zosh`". This ADR is that revisit. It decides
who gives a program of a `zosh` line its terminal, what the terminal then promises (raw mode, timed reads, the window
size, `SIGWINCH`, `/dev/tty`), who may read keys and change the terminal's modes, and how the shell gets a usable
screen back.

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
- In that crate a cloned connection carries the group its caller declares, and a request to change the
  attributes is accepted from any connection (`crates/zoraya-tty/src/lib.rs`, `Clone` and `SetAttr`). ADR-0105's
  "Effect of the vision" names the declared tag as "the one trust gap (stage 6, trusted processes)".
- `zosh` holds its window's channel, relays the terminal-facing ends of its programs' streams, knows which line is
  in front (ADR-0158) and stops and continues POSIX elements through the broker (ADR-0200). The broker that runs
  beside `zosh` is started without a shell and without a terminal: a `tcsetpgrp` from one of its clients is
  answered "no terminal" today (`user/src/beside.rs`, `crates/zoraya-broker`).
- On the serial console the kernel writes a carriage return before every line feed and refuses a write that is
  not valid UTF-8 (`kernel/src`, the PL011 driver and `console_write`).

**What is assumed.** That the copy through `zosh` is fast enough for a full-screen repaint. Nothing measures the
terminal's throughput today (`stage-10/survey.md#the-terminal-as-built`); item 14 makes it a measurement of the
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
4. **The shell stays where the person's authority is**: it knows the line in front, and ADR-0337 needs a key that
   no program sees (`stage-10/survey.md#the-terminal-as-built`).
5. **The person always gets a working screen back**, whatever a program left behind.
6. **Only the line in front reads keys or changes the terminal's modes**: nothing typed for one program reaches a
   program of another line, and no program of another line can turn echo on under a password prompt (DM-072,
   CP-25). Inside one line this ADR does not separate programs (item 4).

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
  are requests the shell answers from the keys `term` sends it. The shell, which starts every line, tags each
  connection with its line.
- **Evidence for:** Drivers 1 to 6. The shell already decides which line is in front and already holds the
  window's channel, so no process is added per window and the tag that driver 6 needs is the shell's own knowledge.
  The crate and its host tests are reused (driver 3). The split of a stream for output and a connection for input
  is ADR-0105's, which followed QNX's resource managers (`alternatives-postmortem.md#qnx`).
- **Evidence against:** The shell that holds the person's grants now parses requests from every program it runs,
  untrusted ones included; a fault in the discipline is a fault in the shell. The broker beside `zosh` has no
  terminal today, so it must learn to pass a POSIX shell's `tcsetpgrp` on to `zosh`. Output is copied through the
  shell, unmeasured. Two hosts of one crate must be kept alike. ADR-0105 pictured "several `ttyd` instances" for
  several windows.
- **Cost:** *(estimate)* 600 lines in `zosh` and `zoraya-shell`, 400 in `zoraya-tty` (timed reads, flush, tags),
  300 in the broker (a control channel to its native client, foreground requests for client groups), 400 in
  `libposix` (the readiness request, `/dev/tty`, the hidden termios names), tests.
- **Precedent:** ADR-0105's `ttyd` is this code under another host; nothing surveyed puts a discipline inside a
  shell.

### Option C: A `ttyd` per window between `term` and the shell

- **Description:** `init` or the panel starts `term`, a `ttyd` and `zosh` for each window. `zosh` becomes a client of
  `ttyd` in raw mode for its own editor, asks it for a connection for each POSIX program, and tells it which line
  is in front.
- **Evidence for:** It is ADR-0105's picture for target 3 and the shape of the systems surveyed: Redox keeps the
  discipline in a daemon and Fuchsia in a service, and both hand it out as handles
  (`stage-10/survey.md#ptys-without-a-global-name`). The requests of untrusted programs (attributes, clones,
  reads) are parsed by a small process that holds nothing but the window's channel, not by the shell that holds
  the person's grants. A shell fault leaves the discipline running. Output goes from the program to `ttyd` without
  passing the shell.
- **Evidence against:** One more process per window. The shell must still tell `ttyd` the line in front and the
  tag of each connection, a controller protocol that does not exist (the control connection of ADR-0105 item 5
  belongs to the broker). ADR-0337's key and status line would be `ttyd`'s and travel on to the shell, which holds
  the grants. The same broker work as Option B for a POSIX shell's `tcsetpgrp`.
- **Cost:** *(estimate)* Option B's library and broker work, plus the controller protocol between `zosh` and
  `ttyd`, plus the start-up wiring per window.
- **Precedent:** Redox's `ptyd`, Fuchsia's `ptysvc`; every Unix terminal with a shell on a pty; ADR-0105's session.

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

**Recommendation (not a decision): Option B.** Option C is the serious alternative: it costs a process and a
protocol more and keeps untrusted requests out of the shell. The architect is asked to choose between them
knowingly; items 2 to 13 hold under either, with `ttyd` in the shell's place.

1. **The shell is the terminal service of its window.** `zosh` holds a `zoraya_tty::Terminal` and answers
   `zoraya-proto-tty` requests in its event loop. `ttyd` stays the host for the session of `/etc/zoraya/session`
   and for the tests that drive it. Both hosts get the same crate, so items 5 and 7 hold in a session too;
   `ttyd` ignores the messages of `zoraya-proto-term` it does not know (`Paste`, `Restore`, ADR-0337's).
2. **What a POSIX program receives.** Each of fds 0, 1 and 2 that the line does not redirect or pipe is a tty
   description (ADR-0103, ADR-0105 item 6): a terminal connection and the write end of the line's output stream.
   `isatty` is true. The namespace table's `Terminal` entry (ADR-0107 item 3) is present when at least one of the
   three is a terminal, and then `open("/dev/tty")` clones the program's own connection and `ttyname` answers
   `/dev/tty`, as ADR-0105 items 4 and 6 decided and the code does not do yet. With no terminal
   `open("/dev/tty")` fails with `ENXIO`, as today; `less`, git and ssh each have a fallback for that
   (`stage-10/survey.md#the-name-devtty`).
3. **A program with the terminal on any of the three can read keys while its line is in front**, from that fd or
   from `/dev/tty`: redirecting its input does not take the keyboard away, as it does not on Unix (it is how
   `less` reads keys under a pipe). `grants` says so with a line of its own whenever any of the three is the
   terminal:

   ```
   keys                 the terminal, while this line is in front
   ```

   A program whose three streams are all elsewhere has no terminal and no keys.
4. **Tags, not declarations.** The shell marks every connection it hands out with the line it started the
   program for. A clone carries its parent's line; the group a caller declares when it clones (ADR-0105 item 4)
   is believed only among the programs of that same line, which share one set of grants. `Read`, `PollReadable`
   and `SetAttr` from a connection whose line is not in front are refused: the read as ADR-0105 item 8 says
   (`NotForeground`, then the stop or `EIO`), the others with `EIO`. `GetAttr` and the window size are answered for
   any connection. This closes, for programs of different lines, the gap ADR-0105 left open. **It does not close it
   inside a line**, and a line lasts as long as any process of its job: a child that a command typed inside `vi`
   leaves running, or a job a POSIX shell put in its own background, is still of the line in front, can declare
   the chosen group, and can then read what is typed there (a password included) or change the modes. The broker
   knows which group each process was created in and could refuse a false declaration; that check is not part of
   this ADR (open question), and the gap is recorded as accepted for stage 10.
5. **Raw mode is complete.** `zoraya-tty` implements `VMIN` and `VTIME` in POSIX's four cases; `libposix` sends
   `PollReadable`, so `pselect`, `select` and `poll` on a terminal wait until a read would not block;
   `tcsetattr`'s three actions differ as POSIX says (`TCSADRAIN` waits for the output to be shown, `TCSAFLUSH` also
   drops unread input); `tcflush` exists. The names enter `libc.a` by ADR-0342.
6. **The line in front, and the group within it.** The discipline's foreground is the line `zosh` has in front
   (ADR-0158); only the shell's own job control changes it. The signal characters act as ADR-0105 item 3 says:
   INTR and QUIT are one `job_signal` on that line's job, SUSP goes through the broker for POSIX elements and
   `job_suspend` for native ones (ADR-0200). A POSIX shell running in front (`sh`) hands the terminal to its own
   jobs with `tcsetpgrp`: the broker beside `zosh` gains a control channel to it and passes the request on, and
   the shell accepts it only for a group the broker says was created inside the line that is in front. Within
   that line a read from a group that is not the chosen one is `NotForeground` (ADR-0105 item 8).
7. **The window size.** `term`'s `Size` message sets the discipline's size; the shell then sends
   `job_signal(front, Custom(28))`, `SIGWINCH`, as `ttyd` does. `TIOCGWINSZ` answers rows and columns; the pixel
   fields are 0. A program cannot set the size (`TIOCSWINSZ` is `EPERM`): only the window does, as only the server
   side may on Fuchsia (`stage-10/survey.md#ptys-without-a-global-name`). The shell never puts `LINES` or `COLUMNS`
   in a program's environment, because programs read them before the real size and then miss every change
   (`stage-10/survey.md#window-size`).
8. **Who writes the carriage return.** ADR-0332 makes LF a line feed that keeps the column. The shell therefore
   adds the carriage return to its own output and to the output of native programs, which write `\n` alone; for a
   POSIX program the discipline's `OPOST` and `ONLCR` decide, as its attributes stand when the shell reads the
   bytes from the stream. A program that prints and then at once turns `OPOST` off may so lose the carriage
   returns of what it had just written; `TCSADRAIN`, which curses and `vi` use, avoids it.
9. **The screen comes back.** When the line in front ends, stops or goes to the background, the shell (a) sets
   the discipline's attributes back to its defaults, keeping a stopped line's attributes to restore at `fg`, and
   (b) sends `term` a new out-of-band message, `Restore` (a new ordinal of `zoraya-proto-term` under ADR-0052),
   which ends any unfinished escape sequence, leaves the alternate screen, shows the cursor, and clears the scroll
   region, the keypad, paste and mouse modes and the SGR state of ADR-0332, without erasing the primary screen.
   For a stop, `term` keeps the modes it cleared and sets them again when the shell says the line is back in
   front; the shell then sends the line `SIGWINCH` so that a full-screen program repaints. `Restore` is a message,
   not bytes, because the program's output may have stopped in the middle of a sequence.
10. **Keys.** In front, a POSIX program with the terminal gets every key as the bytes of ADR-0332, through the
    discipline. The shell keeps one chord for itself, ADR-0337's, which `term` reports in a message of its own and
    never as bytes. Nothing a program writes becomes input (INV-0096-2 stands, ADR-0332).
11. **Native programs are unchanged.** They get streams, and the shell cooks their lines (ADR-0152). A native
    full-screen interface is not decided here.
12. **Background output** is shown as it comes (`TOSTOP` stays off, ADR-0105 item 8), above the line being edited,
    as ADR-0158 already does. It can draw on the screen of the program in front; it cannot read its keys or change
    its modes (item 4).
13. **The serial console is for lines, not full screens, in stage 10.** There the shell hosts the discipline too:
    `sh`, password prompts and raw-mode input work. But the kernel's console adds a carriage return before every
    line feed and refuses bytes that are not UTF-8, so a full-screen program's output is not carried exactly. A
    byte-exact console write is a kernel decision this ADR does not take (open question); until then `vi` and
    `less` are for the window, and ADR-0333 sets `TERM` on the serial console accordingly.
14. **Measured before it is relied on.** The first implementation part records, under HVF and TCG, the time for
    `cat` of 1 MiB to the window and for 100 full repaints of an 80 by 24 screen by a test program, with the share
    spent in `zosh`. If the shell's copy is more than half of either, Option C is reconsidered.
15. **`TERM`.** The shell sets it, as ADR-0333 says, for a POSIX program that receives the terminal, and leaves it
    unset for one that does not.

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
is still typed and shown normally (item 9). On the desktop the same lines run in the terminal window; dragging the
window's corner while `less` is open redraws it at the new size. (`sh` is `/bin/sh`, a link to dash; ADR-0336 gives
every program in `/bin` a description, `sh` included.)

**Tensions with accepted ADRs.**

- *ADR-0105 item 9 and its note of 2026-11-03*: "`zosh` owns the terminal and gives POSIX programs streams, not a
  tty connection". This ADR replaces that sentence for programs with a terminal; it is the revisit the note asks
  for.
- *ADR-0105 item 1, item 5 and INV-0105-2* ("only the broker's control connection can change the foreground
  group"). In a window the service is `zosh`, and the foreground *line* is changed by the shell's own job control,
  not by the broker; only the group within the line in front is changed through the broker. INV-0105-2 does not
  hold as worded for a window; INV-0331-2 is its form there, and INV-0105-2 stays true of the session's `ttyd`.
- *ADR-0104 item 10* ("any process of the session may make a group of the session the foreground group") is
  narrowed for a window: only a group created inside the line in front (item 6). The broker run with `--no-shell`
  gains a control channel to its native client; ADR-0153 rule 10 and ADR-0200 gave it none.
- *ADR-0105 item 4* (`clone(group pid, server_end)`: a connection "tagged with the group the caller declares") and
  its "Effect of the vision" (the declared tag as the stage 6 trust gap): item 4 adds the line tag the caller
  cannot choose.
- *ADR-0105 items 4 and 6* (`/dev/tty`, `ttyname`) are carried out, not changed. *ADR-0105's deferred list*
  (non-canonical mode, readiness on a tty) is taken up; `TOSTOP` and `SIGTTOU` stay deferred.
- *ADR-0152 item 1 and ADR-0154 rules 7 and 8*: a native program's output is passed "as it is" and the editor's
  subset has `\n`. Item 8 makes the shell add the carriage return on the way to `term`; what a program writes and
  what a pipe carries are unchanged.
- *ADR-0153 rule 7*: `grants` gains the `keys` line of item 3.
- *ADR-0096's protocol* gains `Restore` (item 9) and, by ADR-0332 and ADR-0337, three more messages.
- *ADR-0153*'s consequence "interactive POSIX programs do not run from `zosh` yet" ends; its revisit trigger said
  "lend it `ttyd` (ADR-0105) by a superseding ADR", and this ADR lends the discipline, not the daemon.
- *ADR-0060 and INV-0105-4*: unchanged; item 13 records what the kernel's console write still does to output.
- *ADR-0005* lists raw mode, `SIGWINCH` and job control beyond Ctrl-C as outside the acceptance tests. ADR-0342
  brings them in with the programs that need them.

### Effect of the vision

For target 2 this is where ADR-0105's split has to become real, because every program of every line now holds a
terminal connection. Item 4 is the answer: a program of another line, in the background or not, cannot read what is
typed to the program in front and cannot change its modes; the service on the other end is the room's own shell,
which holds no more than the room (ADR-0172). What remains is inside one line, whose programs share their grants but
can also each read what is typed there, a password included (item 4), and the shell's exposure to the requests of the
programs it runs, which Option C would remove. A supervisor
of an agent would be one more client of the same connection kind. For target 3 each terminal window is
self-contained: one `term`, one `zosh`. Networking: the ssh client needs exactly items 2, 5 and 7. Threads: a tty
read is a wait point of the thread that makes it (ADR-0304).

## Consequences

- **Positive.** `vi`, `less`, `sh`, password prompts and ssh become possible in every terminal window, with one
  discipline, no kernel change and no new process. Only the line in front reads keys and sets modes. The shell can
  always restore the screen.
- **Negative.** `zosh` carries the discipline and parses its programs' terminal requests. The broker gains a
  channel to the shell. Output is copied through the shell. A native program still cannot draw a full screen. The
  serial console does not carry full-screen output. The session's `ttyd` and the shell must be kept in step by
  shared tests. Inside one line any of its processes can read the keys.
- **Follow-up.** `VMIN`/`VTIME`, flush, drain and line tags in `zoraya-tty`; the readiness request, `/dev/tty` and
  `ttyname` in `libposix`; the broker's control channel; `Restore` in `zoraya-proto-term` and `term`; the carriage
  return in the shell's relay; `grants`' `keys` line; the measurement of item 14; `dash.cmd`'s sentence on having
  no terminal; `docs/design.md`.

## Invariants

- **INV-0331-1.** A POSIX program of a `zosh` line has a terminal on exactly those of fds 0, 1 and 2 that the line
  neither redirects nor pipes, `open("/dev/tty")` succeeds exactly when it has one, and `grants` prints the `keys`
  line exactly then. *Check:* a host test of the shell's plan for lines with every combination of redirection and
  pipe, compared with `grants`' output, and an in-QEMU script running a C program that prints `isatty` of each fd
  and the result of the open, in the window and (for a program that is not full-screen) on the serial console.
- **INV-0331-2.** The line in front changes only by the shell's own job control, and the group within it only by a
  request the broker passes on for a group created inside that line; a `Read`, `PollReadable` or `SetAttr` from a
  connection of another line is refused and consumes and changes nothing, whatever group the connection declares.
  *Check:* a host test of `zoraya-tty` and of the shell's request handling with two lines, a background clone that
  declares the front line's group, and each of the three requests; an in-QEMU script in which a background program
  tries to read the terminal and to turn echo on while another program reads a password.
- **INV-0331-3.** With `ICANON` off, a read obeys `VMIN` and `VTIME` in the four cases POSIX defines, and `pselect`
  on a terminal fd returns ready only when a read would not block. *Check:* host tests of `zoraya-tty` with a fake
  clock for the four cases, and an in-QEMU C program that selects with a 200 ms timeout on an idle terminal and
  reports the elapsed time.
- **INV-0331-4.** After the line in front ends or stops, whatever it wrote and whatever attributes it set, the
  discipline has its default attributes and the terminal is on the primary screen with the cursor shown and no
  mode of ADR-0332 set; after `fg` of a stopped line its attributes and modes are those it had. *Check:* a host
  test feeding `zoraya-term` each mode-setting sequence, then `Restore`, then the return to the front; an in-QEMU
  script that kills a program which set raw mode, the alternate screen and a hidden cursor, then types a command
  and reads the screen.
- **INV-0331-5.** No byte reaches a program's terminal read that was not a key pressed, or text pasted, while its
  line was in front. *Check:* a host test of the shell's input path with two lines and keys arriving around a
  change of the line in front; INV-0096-2's test.
- **INV-0331-6.** A change of the window's size reaches the line in front as one `SIGWINCH`, and `TIOCGWINSZ`
  then gives the new size; no program request changes the size. *Check:* a host test of the shell's handling of
  `Size`; the GUI test resizes the window over QMP while a C program with a handler prints each size it reads.
- **INV-0331-7.** Every line feed that reaches `term` from the shell's own output or a native program's is
  preceded by a carriage return, and one from a POSIX program is exactly when its `OPOST` and `ONLCR` are set.
  *Check:* a host test of the shell's relay with a native `\n`, a POSIX `\n` with `OPOST` on and one with it off.
- **INV-0331-8.** `zoraya-tty` is the only implementation of the discipline: `ttyd` and `zosh` both link it, and
  neither tests a termios flag itself. *Check:* a lint (`grep` for the flag names outside `crates/zoraya-tty` in
  `user/src/bin/ttyd.rs`, `user/src/bin/zosh.rs` and `crates/zoraya-shell`), in `make check`.

## Open questions

- Option B or Option C: whether the shell or a `ttyd` per window parses the terminal requests of untrusted
  programs. The measurement of item 14 informs it; the security argument does not depend on the measurement.
- A byte-exact write to the kernel's console (item 13), so that full-screen programs run on the serial console.
- Whether a stopped full-screen program's screen should be kept and shown again at `fg` instead of asking it to
  repaint; a second alternate buffer per line would do it, at a cost in memory.
- Whether native programs should get the same connection, so that a native full-screen program can exist; no
  native program needs it yet.
- Whether the declared group should be checked by the broker inside a line too, for the agent-isolation direction.
- `TOSTOP` and `SIGTTOU`: still no program in scope depends on them.

## Revisit triggers

- The measurement of item 14 shows the shell's copy dominating.
- A fault in the shell traced to a program's terminal request, or the agent-isolation direction is chosen:
  Option C.
- A second kind of shell hosts a window (a POSIX shell without `zosh`): Option C's `ttyd` per window becomes the
  simpler shape.
- A program in scope needs to set the window size, or needs a terminal that is not a window (ADR-0338).

## Architect's decision
