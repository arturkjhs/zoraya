# ADR-0190: Process, terminal and temporary-service choices of stage 6d

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0014, ADR-0040, ADR-0042, ADR-0052, ADR-0055, ADR-0101, ADR-0102, ADR-0103, ADR-0104, ADR-0105, ADR-0106, ADR-0107, ADR-0108, ADR-0110, ADR-0142, ADR-0153
- **Evidence:** alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

TASK-21 built row 6d of the stage 6 plan: the broker (`crates/zoraya-broker`, `user/src/bin/broker.rs`), the
terminal service (`crates/zoraya-tty`, `user/src/bin/ttyd.rs`), the temporary directory service (`fsd --memory`),
the process half of `libposix` (`crates/zoraya-posix`'s `jobs`, `signals`, `wait` and `ffi/procs`), and `init`'s
session path. Writing and running the code met points where the accepted ADRs are silent, or say less than the code
must decide, or say something the code had to do differently. The task's rule: write a proposed ADR, implement the
least committing option, never fill a gap silently. ADR-0191 (the time base) and ADR-0192 (the snapshot's job tree)
are the two that needed an ADR of their own; the rest are here. Each item is implemented as its Recommendation says;
the tests that cover it are named under Invariants.

What is measured: the costs are in `docs/design.md`, "Processes and the terminal (as built)". No item is chosen on a
measurement.

The items:

1. **fd 0 has no slot.** ADR-0107 item 5 and ADR-0103 item 7 put each fd's handle "at the slot equal to its number".
   Slot 0 can hold no handle (ADR-0040: a handle's slot is 1 or more), and 6c's launcher only ever gave fd 0 as
   `/dev/null`. A terminal or a pipe on stdin needs a handle for fd 0.
2. **What a move lists.** A moved or duplicated handle lists the rights it carries, which the kernel checks against
   the rights held (ADR-0040, ADR-0050), and a move needs `TRANSFER`. No call reads a handle's rights, and the job
   handles of ADR-0104 items 2, 4 and 10 are narrowed: their holders must know their rights.
3. **The new image's broker connection.** ADR-0104 item 6's `exec_commit(process)` carries the new image's process.
   The new image needs a broker connection of its own at slot `BROKER`, and a channel end cannot be duplicated: the
   old image cannot hand on its own.
4. **FIFO type bits.** ADR-0107 item 13 makes a FIFO by `open_with_mode` with `posix_mode = S_IFIFO | ...`. ADR-0014's
   format stores 12 mode bits and the engine refuses more (`MODE_MASK`, a corrupt record otherwise).
5. **Opening a FIFO's side.** ADR-0107 item 14 has the open of a FIFO give "a duplicate of the read end" or "of the
   write end", hold a blocking open, and refuse a non-blocking writer with `ENXIO`; `open_with_mode`'s reply carries
   no handle and no such status.
6. **Who makes the terminal's channels.** ADR-0105 item 1 has `ttyd` create the output stream and give `init` the
   write end and two connections; `init` starts `ttyd` and the broker and would have to receive them first.
7. **The shell's stdio.** ADR-0103 item 13 counts the three stdio fds on the terminal as one description; the
   record of ADR-0107 has no "same description as fd N".
8. **`/etc/zoraya/session`'s form.** ADR-0104 item 12 and ADR-0110 item 1 say the file names the command and what
   the environment adds; neither gives its syntax.
9. **A terminal read and a signal.** ADR-0105 item 4 holds one read per connection; ADR-0106 item 6 returns `EINTR`
   from a blocking read interrupted by a handler without `SA_RESTART`. The held read is still the service's.
10. **Where handlers run.** ADR-0106 item 6 runs a handler "on the calling thread, at the wait point or the entry".
    The library's state is in use at a wait point.
11. **The start-up dispositions.** ADR-0106 item 1: "At startup `libposix` sets every slot from this table". A
    process starts with the kernel's default `Terminate` for every kind, so a `SIGCHLD` or `SIGWINCH` that came before
    the table was set would end it, and setting it costs a call per kind in every process.
12. **A FIFO fd across `spawn`.** ADR-0103 item 7 lists "a `File`, `Dir` or FIFO connection" as `share`d into the
    child. A FIFO description is a stream end with a connection; the service counts sides by the connections that
    opened them (item 5 above).
13. **`execl` on a stable toolchain.** `execl` is in the tier list and variadic; Rust's C variadics are unstable.
14. **The broker's mode with no first shell.** ADR-0153's note gives the broker a mode in which `init` gives the
    native shell a client connection and the temporary service's root; it does not say how they reach `init`.
15. **A spawn that cannot be finished.** A child spawned, whose bootstrap record then cannot be written, waits on its
    bootstrap channel for ever.
16. **Reasons in diagnostics.** C sees only an `errno` from `fork`, `posix_spawn` and `exec`; the first runs needed
    the native status.
17. **An inherited file's access.** ADR-0107 item 3's record gives each fd's kind and `O_NONBLOCK`; a file's access
    mode lives in the spawner's description, and a share asks for rights no wider than the connection has (ADR-0103
    item 1). A child that took every inherited file as read and write could not hand its write-only stdout on (W9b's
    `make > file &` failed so).
18. **The mask across `exec` and `posix_spawn`.** POSIX keeps the signal mask across `execve` and gives a spawned
    child `POSIX_SPAWN_SETSIGMASK`'s or the parent's; the record of ADR-0107 has no mask.
19. **`SIGCHLD` for a stopped child.** ADR-0106 item 3 subscribes a child's job for `SUSPENDED` too when
    `SA_NOCLDSTOP` is clear.

## Decision drivers

1. Least change to accepted interfaces: ADR-0001's rule, and TASK-21's "never fill a gap silently".
2. No new kernel call where user space can do it (ADR-0002 rule 3).
3. dash and GNU make as measured (ADR-0110): every item is on a path `make test-posix` runs.

## Considered options

### Option A: Each gap as the nearest accepted text reads, with the least new surface (the recommendation)

- **Description:** The items as listed under Recommendation.
- **Evidence for:** Drivers 1 to 3: one new fixed slot, two new file-protocol ordinals already numbered by
  ADR-0052's rule, a second handle on `exec_commit`, no kernel change but ADR-0192's.
- **Evidence against:** Several are stated deviations (items 4, 7, 12).
- **Cost:** what is built.
- **Precedent:** ADR-0130 and ADR-0180 did the same for 6a and 6c.

### Option B: What Linux does

- **Description:** Linux has fd 0 in the kernel's table, a kernel FIFO, a kernel tty with its line discipline, and
  `sigaction` in the kernel.
- **Evidence for:** No gap at all.
- **Evidence against:** Every one of these is what ADR-0002 to ADR-0006 chose not to put in the kernel.
- **Cost:** the rejected designs.
- **Precedent:** Linux.

### Option C: A kernel call for each gap

- **Description:** `handle_rights` to read a handle's rights (item 2), a duplicable channel end (item 3), wider mode
  bits in the format (item 4).
- **Evidence for:** Fewer conventions in user space.
- **Evidence against:** Each is a kernel or format change with a security or compatibility question of its own
  (ADR-0040's rights only decrease and are never shown; ADR-0050's one handle per end); driver 2.
- **Cost:** three ADRs of their own.
- **Precedent:** Fuchsia's `zx_object_get_info(ZX_INFO_HANDLE_BASIC)` reads rights
  (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

## Recommendation

### Decision

**Option A, item by item, except item 18** (the architect's decision, 2026-11-04). Item 18 is replaced: **the signal
mask and every `SIG_IGN` disposition are carried across `exec`, as POSIX requires** (`nohup`; dash ignores `SIGINT`
and `SIGQUIT` for the asynchronous lists of a non-interactive shell). The bootstrap record's header grows to 72
bytes: `blocked` (u32, bit n for signal n, as `ignored`) and a reserved zero word follow `realtime_base`. `exec`
writes the current mask and the ignored set; `posix_spawn` writes `POSIX_SPAWN_SETSIGMASK`'s mask, else the
parent's, and the ignored set as item 11 says; the new image starts with that mask (`SIGKILL` and `SIGSTOP` never in
it). Blocking stays ADR-0106 item 2's: a request taken while blocked waits in the library's pending set, and a
blocked signal whose disposition is `SIG_DFL` and whose default terminates still ends the process at once (the kernel
disposition does not change on block). INV-0190-5 states it.

The original recommendation follows.

**Recommendation (not a decision): Option A**, item by item:

1. **fd 0's handle is at slot `FD0` (1005)**, a fixed slot above `OPEN_MAX` with the others
   (`zoraya_abi::posix::slot::FD0`, `slot::of_fd`); every other fd stays at its number, so `MAKEFLAGS`' `3,4` still
   names slots 3 and 4 (INV-0003-6).
2. **The job rights are named in `zoraya_abi::posix::job_rights`**: `TABLE` (the broker's), `PARENT`, `OWN` (at
   `PROCESS_JOB`) and `TERMINAL`. The broker lists them on its replies and moves; `libposix` lists `OWN` when it
   moves its process job into a child or a new image. A handle duplicated to be moved carries `TRANSFER` besides
   the rights the move lists. Every other handle the library moves is at its type's full rights (a process handle is
   duplicated so).
3. **`exec_commit` carries two handles:** the new image's process and the server end of a channel whose client end
   the new image holds at `BROKER`. The broker serves the new end as the pid's connection and stops answering the
   old one (`NOT_PERMITTED`) until it closes with the old image.
4. **The temporary service keeps the FIFO bit itself.** `open_with_mode` with `S_IFIFO` on a service with FIFOs
   stores the 12 permission bits and remembers the object as a FIFO, for the life of the volume (which is the
   session's); its replies report `S_IFIFO` in the mode. A persistent volume's service refuses the type bits as
   before (`INVALID_ARGUMENT`), so a FIFO cannot be made there and `libposix`'s `EPERM` outside `/tmp` stands.
5. **`fifo_open` (file ordinal 34)** on the FIFO's own connection, after the `open_with_mode` that reported
   `S_IFIFO`: `side` and `nonblock`; the reply moves a stream end of that side. A non-blocking writer with no reader
   is `NOT_FOUND`, which `libposix` reads as `ENXIO`; a blocking open is held until the other side has a connection;
   a held open goes with its connection. The service holds its own end of each side while that side has a
   connection (ADR-0107 item 15). `time_base` is ordinal 35 (ADR-0191).
6. **`init` makes the output stream and the two channels**, and hands `ttyd` the read end and the server ends and the
   broker the write end and the client ends. The authority is ADR-0105's; only who calls `stream_create` changes.
7. **The shell's fds 0, 1 and 2 are three terminal descriptions**, each a duplicate of the output stream's write end
   and a terminal connection of its own (the broker clones three). No terminal state lives in a description that
   another fd would need to share (the position is the service's, `O_NONBLOCK` is per process at stage 6 by ADR-0103
   item 4), so the deviation shows only as three connections instead of one.
8. **The session file** is lines: `#` comments, `command WORD...` (one at most; words split at spaces), and
   `env NAME=VALUE` (the value to the end of the line, or between double quotes when it starts with one, for a value
   with a space at its end). Without a `command` it is `sh -i`; the program is `/bin/` and the first word unless the
   word has a `/`. The environment is ADR-0104 item 12's four, then the file's, a name given again replacing the
   base's (`zoraya_broker::session`).
9. **A terminal read interrupted by a signal stays outstanding:** the next `read` on that connection takes its
   answer rather than sending another, so no line is lost and `BUSY` never comes back. A held read of a group moved
   to the background is answered `NotForeground`, as a new read would be.
10. **Handlers run between calls into the state:** a wait point returns `EINTR` out of the state, the C boundary takes
    the requests and runs the handlers, and then retries (all `SA_RESTART`) or returns `EINTR`. A handler that does
    not return leaves the call abandoned with nothing of it held but item 9's read.
11. **The spawner sets the start-up dispositions** in `spawn`'s attribute block, from the child's ignored set: ignored
    stays `Ignore`, a default-terminating signal is `Terminate`, the others `Ignore`. A `fork` child has its
    parent's. `libposix` makes no disposition call at start-up.
12. **A FIFO fd is inherited by its stream end alone**, as a pipe's; the child's side is not counted by the service,
    so a non-blocking writer that only a child reads for would see `ENXIO` (no measured program does that). `fork`
    shares the FIFO's connection too.
13. **`execl` takes up to six arguments after `arg0`**: on AAPCS64 a variadic call passes its first eight arguments in
    the registers a fixed one uses, so they are read as parameters; a longer list is `E2BIG`.
14. **In the mode with no first shell** (`broker --no-shell`), the broker writes one message on the channel at its
    slot 9: `ZPBC` with the client connection and a connection to the temporary service's root.
15. **A process that finds its bootstrap channel closed with no record exits 127**, as a shell reports a command it
    could not start.
16. **`ZORAYA_POSIX_DEBUG`** in a process's environment makes `libposix` write why a `fork`, `posix_spawn` or `exec`
    failed to fd 2.
17. **The record's fd flags gain `FD_READ` (2) and `FD_WRITE` (4)**: the spawner sets the description's access; a
    record with neither, as stage 6c's launcher writes, means read and write as before.
18. *(Replaced by the Decision above: the mask is carried.)* **Not carried at stage 6:** every new image starts with
    an empty mask, and `POSIX_SPAWN_SETSIGMASK` is accepted and not applied. No measured program execs with a signal blocked that it then relies on staying blocked (make's
    blocked `SIGCHLD` is its own, in the parent). A record field is the fix when one does.
19. **Not delivered at stage 6:** `SIGCHLD` comes for a child's end only; a stop is seen by `waitpid(WUNTRACED)`.
    dash waits for stops with `waitpid`, and no measured program has a `SIGCHLD` handler that counts stops.

## Consequences

- Every item is exercised by `make test-posix`: the shell on the console, its pipelines, `make`'s jobserver FIFO,
  `exec` of scripts, signals at wait points.
- Items 4, 7 and 12 are stated deviations from ADR-0107 item 13, ADR-0103 item 13 and ADR-0103 item 7; item 19 from
  POSIX and ADR-0106 item 3. Item 18, as decided, is not a deviation; the record of ADR-0107 gains its `blocked`
  word.
- Item 2's convention ends when a call reads a handle's rights (Option C).

## Invariants

- **INV-0190-1.** A spawned child's fd 0 is the handle at slot `FD0` and every other fd the one at its number.
  *Check:* `make test-posix`'s `session.script` (pipes into stdin) and `invariants.script` (`t_posix cloexec`).
- **INV-0190-2.** After `exec_commit` the pid's requests are answered on the new connection and refused on the old.
  *Check:* `zoraya-broker`'s host test `exec_keeps_the_pid_and_the_reap_gives_the_new_images_record`.
- **INV-0190-3.** A FIFO made on the temporary service reports `S_IFIFO`, and a persistent volume's service refuses
  the type bits. *Check:* `zoraya-fs-service`'s host test `fifo_open_is_refused_where_it_has_no_meaning`.
- **INV-0190-4.** A terminal read interrupted by a handler loses no input: the next read takes the held line. *Check:*
  `zoraya-tty`'s host test `a_second_read_while_one_is_held_is_busy` and `make test-posix`'s Ctrl-C steps.
- **INV-0190-5.** A new image after `exec`, or a child of `posix_spawn`, starts with the mask the caller had (or
  `POSIX_SPAWN_SETSIGMASK`'s) and with every signal the caller ignored still `SIG_IGN`: a child exec'd with `SIGINT`
  ignored survives `job_signal` of `Interrupt`, and a signal blocked before `exec` stays blocked after it. *Check:*
  `make test-posix`'s `invariants.script` (`t_posix execmask`, by `fork` and `exec` and by `posix_spawn`), and the
  host tests `the_raw_header_is_checked` (`zoraya-abi`) and `the_mask_on_entry_is_kept` (`zoraya-posix`).

## Open questions

- Item 2: a call that reads a handle's rights, or a convention in the `abi` crate for every narrowed handle.
- Item 7: one description for the three if a terminal ever has per-description state.
- Item 12: counting a spawned child's FIFO side, if a program is found that needs it.

## Revisit triggers

- A kernel call that reads rights (item 2); a format with type bits (item 4); a program that opens a FIFO with only
  a child as its reader (item 12).

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request. Changed from the
recommendation: item 18 is replaced, the mask and every `SIG_IGN` disposition carried across `exec` as POSIX requires
(the `### Decision` above), with INV-0190-5 and its tests.
