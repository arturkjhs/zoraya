# ADR-0385: The terminal's mouse reports, wheel and focus, and one place that makes what is sent

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0052, ADR-0094, ADR-0096, ADR-0322, ADR-0332, ADR-0384
- **Evidence:** CP-25, DM-071, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal

## Context

ADR-0332 item 3 names mouse reporting 1000 and 1002 with the SGR encoding 1006 and no other, and focus events 1004,
and says that the only messages `term` sends toward the shell come from keys, the pointer, a paste or a size change
(INV-0332-2). It does not say how a wheel turn is reported, what a report holds besides the button and the place, what
Shift does, or how the "only these messages" rule is made checkable. TASK-31 part 10b built them.

**What is known.**

- `less` and Vim send the mouse modes from hard-coded strings, and read `\E[<` reports
  (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`); the entry's `kmous` is `\E[<` and `xm` the
  format, with a trailing `;` that is `terminfo`'s notation and that xterm's reports do not have
  (`stage-10/survey.md#terminfo-entries-compared`).
- The compositor sends `WindowPointerMods` (position, buttons, modifiers) to the window under the pointer after each
  frame, so intermediate positions between two frames are not seen, and `Wheel` in 120ths of a notch (ADR-0322); a
  release outside the window is not delivered to it.
- A report is made of numbers the person's gesture chose, so a program cannot choose a byte of it except through the
  modes it set (Option E of ADR-0332 was refused for replies that a program chose; these are not replies).

**What is assumed.** That a report on the person's pointer event, with the mode a program set, is not "output
producing input": the program asked to be told what the person did.

## Decision drivers

1. **No output produces input**: a report exists only for a gesture of the person (INV-0332-2, DM-071, CP-25).
2. **Programs read what xterm sends** (`kmous`, `xm`, SGR).
3. **A selection stays possible** while a program has the mouse (Shift).
4. **Bounded**: a fast wheel makes few reports.
5. **Checkable**: the places where `term` makes bytes to send are few and a test can enumerate them.

## Considered options

### Option A: SGR only, press and release, drag motion, wheel as buttons, Shift selects; one type makes everything sent

- **Description:**
  1. **Reports** are `CSI < b ; x ; y M` (`m` for a release), cells from 1. Buttons left 0, middle 1, right 2; Alt adds 8,
     Ctrl 16; motion (1002, a button held, the cell changed) adds 32 and names the lowest button held. Shift is never
     in a report: Shift+drag selects (ADR-0384). 1003 (motion with no button) is parsed and ignored. A release is
     reported only for a press that was.
  2. **The wheel.** While a program has 1000 or 1002 and Shift is not held, each notch is a press of button 64 (away
     from the person) or 65 (toward), at most 8 reports for one turn; otherwise the wheel moves the view (ADR-0322).
  3. **Focus.** `CSI I` and `CSI O` while 1004 is set, from `WindowFocus`.
  4. **One place.** `zoraya_term::Session` is the terminal as the window runs it; its gesture methods (`key`,
     `pointer`, `wheel`, `focus`, `paste`) are the only code that makes anything to send, into an outbox; `output`
     (what the shell wrote) cannot reach it, and `term` sends the outbox as `Input` (reports and keys) and `Paste`.
     Focus is the person's own gesture and joins ADR-0332's list.
  5. **Ordinals.** `Paste` is ordinal 5 of `zoraya-proto-term` (ADR-0052); `Restore` (ADR-0331) and `Status`
     (ADR-0337) take the next numbers in their parts.
- **Evidence for:** Drivers 1 to 5; INV-0332-2's second half is a host test of `Session`.
- **Evidence against:** A drag that leaves the window is not reported to its end; a fast drag between frames is a jump;
  a program that wants 1003 (hover) gets none.
- **Cost:** *(estimate)* 120 lines of reports, 150 of `Session`, 250 of tests.
- **Precedent:** xterm, foot and alacritty implement 1000, 1002 and 1006; `st` has no mouse reports.

### Option B: Every encoding xterm has (X10, UTF-8 1005, urxvt 1015)

- **Description:** Parse and send all of them as a program asks.
- **Evidence for:** Old programs.
- **Evidence against:** ADR-0332 item 3 says SGR only; the X10 form puts a coordinate in a byte that can be a control
  character.
- **Cost:** *(estimate)* 200 lines.
- **Precedent:** xterm.

### Option C: No wheel reports: the wheel always scrolls the view

- **Description:** As ADR-0322 stands.
- **Evidence for:** Nothing to add.
- **Evidence against:** `less` and Vim cannot scroll with a wheel when they have the mouse, and a program that took the
  mouse expects the wheel (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`).
- **Cost:** *(estimate)* None.
- **Precedent:** the Linux console without a mouse daemon.

## Recommendation

**Recommendation (not a decision): Option A.** The reports are the entry's own, `Session` makes the rule of
ADR-0332 item 3 a thing a test can assert (a hostile output is fed to it and the outbox stays empty), and Shift keeps
a selection possible when a program takes the mouse. It is the terminal xterm-aware programs expect, at the size of ADR-0332's list.

## Consequences

- **Positive.** `less` and Vim can use the mouse and the wheel; focus events work; "no output produces input" is a
  test of one type.
- **Negative.** Hover (1003) and the other encodings do not exist; a button released outside the window is reported at
  the next event.
- **Follow-up.** 10d tests `less` and `vi` with the mouse; the reports come to a POSIX program through 10c's
  discipline.

## Invariants

- **INV-0385-1.** A pointer event is reported only while a mode of the program asks, with the lowest-numbered
  button, at most 8 wheel reports to a turn, and never with Shift. *Check:* `cargo test -p zoraya-term`
  (`tests_session.rs`: `presses_releases_and_the_buttons_numbers`, `motion_is_reported_only_with_1002_and_a_button_down`,
  `the_wheel_is_reported_to_a_program_that_asked_and_scrolls_otherwise`).
- **INV-0385-2.** Output to the terminal never adds to what is to be sent. *Check:* `cargo test -p zoraya-term`
  (`output_never_produces_anything_to_send`, `what_is_sent_depends_on_gestures_and_modes_not_on_output_text`).

## Open questions

- Whether 1003 (hover) is wanted (ADR-0332's open question).
- Whether a drag that leaves the window should end at the window's edge.

## Revisit triggers

- A ported program that needs an encoding other than SGR.
- A compositor that tells a client of the pointer's leaving.

## Architect's decision
