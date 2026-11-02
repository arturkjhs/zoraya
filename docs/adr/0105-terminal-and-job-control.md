# ADR-0105: Terminal and job control

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0004, ADR-0005, ADR-0006, ADR-0040, ADR-0046, ADR-0050, ADR-0052, ADR-0053, ADR-0060, ADR-0101, ADR-0103, ADR-0104
- **Evidence:** CP-02, CP-25, DM-017, DM-069, DM-070, DM-071, DM-072, alternatives-postmortem.md#qnx, stage-6/sources.md#other-programs-and-inputs, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-4-tty-and-job-control, task-02/libc-surface.csv, task-02/method.md#tiers-runs-and-sets, task-02/posix-surface.md#linux-syscall-surface-m2, task-02b/event-timelines.md#w9a-ctrl-c-run-w9-int-d15-r1

## Context

ADR-0004 rule 8 has a terminal service hold the foreground job handle and turn the interrupt character into
`job_signal(fg_job, Interrupt)`, hangup into `job_signal(session_job, Hangup)` and the suspend character into
`job_suspend`; its personality mapping makes `tcsetpgrp` hand the foreground job to the terminal through the broker,
and has a background job's read return `NotForeground`. ADR-0060 keeps every terminal policy out of the kernel: the
console is raw bytes in a ring, and "a terminal service at a later stage must take the rule's form" of ADR-0006, a
byte stream with `READABLE`, `WRITABLE` and `PEER_CLOSED`. Stage 5 (TASK-14, in parallel) builds a GUI terminal that
speaks a protocol to `zosh` over a channel and must not preclude job control. This ADR decides how the kernel console
and the stage 5 terminal become dash's controlling terminal, where the line discipline lives, how `TIOCSPGRP` and the
rest map, and how Ctrl-C and Ctrl-Z reach a job.

**What is measured.**

- Job control is T0 only: `tcsetpgrp`, `tcgetpgrp`, `setpgid`, 15 `TIOCSPGRP` and 1 `TIOCGPGRP` ioctls; no T1 or T2
  program uses process groups (`task-02/callsite-analysis.md#feature-4-tty-and-job-control`). Beyond job control the
  only tty request is the window size, `TIOCGWINSZ`, 552 calls, plus `TCGETS` 12
  (`task-02/posix-surface.md#linux-syscall-surface-m2`).
- W5, the interactive session: a pipe, redirections, a glob, an export, `sleep 2 &`, `jobs`, Ctrl-C of a foreground
  `sleep 30`, `cd`, `wait`, `exit`; no Ctrl-Z (`task-02/method.md#tiers-runs-and-sets`, the W5 row).
- W9a: the tty driver sends `SIGINT` to every process of the foreground group at once; dash later takes the terminal
  back with `TIOCSPGRP` and interrupts itself; `$?` is 130 (`task-02b/event-timelines.md#w9a-ctrl-c-run-w9-int-d15-r1`).
- dash's `setjobctl` opens `/dev/tty`, loops on `tcgetpgrp` until the terminal's group is its own, sending its group
  `SIGTTIN` while it is in the background, then `setpgid(0, pgrp)` and `tcsetpgrp`; `forkchild` hands the terminal to
  a foreground job and sets `SIGTSTP` and `SIGTTOU`; `waitforjob` gives the terminal back with `tcsetpgrp(rootpid)`
  and raises `SIGINT` on itself when the job died of it ([D1], `stage-6/sources.md#other-programs-and-inputs`).
- dash probes `tcgetattr` once for input buffering, and `isatty` serves `test -t`, `read -p` and the interactive
  decision (`task-02/callsite-analysis.md#feature-4-tty-and-job-control`); `tcsetattr` and raw mode are comparison
  programs only (`task-02/libc-surface.csv`, oksh and mrsh).
- POSIX and Linux: `ISIG` makes INTR, QUIT and SUSP send `SIGINT`, `SIGQUIT`, `SIGTSTP` to the foreground group;
  only the foreground group may read, a background reader gets `SIGTTIN`; `TOSTOP` sends `SIGTTOU` to a background
  writer; `tcsetpgrp` from a background process sends its group `SIGTTOU` ([P8], [P9], [P10],
  `stage-6/sources.md#posix-semantics`).
- The kernel console: a 256-byte ring, `READABLE`, a `console_read` that never blocks and takes bytes from whichever
  holder of `READ` calls first; the shell's children get `WRITE` only (ADR-0060, `docs/design.md` "Known limitations").

**What is assumed.** The shape of the stage 5 terminal protocol is not on `main`; this ADR states what it must
provide, not how it provides it. Nothing about terminal latency is measured.

**Forces from TASK-01.** The kernel TTY layer and line disciplines are a large retrofitted layer (DM-069); process
groups, sessions and the controlling terminal were designed for one physical terminal (DM-070); escape sequences are
in-band and injectable (DM-071), `TIOCSTI` let a process inject input (DM-072); NT's console delivers Ctrl-C on a fresh
thread (DM-017); Plan 9's `rio` turns DEL into a note to the process named at window creation (DM-070). CP-25 asks
that terminal rights be separated and control be out of band natively, with a VT translation for POSIX.

## Decision drivers

1. **Ctrl-C must reach the whole foreground job at once**, from the terminal, without any process forwarding it
   (ADR-0004 driver 2, W9a).
2. **The data path is high volume and simple**: compiler and make output, a byte at a time or a line at a time,
   should be a stream write, not a round trip (CP-25 keeps data and control apart; the data side is bytes).
3. **The control path needs to know who asks**: `tcsetpgrp` is a shell's authority over the terminal, a background
   read must be refusable, the window size is a query (ADR-0004's mapping; CP-25).
4. **No terminal policy in the kernel** (ADR-0060, DM-069): echo, canonical lines, the signal characters and `TOSTOP`
   live in a user process.
5. **One service for every backend**: the serial console for `make test`, the stage 5 window for a person; dash sees
   one `/dev/tty`.
6. **Only the shell may change the foreground group** (CP-25, CP-02): a process without the broker's authority cannot
   steal the terminal.

## Considered options

### Option A: Line discipline and job-control signals in the kernel (Linux TTY)

- **Description:** The console object gains termios state, canonical editing, echo and the signal characters; `read`
  on it blocks; the kernel knows the foreground process group and sends `SIGINT`, `SIGTSTP`, `SIGTTIN`, `SIGTTOU`.
- **Evidence for:** Exactly POSIX; works while every user process is hung; no service to start first.
- **Evidence against:** DM-069 and DM-070 in the kernel; the kernel has no process groups (ADR-0002 rule 3) and no
  signals (ADR-0004); ADR-0060 decided against it with reasons that stand; a GUI terminal would need a second
  implementation anyway.
- **Cost:** *(estimate)* Thousands of lines in the kernel, twice (serial and GUI), permanently.
- **Precedent:** Unix.

### Option B: The terminal as a pair of byte streams only (ADR-0006 rule 1 read literally)

- **Description:** A terminal service turns raw input into an input stream (canonical lines, echo done by the
  service) and renders an output stream; `/dev/tty` is the two stream ends; the service signals the foreground job on
  INTR and QUIT; no control connection.
- **Evidence for:** Data path and wait points fall out of ADR-0101: `read` is a stream read, `pselect` on stdin is
  `READABLE`. Smallest protocol. The console of ADR-0060 and the stage 5 terminal need only produce bytes.
- **Evidence against:** The service cannot tell readers apart, so a background job's read steals input and `SIGTTIN`
  cannot exist (ADR-0004's mapping breaks); end of file (Ctrl-D) has no expression on a stream short of closing it;
  `TIOCGWINSZ`, `tcgetattr` and `tcsetpgrp` need a channel anyway, so the "only" is false for the shell.
- **Cost:** *(estimate)* Smallest service; a stated loss of `SIGTTIN`, `TOSTOP` and Ctrl-D.
- **Precedent:** ADR-0060's console (one reader, by design, with the same theft problem noted there).

### Option C: Output as a byte stream, input and control through a per-process connection (QNX `devc` shape)

- **Description:** A terminal service, `ttyd`, owns the line discipline and one backend (the kernel console at stage
  6; the stage 5 window through its channel). To the session it gives one output stream (write ends at fds 1 and 2),
  and a *terminal connection* per process over which `read` is a request answered with a line, `TIOCGWINSZ`,
  `tcgetattr`, `isatty`'s kind and a `clone` for children are requests, and through which the broker, on a privileged
  connection, sets the foreground group. INTR and QUIT are `job_signal` on the foreground job handle the broker gave
  it; SUSP is `job_suspend` plus a note to the broker. QNX's terminal is a resource manager serving POSIX through
  messages (`alternatives-postmortem.md#qnx`).
- **Evidence for:** Drivers 1 to 6. Reads are keyboard-paced, so a round trip per read costs nothing measurable;
  writes are a stream (driver 2). The service knows each reader's group (the parent declares the child's group when it
  clones the connection), so `NotForeground`, Ctrl-D as an empty reply and `TOSTOP` are expressible. Window size and
  attributes are queries on the same connection. The foreground group changes only through the broker's connection.
- **Evidence against:** Two mechanisms for one fd: a stream for writes and a connection for reads, so a tty
  description holds two handles. A `read` that waits is a pending request the service must hold per connection.
  `pselect` on a tty fd needs a readiness request (a later line editor's need; T1 has none). The parent's declaration
  of a child's group is trusted at stage 6.
- **Cost:** *(estimate)* A `zoraya-tty` crate (line discipline, host-tested, about 500 lines), the service with two
  backends (about 400 lines each), a protocol crate; `libposix`'s tty description kind.
- **Precedent:** QNX resource managers; Fuchsia's `pty` service `[outside evidence: fuchsia.hardware.pty, not fetched]`;
  Plan 9's `cons` file server with its `consctl` control file (ADR-0060 Option B).

### Option D: Everything through the connection, output too (Plan 9 `cons`, Fuchsia pty)

- **Description:** As C, but writes are requests as well: `write` on a tty fd is a message with the bytes.
- **Evidence for:** One mechanism; the service sees every write and can enforce `TOSTOP` without a stream.
- **Evidence against:** Output is the volume: a compile's diagnostics, make's recipe echo, `ls` output, one message
  per `write` through ADR-0050's copy path, where ADR-0101's stream is one copy with no reply. `TOSTOP` is off by
  default on Linux and no measured program depends on it.
- **Cost:** *(estimate)* Fewer handles per fd; two copies and a reply per write.
- **Precedent:** Plan 9 (whose `cons` is a 9P file); Fuchsia.

## Recommendation

**Recommendation (not a decision): Option C.**

**The service.**

1. **`ttyd`** is started by `init` before the broker (ADR-0104 item 12), in its own job, with the backend's handles:
   the console with `READ | WRITE | WAIT` for the serial backend (ADR-0060), or the stage 5 terminal's channel for the
   window backend. It creates the output stream (ADR-0101) and keeps its read end, renders what it reads to the
   backend, and keeps the raw input from the backend in a line buffer in `zoraya-tty`. It gives `init`, for the
   broker: the output stream's write end, a *terminal connection* for the session's first process, and a *control
   connection* for the broker.
2. **Line discipline** (`zoraya-tty`, host-tested): canonical mode with echo, ERASE (DEL and Ctrl-H), KILL (Ctrl-U),
   EOF (Ctrl-D), `ONLCR` on output; `ISIG` with INTR (Ctrl-C), QUIT (Ctrl-\), SUSP (Ctrl-Z); input flushed on a signal
   unless `NOFLSH`. Non-canonical mode exists in the state machine (for a later line editor) but nothing at T1 sets it.
   `TOSTOP` is off and not implemented at stage 6 (no measured use).
3. **Signals.** INTR: `job_signal(fg_job, Interrupt, 2)`; QUIT: `job_signal(fg_job, Quit, 3)`; SUSP:
   a `suspend(20)` message to the broker, which applies each foreground process's recorded disposition for `SIGTSTP`
   (ADR-0104 items 8a and 9: `job_suspend` of its process job, with the pid marked stopped so that a
   `waitpid(WUNTRACED)` in the shell returns, or `Custom(20)` to a handler, or nothing when ignored); the terminal
   itself calls `job_suspend` for nothing, since it cannot know the targets' dispositions; when every member's
   record is `Default` the broker suspends the group job in one call (ADR-0104 item 9). Hangup, when the backend closes:
   `job_signal(session_job, Hangup, 1)`, with the session job handle the broker gave it (ADR-0104 item 1: the session
   job holds only POSIX processes). Window size change:
   `job_signal(fg_job, Custom(28), 28)`, whose default disposition is `Ignore` (ADR-0004). With no foreground job set,
   the characters are data.
4. **The terminal connection** (`zoraya-proto-tty`): `read(max) → bytes` (an empty reply is end of file; `NotForeground`
   when the connection's group is not the foreground group and the foreground is set; the service holds at most one
   pending read per connection and answers it when a line completes); `poll_readable → ()` (answered when a read
   would not block, for a later `pselect`); `window_size → {rows, cols}`; `get_attr → termios subset`;
   `set_attr(subset)`; `clone(group pid, server_end)`: a new connection for a child, tagged with the group the caller
   declares. `isatty` and `ttyname` need no request: the description's kind says so and the name is `/dev/tty`.
5. **The control connection** (broker only): `foreground(group pid, job handle)` sets the foreground group and the
   handle the signals go to, with the rights ADR-0104 item 10 gives it (`SIGNAL | KILL`); `session(job handle)` for
   hangup; `clear_foreground`; and, from `ttyd` to the broker, `suspend(sig)` for the SUSP character (item 3).
   `job_suspend` and `job_resume` need the `KILL` right on the job, by the architect's decision (ADR-0040's `RESUME`
   is a process right, for `process_resume`; a job handle carries none); ADR-0040's table gains that use of `KILL`.

**`libposix`.**

6. **The tty description** (ADR-0103) holds the output stream's write end and the terminal connection. `write` is a
   stream write; `read` sends `read`, then waits on the connection's `READABLE` at a wait point (ADR-0106), and maps
   `NotForeground` as item 8 says. fds 0, 1 and 2 of the shell are one description; `open("/dev/tty")` makes another
   from the namespace entry (ADR-0107) by `clone` of the process's own connection.
7. **Job control calls.** `tcsetpgrp(fd, pgid)` → broker `tcsetpgrp(pgid)` (ADR-0104 item 10): any process of the
   session may make a group of the session the foreground group, which is what dash needs twice per foreground job,
   in `forkchild` from the child for its own new group and in `waitforjob` from the shell while the job's group is
   still foreground ([D1]; both run with signals blocked, which on Linux is what lets a background caller through,
   [P10]); `pgid` outside the session is `EPERM`. The `SIGTTOU` Linux would send a background caller with `SIGTTOU`
   unblocked is not emulated (item 8). The broker sends `foreground` to `ttyd`. `tcgetpgrp` → broker lookup; `TIOCSPGRP`/`TIOCGPGRP` through `ioctl` are the same two.
   `TIOCGWINSZ` → `window_size`. `tcgetattr` → `get_attr`. `setpgid` is ADR-0104's.
8. **`SIGTTIN`, `SIGTTOU`, Ctrl-Z, `fg`, `bg`.** Specified to POSIX, not acceptance tests (ADR-0005). A `read` answered
   `NotForeground`: `libposix` asks the broker to stop its group (`stop_self(21)`), which `job_suspend`s the process
   job and marks it stopped; when resumed, the read is retried. If `SIGTTIN` is ignored or blocked the read returns
   `EIO`, as POSIX says. `SIGTTOU` is sent nowhere (`TOSTOP` off; `tcsetpgrp` from the background succeeds, item 7).
   `kill(pid, SIGSTOP | SIGTSTP)` and `kill(pid, SIGCONT)` are the broker's by the target's recorded disposition
   (ADR-0104 items 8a and 9): `job_suspend` or `job_resume`, `Custom(n)` to a handler, nothing when ignored. The shell's `fg` is `tcsetpgrp` then
   `kill(-pgid, SIGCONT)`; `bg` the latter alone. `job_suspend(job)` and `job_resume(job)` (ADR-0004 rule 3, not yet
   built) stop and continue every thread in the job tree and assert `SUSPENDED` on the job and every job below it;
   this ADR fixes that meaning, since ADR-0046 left the calls unbuilt.
9. **The serial session.** `ttyd` over the console is what `make test` drives: the test runner's keys reach dash
   through the line discipline, and dash's output reaches the serial line through the output stream and `ONLCR`. The
   `zosh` line editor stays for the native shell; a native shell and a POSIX session do not share one console at once
   (ADR-0060: whoever reads first wins), so `init` runs one or the other.
10. **The stage 5 terminal.** It needs to give `ttyd` a channel carrying raw key bytes in, display bytes out, size
    changes and a close. Whatever protocol TASK-14 chose for `zosh`, a backend of `ttyd` adapts to it; the GUI terminal
    keeps no POSIX knowledge, which is TASK-14's requirement.

**Tensions with accepted ADRs.**

- *ADR-0006 rule 1* lists terminals as byte streams. Here the output side is a stream and the input side a
  connection, so the row is half true; ADR-0060's expectation that a terminal service "take the rule's form" is met
  for output. Accepting this ADR records that a terminal's input is a connection.
- *ADR-0004 rule 8*'s "the terminal never names processes" holds: it holds job handles the broker gave it, and group
  *pids* only as tags on connections, which it never acts on except to compare.
- *ADR-0004*'s `SIGTTIN` sentence is implemented as written, through `NotForeground`; the `SIGTTOU` Linux sends a
  background `tcsetpgrp` caller with the signal unblocked ([P10]) is not emulated: the call succeeds for any process
  of the session (item 7). dash exercises the background call on every foreground job, with the signal blocked, so
  the deviation changes nothing it sees.
- *ADR-0040*: its job row lists `KILL` for `job_kill` alone; `job_suspend` and `job_resume` now need it too.
- *ADR-0060*: the console's `READ` right goes to `ttyd` alone in a POSIX session; `zosh` does not run beside it.

### Effect of the vision

For target 2 the terminal is the one place a sandboxed program meets a person, and this shape keeps the authority
split: a sandbox's processes hold terminal connections and a stream, never the console or the foreground handle; only
the broker can change the foreground group. The parent-declared group tag is the one trust gap (stage 6, trusted
processes); target 2 would have the broker clone terminal connections instead. For target 3, several windows are
several `ttyd` instances with one backend each. Threads and networking: no effect.

### Deferred to a later stage

Non-canonical mode and `pselect` readiness on a tty (`poll_readable` is specified, unused); `TOSTOP`; `SIGTTOU`;
a pseudo-terminal pair for programs that run others under a terminal they create (nothing in T1 or T2 does).

## Consequences

- **Positive.** Ctrl-C reaches the job as one `job_signal`, with no forwarding and no kernel policy. Output is one
  copy. The foreground group is a capability held by the terminal and set only by the broker. One service, two
  backends; the GUI terminal stays POSIX-free.
- **Negative.** A tty fd holds two handles. A `read` is a round trip. Ctrl-Z needs `job_suspend`, which is new kernel
  work beyond the T1 acceptance tests. `SIGTTOU` is not emulated.
- **Follow-up.** `zoraya-tty`, `zoraya-proto-tty`, the two backends, `job_suspend`/`job_resume` with a right, the W5
  script as a test (ADR-0110), a W5 extension with Ctrl-Z as a measurement.

## Invariants

- **INV-0105-1.** The interrupt character reaches every process of the foreground job and no other process, as one
  `job_signal`. *Check:* an in-QEMU test with a foreground job of three processes and a background job of two, each
  counting `Interrupt` requests, after one Ctrl-C through `ttyd`'s backend.
- **INV-0105-2.** Only the broker's control connection can change the foreground group; a `foreground` message on a
  terminal connection is refused; and through the broker, `tcsetpgrp` from a child for its own new group and from the
  shell after a foreground job both succeed. *Check:* a host test of the service loop with both connection kinds, and
  an in-QEMU `libposix` test of dash's two calls (a child calling `tcsetpgrp(getpgrp())` after `setpgid`, then the
  parent calling `tcsetpgrp(getpgrp())` after `waitpid`).
- **INV-0105-3.** A `read` from a connection whose group is not the foreground group returns `NotForeground` and
  consumes no input; a `read` from the foreground group returns the next line. *Check:* host test of the service with
  two tagged connections.
- **INV-0105-4.** `ttyd` holds no terminal policy in the kernel: the kernel console stays ADR-0060's raw ring.
  *Check:* INV-0060-1 to -4 keep passing; `grep -rn` for `ICANON`, `ECHO`, `ISIG`, `VINTR`, `VQUIT`, `VSUSP`,
  `VERASE`, `VKILL`, `VEOF`, `ONLCR`, `TOSTOP`, `termios` and `winsize` finds nothing in `kernel/src`.
- **INV-0105-5.** `job_suspend` stops every thread in the job tree before it returns and asserts `SUSPENDED` on every
  job in it; `job_resume` clears it. *Check:* an in-QEMU test with a two-level job of counting processes, reading the
  counters before and after.

## Open questions

- Whether the stage 5 terminal protocol carries raw bytes or key events; the backend adapts either, with a keymap in
  the latter case.
- Ctrl-Z, `fg` and `bg` end to end: a W5 extension on the prototype (ADR-0110), since W5 never pressed Ctrl-Z.

## Revisit triggers

- A line editor or `less`-like program in scope: non-canonical mode and `poll_readable` become acceptance material.
- A measured program depending on `TOSTOP` or `SIGTTOU`.
- A need for pseudo-terminals.

## Architect's decision

Accepted as recommended, 2026-11-02, with the architect's six changes already in the text of ADR-0100 to ADR-0110: the 10 % threshold in ADR-0102, `KILL` for `job_suspend` and `job_resume`, `make test-posix-long`, `make fetch-ports`, `aarch64-unknown-zoraya` with the T2 cost of `config.sub`, INV-0004-6 until exception channels, and INV-0106-4. Decided by the architect; recorded by an agent at the architect's request.
