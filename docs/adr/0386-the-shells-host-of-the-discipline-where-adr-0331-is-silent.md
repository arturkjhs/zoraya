# ADR-0386: The shell's host of the discipline where ADR-0331 is silent

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0052, ADR-0096, ADR-0105, ADR-0331, ADR-0332, ADR-0384
- **Evidence:** CP-25, DM-072, stage-10/survey.md#the-terminal-as-built

## Context

ADR-0331 decides that `zosh` hosts `zoraya-tty` for its window. Building it (TASK-31 part 10c) met places the ADR
leaves open: how `term` is told to restore the screen and to give it back, how the shell knows a program set bracketed
paste, where the question of a held paste is shown while a program owns the screen, which requests the discipline
gains for `tcflush` and `tcdrain`, and what the discipline does when no line is in front.

**What is known.**

- `zoraya-term` has `restore` and `reinstate` (part 10a); nothing sends them. ADR-0331 item 9 says `Restore` is a
  message, not bytes.
- ADR-0332 item 8 wants the shell to bracket a paste "when the program set 2004", and the shell sees only the bytes
  the program writes on its way to `term`; ADR-0337's status line, where the question would go, is part 10f's.
- Output is hostile (DM-072's rule for input: nothing a program writes becomes input; the same holds for anything
  that decides what a paste does), and the person's confirmation must be of what they saw (CP-25).
- Today the shell cooks keys itself (`stage-10/survey.md#the-terminal-as-built`).

**What is assumed.** That a mode-2004 sequence in a program's output is a reliable sign of the mode, as `term` would
act on it.

## Decision drivers

1. Nothing a program writes may reach its own input, or decide what a paste does beyond choosing the brackets.
2. The shell adds no state to `term` beyond what `zoraya-term` already keeps.
3. A choice that the status line of ADR-0337 can replace later without changing the protocol.
4. Nobody reads keys that no line was in front for.

## Considered options

### Option A: the shell scans the output for mode 2004 and shows the question in the title; `Restore` and `Reinstate` are two messages

- **Description:** A small recognizer in `zoraya_shell::tty_host` reads `CSI ? ... 2004 h` and `l` in each POSIX
  program's output on its way to `term` and keeps one bit per line. `zoraya-proto-term` gains `Restore { keep }`
  (ordinal 6) and `Reinstate` (ordinal 7). A paste with a newline for a program without 2004 is held and the question
  `paste N lines? Enter sends, any other key drops` is sent as the window's title (out of band, ADR-0168); the title
  returns to the directory when it is answered or its line ends. The bit is believed only while the discipline is
  out of canonical mode (a reader in canonical mode, such as a POSIX shell at its prompt, cannot take the marks for
  what they are), and it is cleared when a `tcsetpgrp` chooses another group: a `cat` of a file holding the sequence
  inside `sh` must not turn the question off for `sh` (the review of 10c, finding 4). Dismissing the question with a
  key of several bytes (an arrow) takes the whole key.
- **Evidence for:** Drivers 1 to 4: the recognizer has no way to send anything, the title never comes from output, and
  the shell's whole state is a bit per line.
- **Evidence against:** The recognizer can disagree with `term` when a sequence sits inside a string that `term`
  swallows; the result is brackets or a question that are not needed, never input that the person did not paste.
  The title is small and a person may not look at it.
- **Cost:** *(estimate)* 120 lines and their tests, 30 in `term`.
- **Precedent:** Terminal multiplexers track the modes of their programs from the output.

### Option B: `term` tells the shell its modes (a message up) and shows the question itself

- **Description:** `term` sends the shell a `Modes` message on each change of mode 2004, and draws the question over
  the grid.
- **Evidence for:** The mode is `term`'s own, so it cannot disagree.
- **Evidence against:** A new message from `term` to the shell on every mode change; a drawing over the grid is
  ADR-0337's trusted UI, which is part 10f's.
- **Cost:** *(estimate)* 80 lines in `term` and the trusted drawing.
- **Precedent:** xterm answers `DECRQM` queries instead.

### Option C: every paste to a program is bracketed or refused (Linux console: pasted as typed)

- **Description:** Without 2004 a paste with a newline is refused with a line saying so; the Linux console types it as
  keys.
- **Evidence for:** No state, no question.
- **Evidence against:** Contradicts ADR-0332 item 8, which asks for the confirmation; or, typed as keys, runs what the
  person did not confirm.
- **Cost:** *(estimate)* None.
- **Precedent:** The Linux console, and terminals without bracketed paste.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **`Restore { keep }` and `Reinstate`** are `zoraya-proto-term` ordinals 6 and 7 (ADR-0052), sent by the shell:
   `Restore { keep: false }` when the line in front ends, `Restore { keep: true }` when it stops or goes to the
   background, `Reinstate` at `fg` of a stopped line, followed by one `SIGWINCH`. `term` keeps one `Restored`; a
   `Restore` replaces it and `Reinstate` takes it. `ttyd` ignores both.
2. **Lines are told apart from the start.** With no line in front the discipline refuses every read: keys typed then
   are the shell's, and the shell never feeds them to the discipline. A change of the line in front drops the unread
   input, so what was typed for one line is not read by the next.
3. **Bracketed paste** is read from the program's output as in Option A, per line, and reset when the line ends.
4. **A held paste** is one at a time; a new paste replaces it. Enter (`\r` or `\n`) sends it; any other key drops it
   and is not typed; the line ending, stopping or leaving the front drops it. Pasted newlines go to the program as
   carriage returns, as terminals send them. At most the discipline's pending bound (16 KiB) of a paste reaches a
   program in one go; the rest is dropped by the discipline and counted (`dropped`).
5. **Requests.** `zoraya-proto-tty` gains `Flush { queue }` (7; only the input queue, 0) and `Drain` (8, answered when
   the shell has shown what the program wrote), and the status `IO` (7) for a request from a line not in front
   (the client reports `EIO`).
6. **Time.** The discipline has no clock; the host gives it one (`set_time`) before each call, and asks `deadline` for
   the time the next held read needs.

**Tensions with accepted ADRs.** ADR-0332 item 8 says the question is on the status line (ADR-0337); until part 10f
the title carries it. ADR-0331 item 4 says reads, polls and mode changes from another line are refused: this adds
`Flush` to the three.

### Effect of the vision

Target 2: the recognizer reads the program's output and can only choose between two ways of delivering text the person
pasted; it cannot make input. Target 3: nothing here is per-machine.

## Consequences

- **Positive.** The paste path and the restore path are host-tested in `zoraya_shell::tty_host` before `zosh` calls
  them; `ttyd` and `zosh` share the discipline.
- **Negative.** The title is a weak place for a question; the recognizer and `term` can disagree; a paste past 16 KiB
  to a program is cut.
- **Follow-up.** ADR-0337's status line replaces the title; `libposix` maps `IO` to `EIO` and sends `Flush` and
  `Drain`; the wiring in `zosh`.

## Invariants

- **INV-0386-1.** A paste to a program contains no byte that a program could take for the end of the bracket or, with
  the default control characters, for a signal character (a program's own `c_cc` applies to a paste as to typed
  keys), and is bracketed exactly when the program set 2004 and is out of canonical mode. *Check:* `cargo test -p zoraya-shell` (the
  `tty_host` paste tests with hostile text).
- **INV-0386-2.** With no line in front the discipline answers no read, and a request from a line not in front changes
  nothing. *Check:* `cargo test -p zoraya-shell` and `cargo test -p zoraya-tty`.

## Open questions

- Whether `term` should report mode 2004 itself (Option B) once ADR-0337's drawing exists.
- Whether a paste past the discipline's pending bound should be refused whole instead of cut.

## Revisit triggers

- A program is found whose output sets 2004 in a way `term` ignores, or the reverse.
- ADR-0337's status line is built.

## Architect's decision
(Left empty for the architect.)
