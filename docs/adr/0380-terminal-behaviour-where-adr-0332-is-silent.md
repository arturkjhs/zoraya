# ADR-0380: The terminal's corners that ADR-0332 leaves open: the least state, and sequences that end where they are cut

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0096, ADR-0331, ADR-0332
- **Evidence:** DM-071, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#how-the-terminal-ecosystem-is-layered, stage-10/survey.md#what-a-small-terminal-implements

## Context

ADR-0332 names the sequences, the modes, the bounds and the colours. Writing the parser (TASK-31 part 10a) met about
twenty places where a sequence it accepts has an effect it does not describe, and each had to be decided to write the
code. They are small; they are listed here so that none is silent, as CLAUDE.md asks. Each is the simplest, least
surprising and most reversible choice, and each is covered by a host test of `zoraya-term`.

**What is known.** xterm's behaviour differs from VT100's in several of these corners, and the small terminals
(`st`, `foot`, `alacritty`) differ from xterm in others (`stage-10/survey.md#what-a-small-terminal-implements`). No
ported program of stage 10 depends on any of them. Output is hostile input (DM-071), so where a choice leaves the
terminal in a state a program cannot then undo, the other choice is preferred.

**What is assumed.** That none of the choices below is visible to a person in ordinary use; they matter to a program
that probes and to a test that compares two emulators.

## Decision drivers

1. **The parser stays total and bounded** (INV-0096-1, INV-0096-3), and a stuck state is escapable: a program that
   dies inside a sequence must not hold the screen.
2. **A well-formed sequence outside the lists changes nothing** (INV-0332-3), so every choice that has an effect is
   for a sequence the lists contain.
3. **The least state**: a mode or a buffer the terminal need not keep is not kept, so that `Restore` (ADR-0331 item 9)
   has little to undo.
4. **A recorded session replays the same** whatever its writer assumed of the terminal (INV-0332-6).

## Considered options

### Option A: Do what xterm does in each corner (the Linux desktop baseline)

- **Description:** Read xterm's source for each corner and copy it: lines leave the top of any region that starts at
  row 0 into the scrollback, `ESC c` clears it, a string abandoned by `ESC` is run, a 33rd parameter is dropped.
- **Evidence for:** Programs are written against it (`stage-10/survey.md#how-the-terminal-ecosystem-is-layered`).
- **Evidence against:** The set grows with xterm; several corners are xterm accidents that other terminals do not
  share; it leaves state the shell must restore (a scrollback holding what a program drew in a region).
- **Cost:** *(estimate)* The same code and a longer reading of xterm's source for each corner.
- **Precedent:** xterm, VTE.

### Option B: The least state, and a sequence ends where it is cut

- **Description:** The choices below.
  1. A line feed, vertical tab and form feed move down and keep the column; `NEL` also returns to column 0.
  2. Lines enter the scrollback only when the primary screen's whole height scrolls; a region's lines are lost, and the
     alternate screen has none. `CSI 2 J` keeps the scrollback, `CSI 3 J` clears it, `ESC c` keeps it (a person can
     still look at what was there).
  3. `DECSTBM` needs at least two rows and the bottom inside the screen, otherwise it is ignored; setting it homes the
     cursor. `IL` and `DL` outside the region do nothing and inside move the cursor to column 0.
  4. A wide character that does not fit in the last cell leaves it blank and wraps (with autowrap) or is dropped
     (without); in a grid one cell wide it takes the one cell. A code point that musl gives no width takes one.
  5. A sequence with more than 32 parameters is consumed and ignored whole; a `CSI` longer than 4,096 bytes is cut there
     and what follows is text again. A `DCS`, `APC`, `PM`, `SOS` or `OSC` is swallowed whole up to its end (`ST`, an
     `OSC`'s BEL, `CAN`, `SUB` or an `ESC` that starts something else): at most 4,096 bytes of it are kept, the rest is
     dropped, an `OSC` that overran is not run, and nothing of it is printed (a sixel or graphics payload is tens of
     KiB). An unterminated string therefore holds the parser until `CAN`, `SUB`, `ESC` or `Restore`.
  6. `ESC` inside a string ends it: `ESC \` ends it properly (an `OSC` runs), any other byte abandons it and is read as
     the byte after an `ESC`. `CAN` and `SUB` end any sequence. A byte of UTF-8 inside a sequence ends it and is text.
  7. `SGR 21` is a single underline; `4:N` is an underline whatever `N` but 0; `58` (the underline colour) is
     consumed and not drawn; `5` and `6` are blink; `26` and the unknown codes do nothing. An extended colour with a
     bad value is consumed and changes no colour.
  8. Reverse video is an attribute of the cell, swapped when it is drawn; the screen's reverse mode (5) swaps the
     whole screen when it is drawn and stores nothing. Erased cells take the pen's background, never its attributes
     (`bce`).
  9. `REP` repeats the last printed character, its work bounded by a screenful: past a screen and a row the screen, the
     cursor and the modes are those of the full repeat (only `n` modulo the period matters) and only the scrollback gets
     fewer lines, so that `CSI 65535 b` costs a screenful and not 800 rows (measured: 84 ms of CPU per KiB of sequences
     otherwise); `ESC 7` and `ESC 8` are one slot a screen, 1048 and
     1049 a third; `RIS` clears both screens and every mode.
  10. `Restore` clears the modes, the region, the sets, the pen, the palette and the shape and leaves the alternate
      screen (restoring the cursor if 1049 had saved it); `reinstate` gives the modes back and enters a blank alternate
      screen if it was on one.
- **Evidence for:** Drivers 1 to 4; each choice keeps less state than xterm's or the same.
- **Evidence against:** A program that relies on xterm's scrollback of a region (rare: `less` uses the alternate
  screen) finds none.
- **Cost:** *(estimate)* None beyond the code and its tests, which exist.
- **Precedent:** `st` and `foot` keep to a similar subset and document it.

### Option C: Refuse what is not named

- **Description:** Any sequence whose effect ADR-0332 does not describe is ignored, as if unknown: no `REP`, no
  insert mode, no region.
- **Evidence for:** The smallest set; driver 3 at its strongest.
- **Evidence against:** `REP`, regions and insert mode are in the pinned entry (`rep`, `csr`, `smir`); INV-0332-1
  needs them.
- **Cost:** *(estimate)* None, and the entry's capabilities fail their tests.
- **Precedent:** The Linux console, which has no `REP` or insert mode in its entry.

## Recommendation

**Recommendation (not a decision): Option B.** Plan 9's `rio` had no sequences at all and so no corners
(`alternatives-postmortem.md#plan-9--inferno`); B keeps the corners few and writes them down. Where a choice turns
out to matter to a ported program, ADR-0332's revisit trigger names it and this ADR is superseded in that line only.

## Consequences

- **Positive.** Every corner is a test; a second emulator's differences on a recording are readable against this
  list.
- **Negative.** A few programs that rely on xterm's scrollback or its `RIS` find less. The list must be kept when the
  parser changes.
- **Follow-up.** `docs/design.md` carries the same list as limits; part 10b's keys and paste add their own corners.

## Invariants

- **INV-0380-1.** Each numbered choice above has a host test of `zoraya-term` that fails when the choice is reversed.
  *Check:* `cargo test -p zoraya-term` (`tests_parser.rs`, `tests_total.rs`); a mutation run of the crate with no
  surviving mutant in `parse.rs` and `screen.rs` that the tests could not tell apart.

## Open questions

- Whether a region that starts at row 0 should feed the scrollback after all (xterm does; the alternate screen has
  none, so `vi` and `less` are not affected).
- Whether `RIS` should clear the scrollback.

## Revisit triggers

- A ported program breaks on one of the choices.
- A second emulator is adopted as the reference and differs on a corner.

## Architect's decision
