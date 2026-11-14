# ADR-0383: The terminal's keys where ADR-0332 item 7 is silent: Backspace stays DEL, the keypad, Alt, and a way to see a key's bytes

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0092, ADR-0096, ADR-0154, ADR-0163, ADR-0332, ADR-0333
- **Evidence:** alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal

## Context

ADR-0332 item 7 lists the keys the terminal sends beyond ADR-0096's and says each is the entry's `k*` string, so
that the key table is checked against the entry (INV-0332-1). Writing the table (TASK-31 part 10b) and checking it
against every `k*` capability of the pinned `xterm-256color` met six places where the entry and the ADRs disagree
or say nothing. Each had to be decided to write the code (CLAUDE.md: small implementation choices are not
decisions, but a key that a program reads is an ABI shape).

**What is known.**

- The pinned entry (ncurses 6.6, built without `--with-xterm-kbs`) says `kbs=^H`. ADR-0096, accepted, says Backspace
  sends `0x7f`, which is also the discipline's erase character in ADR-0331's termios defaults, what `zosh`'s editor
  and its cooked reader take, and what every Linux terminal sends. The entry's own comment says the choice of `^H` or
  `^?` is the configure option's (`stage-10/survey.md#terminfo-entries-compared`).
- The entry has the keypad keys as `SS3` letters (`ka1`, `kb2`, `kent`, `kpADD`, ...), which a terminal sends only
  in keypad application mode, and a centre key `kbeg`/`kp5` for a keypad with NumLock off. `zoraya-keymap` gives no
  text for keypad keys and has no NumLock.
- Ctrl+Space and Ctrl+@ are NUL, which `gui::Key` cannot carry as text (0 is "no text"). Ctrl+^ and Ctrl+_ are
  0x1e and 0x1f, on the keys that carry `^` and `_` on a US keyboard.
- The system key and Caps Lock are not modifiers of a key's bytes; F12 and Alt+Tab never reach a client (ADR-0160).
- ADR-0332's done-when asks for F-keys and Ctrl+arrows to be seen "in a test program". A POSIX raw-mode test program
  needs part 10c's terminal for programs; a native program reads cooked lines (`zosh` drops the sequences).

**What is assumed.** That a person who types Backspace in a ported program expects what Linux sends, and that
nothing in stage 10's list reads `kbs` to decide what Backspace means rather than the tty's erase character.

## Decision drivers

1. **The key table is the entry's** wherever the entry and an accepted ADR agree (ADR-0332 item 7; INV-0332-1).
2. **An accepted ADR is binding** where the entry disagrees (ADR-0096 on Backspace).
3. **No key makes a byte a program could mistake for output**: Alt is an `ESC` prefix, never the eighth bit
   (ADR-0332 item 4).
4. **A person can see what a key sends** without a program that reads raw (done-when of 10b).
5. **Layouts**: a chord means the same key in all four layouts (ADR-0163).

## Considered options

### Option A: The entry where it agrees; ADR-0096 where it does not; the rest from xterm

- **Description:** Every `k*` string the entry has is what the key sends, with the exceptions below, each in the
  conformance exceptions file with its reason.
  1. Backspace is `0x7f`, as ADR-0096 says (`kbs` is an exception; revisit below).
  2. The keypad sends its character (`0`-`9`, `.`, `+`, `-`, `*`, `/`, `,`, Enter as `\r`) until a program sets
     keypad application mode (`ESC =`, `smkx`), then `SS3` and the entry's letter; with Alt, an `ESC` before either.
     There is no NumLock: the keypad is always "on", so `kbeg` and `kp5` (the centre with NumLock off) are
     exceptions, and the centre key sends `kb2`'s `SS3 u`. `zoraya-keymap` names the keypad's codes
     (`code::KP0` ... `code::KP_COMMA`), gives them no text.
  3. Ctrl+Space and Ctrl+Shift+2 (Ctrl+@) are NUL, read from the key code in the terminal. Ctrl+Shift+6 and
     Ctrl+Shift+minus are 0x1e and 0x1f, given as text by the keymap on those physical keys in every layout (as
     `ctrl_base`'s fallback is physical), so Ctrl+^ and Ctrl+_ work on a Czech or Russian layout.
  4. Alt is an `ESC` before the key's bytes (a letter, Enter, Backspace, Tab, Escape); the right Alt of a layout with a
     third level is AltGr and is no Alt (ADR-0163), so `Alt Gr+q` on Czech is `\` and not `ESC q`. A modified cursor
     key, Home, End, Page Up and Down, Insert, Delete and F1 to F12 take xterm's parameter (`CSI 1 ; M A`; 1 plus Shift
     1, Alt 2, Ctrl 4), also while DECCKM is set; bare cursor keys, Home and End are `SS3` only while DECCKM is set.
  5. Shift alone with Page Up or Page Down moves the view (ADR-0332 item 7) and sends nothing, so the entry's `kPRV`
     and `kNXT` are exceptions; every other modified Page key is sent. Shift+Tab is `CSI Z`.
  6. Meta mode (`km`, `smm`, `rmm`, mode 1034) stays an exception for good: it would make Alt+x to Alt+DEL the bytes
     0xF8 to 0xFF, which the shell uses as the places of pastes in its key stream (ADR-0384 item 4); Alt is an `ESC`
     prefix only.
  7. F12 and Alt+Tab are the compositor's; the table has F12's bytes (`kf12`) and a client never receives the key.
  8. **To see a key's bytes**: at the prompt Ctrl+V shows the next key as it is sent, in the form `cat -v` prints
     (`^[[1;5C` for Ctrl+Right, `^[OP` for F1), put into the line as text; Ctrl+Left and Ctrl+Right (and the Alt forms)
     move by a word, Alt+b and Alt+f likewise, Alt+Backspace deletes a word. The test program of 10b's done-when is
     this; a raw-mode C program is part 10c's.
- **Evidence for:** Drivers 1 to 5. The checks of INV-0332-1 run over 256 of the entry's 287 capabilities.
- **Evidence against:** The entry says `^H` for Backspace, so a program that uses `kbs` to recognise the key and not
  the tty's erase character sees `0x7f` as an unknown key. Whether ncurses maps the tty's erase character to
  `KEY_BACKSPACE` as well was not checked against the pinned source and is not claimed; part 10d's curses test program
  must press Backspace (open question).
- **Cost:** *(estimate)* As built: 140 lines of table, 200 of tests; 30 of editor.
- **Precedent:** xterm, `st`, `foot` (all send `0x7f` and ship entries with `kbs=^?`).

### Option B: Backspace as `^H`, as the pinned entry says

- **Description:** Backspace sends `0x08`; ADR-0096's line changes by a superseding ADR, ADR-0331's termios defaults
  take VERASE `0x08`, and `zosh`'s editor and cooked reader already accept both.
- **Evidence for:** Driver 1 with no exception.
- **Evidence against:** Driver 2: ADR-0096 is accepted; every POSIX program built for Linux assumes DEL, and Ctrl+H
  (`^H`, 0x08) would be the same key as Backspace, so a program that binds Ctrl+H to help cannot be used.
- **Cost:** *(estimate)* An ADR to supersede ADR-0096's line, the discipline's default, and a change in each port's
  expectations.
- **Precedent:** the Linux console of the 1990s, and ncurses' default for `xterm` before 2007.

### Option C: Build the entry with `kbs=^?`

- **Description:** ADR-0333 pins the entry as ncurses 6.6 compiles it; a one-line change in the `tic` input
  (`xterm+kbs` with `kbs=^?`, the `--with-xterm-kbs=DEL` option) makes the entry say what the terminal sends.
- **Evidence for:** Drivers 1 and 2 together; the exception goes away. Debian, Fedora and macOS ship it so.
- **Evidence against:** ADR-0333 pins the entry as built, and a changed entry is its decision; the architect's.
- **Cost:** *(estimate)* One line in `tools/term-terminfo` and a new manifest line.
- **Precedent:** Linux distributions' `xterm-256color`.

## Recommendation

**Recommendation (not a decision): Option A now, Option C when ADR-0333 is next opened.** A keeps ADR-0096 and the
entry's `kbs` is the one named exception; C removes it at the source, and is the architect's. Plan 9's `rio` read
keys as runes with no sequences at all (`alternatives-postmortem.md#plan-9--inferno`); programs that read what a
terminal sends need the table, and the table is the entry's wherever the entry can be followed. Ctrl+V's display is
the smallest way to see the sequences without a raw-mode program; programs that read raw are 10c's, and
`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal` lists what they ask.

## Consequences

- **Positive.** Every key capability the terminal sends is checked against the entry; four layouts give the same
  chords; a person can see a key's bytes at the prompt.
- **Negative.** `kbs` and the NumLock-less centre are exceptions; a ported program that decides Backspace from `kbs`
  alone is confused until Option C.
- **Follow-up.** 10c's raw-mode test program shows the same sequences from a program; ADR-0333's open question gains
  `kbs`.

## Invariants

- **INV-0383-1.** For every key capability of the pinned entry that the terminal sends, the key table sends exactly the
  bytes the entry says; every other `k*` capability is an exception with its reason. *Check:* `cargo test -p
  zoraya-term` (`tests_keys.rs`) and `tools/term-conformance/check.sh` (the same as INV-0332-1's key half).
- **INV-0383-2.** Ctrl+^, Ctrl+_ and Ctrl+Space are the same bytes in all four layouts, and the right Alt of a layout
  with AltGr never makes an `ESC` prefix. *Check:* `cargo test -p zoraya-keymap` (`ctrl_shift_six_and_minus_are_control_codes_in_every_layout`,
  `ctrl_alt_and_altgr_across_the_four_layouts`).

## Open questions

- **Before part 10d:** the architect decides Option C (build the entry with `kbs=^?`) or accepts the exception, and
  10d's curses test program presses Backspace; a curses program with `keypad(TRUE)` that reads `kbs` would otherwise
  see `0x7f` as a plain character. This is listed in `docs/backlog.md`.
- Whether Backspace should be `^H` (Option B) if a ported program turns out to need `kbs`.
- A NumLock state for the keypad (the keymap would track it; the compositor sends no LED state).
- Ctrl+digit and Ctrl+punctuation other than `[ \ ] ^ _ @` send nothing; xterm's modifyOtherKeys is not implemented.

## Revisit triggers

- A ported program that reads `kbs` and cannot take `0x7f`.
- The pinned entry is updated (ADR-0333).

## Architect's decision
