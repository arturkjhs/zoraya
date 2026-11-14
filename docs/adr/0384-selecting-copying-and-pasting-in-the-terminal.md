# ADR-0384: Selecting, copying and pasting in the terminal where ADR-0332 item 8 is silent

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0052, ADR-0096, ADR-0154, ADR-0170, ADR-0332
- **Evidence:** CP-25, DM-071, desktop/survey.md#spoofing-and-the-trusted-path, stage-10/survey.md#what-a-small-terminal-implements

## Context

ADR-0332 item 8 says a drag selects text and System+C copies it, that an invisible cell is copied as a space, that
System+V pastes through `Paste` messages (a new ordinal, flow-controlled, at most 64 KiB `[assumption]`), that the
shell inserts a paste into its own line without running it, and that control characters are removed. ADR-0170 makes the
terminal a client of the clipboard. Building it (TASK-31 part 10b) met these places the ADRs leave open.

**What is known.**

- The compositor delivers `Copy` and `Paste` events to the focused window and takes `Offer` for 500 ms after a `Copy`
  (ADR-0170); the clipboard is at most 32 KiB (`MAX_CLIPBOARD`), so a paste is at most that; the 64 KiB is a bound on
  what `term` will send, which a larger clipboard (a later ADR-0170) would meet.
- The grid has no soft-wrap flag: a long line that wrapped is two rows.
- Output is hostile (DM-071); a paste and a copy are the person's gestures, and what they carry must be what the
  person saw and confirmed (CP-25).
- The shell's line editor has continuation lines (`...> `) for an open quote or block, and a limit of 4,096 characters
  and 256 lines (ADR-0154).

**What is assumed.** That a person pastes one or a few lines of a command or a path, rarely a file.

## Decision drivers

1. **What is copied is what was seen** (INV-0332-7): a selection that no longer reads as it did is not copied.
2. **A paste never runs anything** at the prompt, and never carries a control character, whatever the clipboard held
   (INV-0332-5, DM-071).
3. **Bounded** in memory and in messages (INV-0096-3; the channel's flow control).
4. **No new input path from output** (INV-0332-2): copying and pasting are gestures.
5. **Small**: word and line selection, primary selection and middle-click paste are stage 10 nice-to-haves.

## Considered options

### Option A: The simplest linear selection that ends when its cells change; a paste cleaned by the shell

- **Description:**
  1. **Selection.** A left drag selects the cells of the view between its two ends in reading order, inclusive. A press
     without a drag selects nothing. The selection ends when a row it covers is written, when the view scrolls
     (the wheel, Shift+Page), when the window is resized or a program resets the screen; so a copy is always of what
     is drawn highlighted. A row's trailing blank cells are not copied; a hidden cell is (as a space); a wide
     character is one character with its marks; a control character in a cell is a space. A row that wrapped is
     two lines (no wrap flag). While a program has the mouse (1000 or 1002), Shift+drag selects.
  2. **Copy.** `Copy` (and `Cut`, which the terminal treats as a copy) is answered with `Offer` of the selected text,
     at most `MAX_CLIPBOARD` bytes at a character boundary, only when something is selected: with nothing selected the
     clipboard stays as it was.
  3. **Paste.** `term` sends the compositor's text to the shell as `Paste { last, bytes }` messages (ordinal 5) of
     at most 4,096 bytes each, a whole number of characters, at most 64 KiB in all, in order with the keys around
     them; they wait in `term`'s queue as keys do when the shell's queue is full, and a paste that does not fit the
     queue is dropped whole.
  4. **The shell cleans and places.** `zosh` joins the parts, replaces invalid UTF-8 by U+FFFD, removes `ESC` and every
     control character (C0, DEL, C1) but tab and newline, makes `\r\n` and `\r` a newline, and puts the text in the key
     stream at the place it came in, as one of the bytes 0xF8 to 0xFF (never in UTF-8, so no key makes one).
  5. **At the prompt** the text goes into the line at the cursor: a tab is a space, a newline ends a line of the
     entry, which becomes a continuation line, and Enter, the person's, sends the entry. A paste past the editor's
     limits is cut, and the line says how many characters fit. While a program is in front, a paste is dropped with a
     line saying so (ADR-0332's paste to a program, with its confirmation, is part 10c's).
- **Evidence for:** Drivers 1 to 5; INV-0332-5 and INV-0332-7 are host tests.
- **Evidence against:** A selection is lost when output arrives on its rows (a person scrolling through a build
  cannot select what is moving); a wrapped line is copied as two; a paste is limited to what the editor holds.
- **Cost:** *(estimate)* As built: 250 lines in `zoraya-term`, 150 in the shell, 100 in `term`.
- **Precedent:** `st` keeps a linear selection and drops it when its text changes (`stage-10/survey.md#what-a-small-terminal-implements`); xterm keeps it through output.

### Option B: A selection anchored in the scrollback

- **Description:** Selection coordinates are absolute lines; a drag past the top or bottom scrolls and extends; the
  selection survives output.
- **Evidence for:** What xterm and VTE do; a selection of a long output.
- **Evidence against:** Driver 1: after output scrolled the text the highlight is on other text unless the ring's
  indices are tracked and invalidated on overwrite; the grid's ring and `resize` (no reflow) make that the largest part
  of the terminal's state.
- **Cost:** *(estimate)* 400 lines and a ring-index scheme.
- **Precedent:** xterm, VTE, alacritty.

### Option C: Paste as typing

- **Description:** The terminal sends a paste as `Input`, as keys.
- **Evidence for:** Nothing to add to the protocol; what a terminal without bracketed paste does.
- **Evidence against:** Driver 2 and ADR-0332 item 8 ("never as `Input`"): a pasted newline runs the line, and a
  pasted `ESC` types a sequence (DM-071 is the record of what output that types cost).
- **Cost:** *(estimate)* None.
- **Precedent:** the Linux console, and every terminal without bracketed paste.

## Recommendation

**Recommendation (not a decision): Option A.** It is the smallest design that keeps INV-0332-5 and INV-0332-7 true by
construction (the selection cannot outlive its text; the shell cleans, not the sender). Option B is the growth
path when a person needs to select across a scroll; it replaces item 1 only. `desktop/survey.md#spoofing-and-the-trusted-path` supports the shape: the paste is
delivered on the person's chord (ADR-0170) and the shell shows what it took. CP-25's rule that nothing a program prints
becomes input holds for what a program put in the clipboard, too.

## Consequences

- **Positive.** Select, copy and paste work in the terminal with no way to paste a control character or run a line
  by pasting; the clipboard stays unchanged by an empty selection.
- **Negative.** Output on a selected row ends the selection; a wrapped line is copied as two lines; no word or line
  selection; a paste to a program is dropped until 10c; in the session's `ttyd` a paste does nothing.
- **Follow-up.** 10c: the paste to a program (brackets when 2004 is set, the confirmation for a newline), and the
  same in `ttyd`.

## Invariants

- **INV-0384-1.** A selection never outlives a change to a cell it covers. *Check:* `cargo test -p zoraya-term`
  (`what_changes_under_a_selection_ends_it`).
- **INV-0384-2.** A paste is never `Input`: `Session::paste` produces only `Paste` parts, whatever the text and the
  modes. *Check:* `cargo test -p zoraya-term` (`a_paste_is_not_input_and_is_not_filtered_here`).
- **INV-0384-3.** No byte of `Input` is one of the bytes that stand for a paste in the shell's key stream. *Check:*
  `cargo test -p zoraya-shell` (`paste::tests`).

## Open questions

- Word and line selection (double and triple click), and a selection that scrolls (Option B).
- A flag per row for soft wraps, so that a long line is copied as one.
- Whether a paste to the prompt should show `paste N lines?` as ADR-0332 says for a program; at the prompt the
  continuation lines are the confirmation.

## Revisit triggers

- A person loses a selection often enough to ask for Option B.
- A clipboard larger than 32 KiB (ADR-0170's bound changes).

## Architect's decision
