# ADR-0387: `zosh`'s lines on the discipline where ADR-0331 is silent

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0105, ADR-0152, ADR-0158, ADR-0200, ADR-0331, ADR-0386
- **Evidence:** CP-25, DM-072, stage-10/survey.md#the-terminal-as-built

## Context

Wiring `zoraya_shell::tty_host` into `zosh` (TASK-31 part 10c) met places ADR-0331 and ADR-0386 leave open: which
of a line's streams are "the terminal" when a line mixes native and POSIX programs or runs in the background, who in
a pipeline may read before any `tcsetpgrp`, what the shell's kill offer (ADR-0158 rule 3) does to a program that
handles Ctrl-C itself, what replaces the stop-gap reset of part 10a for native programs, and how `Drain` knows that
output has been shown.

**What is known.**

- Every element of a `zosh` line is started by its own `SpawnPrepareIn`, so each POSIX element is a process group
  of its own (`user/src/posix_child.rs`); a Unix shell puts a pipeline in one group.
- The shell relays every stream itself (ADR-0152); the keys of a native program are cooked by the shell
  (`stage-10/survey.md#the-terminal-as-built`).
- Interactive `sh` ignores `SIGINT`; with ADR-0158's offer the second Ctrl-C a second after the first would kill it.
- Job numbers are reused (`jobs::Jobs::free_number`).

**What is assumed.** That a program's client end of a terminal connection does not leave its line's job other than
by a channel to another program, which the line's end closes from the shell's side.

## Decision drivers

1. ADR-0331 item 4 holds between lines without exception (DM-072, CP-25); inside a line nothing is promised beyond
   it.
2. `ls | less` and `cat x | sh` must work: the program at the end of a pipe reads keys.
3. The person always gets a working screen back after any line (ADR-0331 driver 5), not only after a POSIX one.
4. No new message or protocol for what the shell can do with what it holds.

## Considered options

### Option A: lines as the shell already has them, the whole line reads until a group is chosen

- **Description:**
  1. *Which fds are the terminal.* fd 0 when it would be the typed keys (first element, no input text, not in the
     background, not redirected); fd 1 when the line's output goes to the terminal and the element is the last; fd
     2 when its errors go to the terminal and are not piped. A background line keeps the empty input of ADR-0158
     rule 2 on fd 0 and has the terminal on fd 1 and 2, so a read from them or from `/dev/tty` is `NotForeground`
     (the program stops on `SIGTTIN`). The slot of a terminal fd holds the write end of a stream to the terminal
     (the line's own output, or a relay made for it); its connection is in the record, and `/dev/tty` is in the
     table with one more.
  2. *Mixed lines.* The line has the terminal when a POSIX element has it on any fd, unless the first element is
     native and reads the keys: the keys are then the shell's to cook for it, and a POSIX element of that line
     that reads the terminal waits.
  3. *Who reads.* Until a `tcsetpgrp` chooses a group, every program of the line in front reads
     (`Terminal::set_foreground_line`): the line is the job. After it, only the chosen group, until the line ends.
  4. *Ctrl-C.* With the terminal in front, the signal characters are the discipline's (ISIG and the attributes) and
     the shell offers no kill: the program decides what Ctrl-C does, as on Unix. The offer stays for lines without
     the terminal.
  5. *Every line in front is a line of the host.* A native line is started with no terminal: its end sends
     `Restore { keep: false }`, a stop `Restore { keep: true }` and `fg` `Reinstate`, but it is never sent
     `SIGWINCH`. This replaces `zoraya_shell::reset::BEFORE_PROMPT`.
  6. *Drain and the carriage return.* Before the shell takes any request of a line's connection, it relays what the
     line has written; `Drain` is then answered at once, and a `SetAttr` never changes the carriage returns of
     output written before it.
  7. *A line's connections end with it.* When a line ends the shell closes the server end of every connection made
     for it, so a connection kept somewhere cannot become one of a later line with the same number.
  8. *The newline after `^C` and `^Z`* is the shell's when it reports the line's end or stop; the echo is the
     discipline's.
- **Evidence for:** Drivers 1 to 4; the code follows what `zosh` already decides per line and per stream.
- **Evidence against:** Inside a line, a pipeline's elements are not told apart before `tcsetpgrp` (ADR-0331 item 4
  accepted this). A program that handles Ctrl-C and hangs can be ended only with `kill` from another window or by
  closing the window. Every line now ends with a `Restore`, also a native one that drew nothing.
- **Cost:** *(measured)* about 450 lines in `zosh`, 30 in `zoraya-tty`, 90 in `posix_child.rs`.
- **Precedent:** POSIX job control: a pipeline is one group, and the terminal is the group's.

### Option B: one process group per line

- **Description:** The broker makes a line's POSIX elements one group, as a Unix shell does, and the discipline's
  foreground is that group.
- **Evidence for:** The exact Unix picture; `tcsetpgrp` and the line's group coincide.
- **Evidence against:** A broker request that does not exist (`SpawnPrepareIn` with a group to join), and the Ctrl-Z
  path of ADR-0200 item 2 signals each element's group; neither is needed for drivers 1 to 4.
- **Cost:** *(estimate)* 150 lines in the broker and its tests.
- **Precedent:** Every Unix shell.

## Recommendation

**Recommendation (not a decision): Option A.** Option B can replace item 3 later without changing what a program
sees.

**Tensions with accepted ADRs.** ADR-0158 rule 3 (the kill offer) no longer applies to a line with the terminal.
ADR-0105 item 8 (a background read is `NotForeground`) now holds for `zosh`'s background lines too.

### Effect of the vision

Target 2: a program of another line, in front or not, reads nothing typed for the line in front; a connection does
not outlive its line. Target 3: nothing here is per-machine.

## Consequences

- **Positive.** `sh`, `less` and raw-mode programs get a terminal from the shell; every line gives the screen back.
- **Negative.** No kill offer for a hung program with the terminal; one more message per line.
- **Follow-up.** The broker's control channel (`tcsetpgrp` and `group_created`), `libposix`'s readiness request
  and `/dev/tty`, the in-QEMU checks of ADR-0331.

## Invariants

- **INV-0387-1.** Until a `tcsetpgrp` chooses a group, every program of the line in front may read and no program of
  another line may; after it, only the chosen group. *Check:* `cargo test -p zoraya-tty`
  (`the_whole_line_in_front_reads_until_a_group_is_chosen`) and `cargo test -p zoraya-shell`
  (`every_program_of_the_line_in_front_reads_until_tcsetpgrp_chooses`).
- **INV-0387-2.** A line in front sends `Restore` when it ends or stops, native or POSIX, and only a line with the
  terminal is sent `SIGWINCH`. *Check:* `cargo test -p zoraya-shell`
  (`a_native_line_is_restored_but_never_told_a_size`).

## Open questions

- Whether a reserved chord should end the program in front whatever it does with Ctrl-C (ADR-0337's key, or a
  second one).
- Whether a native first element that reads the keys should hand them to the discipline once a POSIX element of its
  line asks for them.

## Revisit triggers

- The broker gains a group for a whole line (Option B).
- A person cannot leave a program because it ignores Ctrl-C.

## Architect's decision
(Left empty for the architect.)
