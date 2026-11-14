# ADR-0338: Pseudo-terminals: none in stage 10; when one is needed, it is a pair of handles from the broker, with no name

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0104, ADR-0105, ADR-0107, ADR-0260, ADR-0331, ADR-0340
- **Evidence:** CP-01, CP-25, CP-27, DM-069, DM-070, DM-072, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#redox, stage-10/survey.md#a-pty-pair-as-capabilities, stage-10/survey.md#pty-security-history, stage-10/survey.md#ptys-without-a-global-name, stage-10/survey.md#the-name-devtty, stage-10/survey.md#the-tty-service-as-built, stage-10/survey.md#what-a-pty-is-on-unix, stage-10/survey.md#which-programs-need-a-pty

## Context

On Unix a program that wants to be a terminal for other programs opens `/dev/ptmx` and gets a pair: a master it
keeps and a slave its children use as their terminal (`stage-10/survey.md#what-a-pty-is-on-unix`). ADR-0105
deferred "a pseudo-terminal pair for programs that run others under a terminal they create (nothing in T1 or T2
does)" and named "a need for pseudo-terminals" as a revisit trigger. Stage 10 brings a terminal emulator's work
(ADR-0332), `vi`, `less`, git and ssh. This ADR decides whether any of them needs a pair now, and what a pair
would be on a system with no global names.

**What is known.**

- Only four kinds of program create a pair: terminal emulators, multiplexers (`tmux`, `screen`), remote-login
  servers (`sshd`) and recorders (`script`, `expect`, Python's `pty` module). `less`, git and the ssh client
  never do: they use the terminal they are given or open `/dev/tty`
  (`stage-10/survey.md#which-programs-need-a-pty`, `stage-10/survey.md#the-name-devtty`). The chosen `vi` has one
  such command: nvi2's `:script` runs a shell on a pair it makes with `openpty` (found by the review in nvi2's
  source); ADR-0335 removes that command.
- Zoraya's own terminal needs none: `term` speaks messages to the shell, and the shell or `ttyd` hosts the
  discipline (ADR-0331, `stage-10/survey.md#the-tty-service-as-built`).
- The master end sees everything the slave side writes and everything typed, injects input and, through the
  control characters, signals, and sets the size. The harm in the pair's history came from names and ambient
  access, not from the pair: the set-uid helper for `grantpt`, the shared `/dev/ptmx`, `TIOCSTI`, which Linux now
  restricts and OpenBSD removed (`stage-10/survey.md#a-pty-pair-as-capabilities`,
  `stage-10/survey.md#pty-security-history`).
- Other systems: Fuchsia's `ptysvc` gives each caller a fresh pair over a routed protocol and further clients by
  handle; Redox's `ptyd` keeps the discipline in a daemon and the group to signal in the pair's object, but names
  each slave by a sequential number that any holder of the scheme can open; Windows returns a handle from a call; Plan
  9 has no pairs and runs foreign programs in an emulator; Genode's libc fakes a fixed termios
  (`stage-10/survey.md#ptys-without-a-global-name`).

**What is assumed.** That `tmux`, an ssh server and Python's `pty` module are wanted after stage 10, not in it
(ADR-0340 decides the server).

## Decision drivers

1. **Build what a program in scope needs, and nothing ahead of it** (CP-27; ADR-0105's own rule for this item).
2. **No global name and no ambient access** (CP-01; `stage-10/survey.md#pty-security-history`).
3. **The discipline exists once** (DM-069; INV-0331-8).
4. **The holder of the master gets authority over its own children only**, and never a way to inject into another
   terminal (DM-072, CP-25, DM-070).
5. **The shape is decided before the first port that needs it**, so that port is not the place where it is
   invented.

## Considered options

### Option A: `/dev/ptmx` and `/dev/pts` (Linux)

- **Description:** A global device that makes pairs; slaves appear as named nodes that any holder of the name may
  open.
- **Evidence for:** Every program that wants a pair is written to it (`posix_openpt`, `ptsname`, then `open`).
- **Evidence against:** The name is the problem the history records: ownership helpers, per-mount instances for
  containers, path confusion across namespaces (`stage-10/survey.md#pty-security-history`). Zoraya has no global
  `/dev` to put it in (ADR-0107) and no kernel tty to back it (DM-069).
- **Cost:** *(estimate)* A device namespace and a kernel or service-side registry of names.
- **Precedent:** Linux, the BSDs.

### Option B: A pair is a `ttyd` the broker starts, returned as handles

- **Description:** `posix_openpt` asks the session's broker for a pair. The broker starts a `ttyd` whose backend is
  a channel instead of a window, keeps its control connection as it does for the session's terminal, and returns
  the master as the backend's handles and the slave as a terminal connection. Nothing is named; `ptsname` returns
  a name that only the creating process's table resolves.
- **Evidence for:** Drivers 2 to 4. `ttyd` with a channel backend is what already runs behind the terminal window
  in a session (`stage-10/survey.md#the-tty-service-as-built`), so the pair is existing code under a new starter.
  It is the shape of Fuchsia's service: a fresh pair per request and clients by handle; and it keeps what Redox's
  daemon does (the discipline and the group to signal in a user process) without Redox's numbered names
  (`stage-10/survey.md#ptys-without-a-global-name`; the systems are those of
  `alternatives-postmortem.md#fuchsia--zircon-incl-starnix` and `alternatives-postmortem.md#redox`).
- **Evidence against:** A process per pair, which a multiplexer with twenty panes pays twenty times. The broker
  gains a request and starts programs for its clients. Programs that pass the slave's *name* to an unrelated
  process do not work.
- **Cost:** *(estimate)* 300 lines in the broker, 300 in `libposix`, a backend variant of `ttyd`; a quota rule.
- **Precedent:** Fuchsia `ptysvc`; Redox `ptyd`; Windows ConPTY, where the terminal supplies the pipes and the
  system starts the host on the other end.

### Option C: The discipline inside the master's process

- **Description:** `libposix` runs `zoraya-tty` on a thread of the program that asked, which then serves terminal
  connections to its own children.
- **Evidence for:** No new process and no broker request; the authority is plainly the creator's.
- **Evidence against:** The foreground job's handle and the stop signals are the broker's to give (ADR-0104,
  INV-0105-2), so the broker is involved anyway. A discipline inside an untrusted program is a discipline that
  program can alter, which matters once its children are less trusted than it. A third host of the crate.
- **Cost:** *(estimate)* Similar to B, with the complexity inside every program that links it.
- **Precedent:** None surveyed.

### Option D: Never (Plan 9)

- **Description:** No pairs. Programs that would be terminals for others are written as native services of the
  window system.
- **Evidence for:** Nothing to build or to attack; `rio` gives each window its own console through the namespace
  (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** `tmux`, `script`, an ssh server and Python's `pty` cannot be ported at all (driver 5 would
  be answered with "no" for ever).
- **Cost:** *(estimate)* None; a closed door.
- **Precedent:** Plan 9, which needed `vt` for foreign programs.

## Recommendation

**Recommendation (not a decision): build nothing in stage 10, and record Option B as the shape.**

1. **Stage 10 has no pseudo-terminal.** No program in its list needs one once nvi2's `:script` is cut
   (ADR-0335 item 1). The pair's functions are not exported by `libc.a` and are on ADR-0342 item 12's guarded
   list, so a port that calls them fails at link time and names itself.
2. **`TIOCSTI` is never offered**, with or without pairs.
3. **When the first program in scope needs one**, the pair is Option B:
   - `posix_openpt` is a broker request. It returns the master: a channel carrying `zoraya-proto-term`'s messages
     (`Output` from the slave side, `Input` and `Size` to it) wrapped by `libposix` as one fd. Reading it gives what
     the children wrote, writing it is typing, and `TIOCSWINSZ` on it sets the size and signals the pair's
     foreground job.
   - The slave is a terminal connection (ADR-0105 item 4) that the creator passes to a child it starts, by the
     ordinary inheritance of fds. `ptsname` gives a path under `/dev/pts/` that resolves only in the creator's own
     table and its children's, to that connection.
   - The pair's `ttyd` runs in the creator's job and is charged to it (ADR-0260): a pair costs one of the job's
     processes. Closing the master ends it and hangs up the slave side.
   - Nothing about a pair is a grant on a line: it gives a program authority over the children it starts on it
     and over nothing else.
4. **That decision is taken again when it is used**: the ADR of the first port that needs a pair confirms or
   replaces item 3 with the port's measured calls.

**Examples.** In stage 10 a person sees no difference; the programs they run do not ask. A program built by hand
that does: `tcc rec.c -o rec` fails at the link with the compiler's own message that `posix_openpt` is not defined.
Later, with item 3 built, `script session.log` would show in
`grants script session.log` only the file it writes and the terminal: the pair it makes for the shell it runs is
its own.

**Tensions with accepted ADRs.**

- *ADR-0105's revisit trigger* "a need for pseudo-terminals" is examined and found not yet met.
- *ADR-0104*: item 3 would add a request to the broker and let it start a `ttyd` per pair; today it starts none.
  Recorded for the later ADR, not decided here.
- *ADR-0107*: `/dev/pts/N` would be a new entry kind private to a process tree.

### Effect of the vision

For target 2 the important part is what is absent: no name by which one program reaches another's terminal, and no
input injection. A supervisor that wants to watch an agent's terminal session would be exactly a holder of a
master, given by construction, not found by name. Target 3: `tmux` becomes possible later without a new kernel
object. Networking: an ssh server is the second likely user (ADR-0340). Threads: none.

## Consequences

- **Positive.** No work and no surface now; a recorded shape that reuses `ttyd` and adds no global name.
- **Negative.** `tmux`, `script`, `expect` and Python's `pty` module do not run in stage 10. The shape is
  untested until its first user.
- **Follow-up.** None in stage 10 beyond ADR-0342's exclusion list.

## Invariants

- **INV-0338-1.** `libc.a` exports none of the pseudo-terminal functions of ADR-0342 item 12, and no ioctl request
  injects input. *Check:* INV-0342-3's deny list, and a `libposix` host test that `ioctl(TIOCSTI)` on a terminal
  fd is `ENOTTY`.
- **INV-0338-2.** No path in any namespace table names a terminal other than the process's own `/dev/tty`.
  *Check:* a host test over the entry kinds of ADR-0107's encoding and the tables the shell and `libposix` build.

## Open questions

- Whether a process per pair is too dear for a multiplexer; measured when `tmux` is in scope, with Option C as
  the alternative.
- Whether Python's `pty` module and test suite matter for stage 9's CPython; they are listed as not available.

## Revisit triggers

- A program in scope needs a pair: `tmux`, `script`, an ssh server, Python's `pty`.
- A terminal that is not a window is wanted for a supervisor of agents.

## Architect's decision
