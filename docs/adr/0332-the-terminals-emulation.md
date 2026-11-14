# ADR-0332: What the terminal emulates: the sequences of the entry it claims and a short list beside it, and still no replies

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0096, ADR-0100, ADR-0154, ADR-0160, ADR-0161, ADR-0162, ADR-0163, ADR-0168, ADR-0170, ADR-0331, ADR-0333
- **Evidence:** CP-22, CP-25, DM-071, DM-072, DM-073, GR-015, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#how-the-terminal-ecosystem-is-layered, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks, stage-10/survey.md#programs-and-libraries-without-curses, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#testing-a-terminal, stage-10/survey.md#the-terminal-as-built, stage-10/survey.md#utf-8-and-character-width, stage-10/survey.md#what-a-small-terminal-implements, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal, stage-10/survey.md#which-term-to-claim

## Context

ADR-0096 gave the terminal a subset of the VT sequences that `zosh` and the stage 6 ports needed, and said that
"later programs (`vi`, `less`) will need scroll regions, insert and delete line, the alternate screen and cursor
visibility: each is a new sequence in the parser". Stage 10 is that later. The parser today knows nine `CSI` finals
and drops every private mode; every character is one cell; there are no function keys, no mouse, no paste; a
256-colour sequence sets wrong attributes; and no query is answered, by an invariant
(`stage-10/survey.md#the-terminal-as-built`). This ADR decides which sequences, colours and keys the terminal
supports, whether it answers anything, how wide a character is, and how conformance is tested. ADR-0333 decides the
name the terminal claims; the two are one design, because the claimed entry is what programs read.

**What is known.**

- Programs do not ask for "xterm"; they ask for terminfo names. The alternate screen, the scroll region, insert and
  delete of lines and characters, cursor visibility, application cursor keys and the SGR attributes are in every
  xterm-like entry. Mouse reports, bracketed paste, focus events and synchronized output are mostly outside
  terminfo: `less` and Vim send them from hard-coded strings
  (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`,
  `stage-10/survey.md#terminfo-entries-compared`).
- xterm's own document is 3,840 lines; the resolved `xterm-256color` entry has 297 capabilities, `st` 214, the Linux
  console 119. Small terminals implement a few hundred sequences and ignore the rest safely
  (`stage-10/survey.md#how-the-terminal-ecosystem-is-layered`, `stage-10/survey.md#what-a-small-terminal-implements`).
- Queries. Programs without curses (toybox, busybox `vi`, kilo) ask the terminal for the cursor position only when
  they cannot get the size another way, and `TIOCGWINSZ` is that other way. Vim's two queries (its version request
  and its position request) are optional. nvi, `less` and ncurses send no query whose answer they need
  (`stage-10/survey.md#programs-and-libraries-without-curses`,
  `stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`).
- Replies are the injection path: a title report put attacker text on the command line (DM-071). Deno's terminal
  prompt was spoofed twice with escape sequences (`stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks`).
- Program and terminal must agree on the width of every character, and no standard fixes it
  (`stage-10/survey.md#utf-8-and-character-width`).

**What is assumed.** That a person's day needs colour beyond eight and a mouse in `less` and `vi`; that the fonts'
coverage (no CJK, no emoji) is acceptable for stage 10. Neither is measured.

## Decision drivers

1. **The ports run unmodified** on the entry the terminal claims (DM-073; ADR-0333).
2. **Output is hostile input to the terminal**: the parser stays total and bounded (ADR-0096 driver 5, DM-071).
3. **No output becomes input** (DM-071, DM-072, CP-25; INV-0096-2).
4. **A defined, finite set** that a test can enumerate, not "what xterm does"
   (`stage-10/survey.md#how-the-terminal-ecosystem-is-layered`).
5. **Program and terminal count columns the same way** (`stage-10/survey.md#utf-8-and-character-width`, CP-22,
   GR-015).
6. **Checked without a person** in `make test`, and without the network (`stage-10/survey.md#testing-a-terminal`).

## Considered options

### Option A: Everything xterm documents (the Linux desktop baseline)

- **Description:** Follow xterm's control-sequence document: all private modes, window operations, every report,
  Tektronix and VT52 modes left aside.
- **Evidence for:** Whatever a program sends, it works; `vttest` and `esctest` assume this target
  (`stage-10/survey.md#testing-a-terminal`). The desktop terminals people know behave this way.
- **Evidence against:** The document lists several hundred modes and parameters
  (`stage-10/survey.md#how-the-terminal-ecosystem-is-layered`); xterm's own README, which st quotes as its reason
  to exist, counts more than 65,000 lines (`stage-10/survey.md#what-a-small-terminal-implements`). The reports
  include the ones DM-071 records as attacks. The set is not finite in practice: it grows with xterm.
- **Cost:** *(estimate)* Tens of thousands of lines, most for sequences no ported program sends.
- **Precedent:** xterm; VTE and others approximate it and differ in the corners.

### Option B: The capabilities of the claimed entry and a named list beside it, with no replies

- **Description:** The terminal implements the capabilities of the terminfo entry ADR-0333 pins, less a stated
  list of exceptions, plus a short list of sequences programs hard-code. It ignores every other well-formed
  sequence and answers no query.
- **Evidence for:** Drivers 1 to 6. The entry is a finite, versioned list, and it is what curses, `less` and
  Python's REPL read (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). foot, st and alacritty are
  this size and publish their lists (`stage-10/survey.md#what-a-small-terminal-implements`). No program in stage
  10's list needs an answer, and the size probes are answered by `TIOCGWINSZ` instead.
- **Evidence against:** Programs that assume xterm beyond its entry meet gaps, which Vim's documentation says of
  terminals that claim an xterm name (`stage-10/survey.md#which-term-to-claim`). A program that waits
  for an answer gets none: Vim's version and position requests time out, and a size probe fails where there is no
  window size. The entry itself names query strings the terminal will not honour.
- **Cost:** *(estimate)* 2,500 lines in `zoraya-term` and its tests, 400 in `term` (mouse, selection, paste, keys),
  a width table, a conformance tool.
- **Precedent:** foot (264 capabilities), alacritty (269), st (214), each with a documented list; each of them
  answers queries, which this option does not.

### Option C: A small VT100-class set (the Linux console's size)

- **Description:** Add only what `vi` and `less` need: cursor moves, erases, insert and delete line, a scroll
  region, reverse index, the alternate screen, cursor visibility; eight colours; no mouse, no paste mode. A small
  own entry describes it.
- **Evidence for:** The smallest parser. The Linux console shows that 119 capabilities and eight colours are a
  usable terminal (`stage-10/survey.md#terminfo-entries-compared`).
- **Evidence against:** `less` and Vim send mouse and paste sequences from hard-coded strings whatever the entry
  says, so the set must at least parse them (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). An
  own small entry exists on no other machine, which ADR-0333 weighs. 256 colours are what `less`, `git diff` and
  `ls` output assume today.
- **Cost:** *(estimate)* 800 lines; and a second round later.
- **Precedent:** The Linux console; `screen`'s entry (116 capabilities).

### Option D: No escape sequences: a typed display protocol (Plan 9's windows, the Windows console API)

- **Description:** Programs draw through typed requests (put text at, scroll, set attribute); nothing is in band.
- **Evidence for:** DM-071 disappears: there is nothing to inject and nothing to answer (CP-25's native half).
  Plan 9's windows have no cursor addressing at all (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Every program in stage 10's list writes VT sequences. Windows had exactly this API and
  built ConPTY to translate (DM-073). ADR-0096 Option C was not taken for the same reason.
- **Cost:** *(estimate)* A protocol, a translator for every port, and the translator's imperfections.
- **Precedent:** Windows before ConPTY; Plan 9, where foreign programs ran in `vt`.

### Option E: Option B, with replies made of numbers

- **Description:** As Option B, and the terminal answers the status, cursor-position, device-attribute and
  mode queries, whose replies are fixed text and decimal numbers, delivering them only to a program in front that
  reads in raw mode.
- **Evidence for:** It is what every xterm-like terminal does, and what the entry's query strings promise. Vim's
  detection and the size probes work. A reply of digits cannot carry a newline or a command word.
- **Evidence against:** The first draft of this ADR recommended it, and the review broke it. A reply is not tied to
  the program that asked and arrives later: a recipe run from inside `vi` (`:!make` in a hostile repository) prints
  position queries and exits, and the replies reach `vi` as keystrokes, where the letters `R`, `c`, `y` and `n` are
  commands and the digits are counts. The program does choose bytes of the reply: the cursor position it set, and
  the mode number a mode report repeats. Over ssh the replies go to the far shell's line. Binding each reply to
  its sender needs a tag on every write that the protocol does not have. And no program in scope needs it
  (driver 3).
- **Cost:** *(estimate)* Option B plus a per-write tag through `libposix`, the shell and `term`.
- **Precedent:** xterm and every terminal modelled on it; DM-071 is the record of what it cost them.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The rule.** `zoraya-term` implements every string capability of the pinned `xterm-256color` entry (ADR-0333)
   except those of item 4, the list of item 3 beside it, and nothing else. Any other well-formed sequence is
   consumed and ignored: it never prints, never answers and never changes state. The parser stays total; a `CSI`
   holds at most 32 parameters and sub-parameters, a string at most 4096 bytes (INV-0096-1 and INV-0096-3 stand).
2. **What the entry brings**, by group (the table of `stage-10/survey.md#terminfo-entries-compared` has each
   sequence):
   - C0: BS, HT, CR, LF, and VT and FF as LF. **LF moves down and keeps the column**, where today it also returns to
     column 0; the shell and the discipline write the carriage return (ADR-0331 item 8), so nothing a person sees
     changes. SO and SI select the line-drawing set.
   - Cursor: CUU, CUD, CUF, CUB, CNL, CPL, CHA, VPA, CUP, HVP, save and restore (`ESC 7`, `ESC 8`), tab stops
     (HTS, TBC, CHT, CBT).
   - Erase and edit: ED 0 to 3 (3 clears the scrollback), EL, ECH, ICH, DCH, IL, DL, REP, insert mode (IRM).
   - Scrolling: DECSTBM, SU, SD, IND, RI, NEL.
   - Modes: DECCKM (1), reverse video (5), origin (6), DECAWM (7), DECTCEM (25), the alternate screen (47, 1047,
     1048, 1049), keypad application mode, the soft reset. The alternate screen is a second grid with no
     scrollback.
   - SGR: 0 to 9, 21 to 29, 30 to 37, 39, 40 to 47, 49, 90 to 97, 100 to 107, and 38 and 48 with `5;N` (256 colours)
     and `2;R;G;B` (direct colour), in the semicolon and the colon forms. An extended colour's parameters are
     consumed as one unit, which ends today's misreading. Bold is a weight and no longer brightens the colour;
     blink is stored and not animated; italic is drawn slanted.
   - Character sets: G0 and G1 with `B` (ASCII) and `0` (DEC line drawing), which `acsc` needs.
   - `ESC c` (full reset).
3. **Beside the entry**, because programs send them from hard-coded strings:
   bracketed paste (2004); mouse reporting 1000 and 1002 with the SGR encoding 1006, and no other encoding; focus
   events (1004); synchronized output (2026), held for at most 150 ms `[assumption]` and then shown; cursor shape
   (`CSI Ps SP q`); `OSC 4` and `OSC 104` (set and reset palette entries, undone by `Restore`). Modes 12, 1003,
   1005, 1015, 1034 and 2027 are recognised and have no effect.
4. **What the entry names and the terminal does not do.** The review read the pinned entry and found capabilities
   the terminal will not honour; they are the first lines of the exceptions file of item 10, each with its reason:
   the query strings (`u6` to `u9`, the version and device-attribute requests `RV` and `XR`), because nothing is
   answered; the clipboard string `Ms` (`OSC 52`), because a program must not copy without the person's gesture
   (ADR-0170); the window title, because nothing a program prints sets it (ADR-0168) and the title stack inside
   `smcup` and `rmcup` is ignored with it; printing (`mc0`, `mc4`, `mc5`); memory lock (`meml`, `memu`); left and
   right margins (`smglp`, `smglr`, `smgrp`); the cursor colour (`Cs`, `Cr`); meta mode (`smm`, `rmm`): Alt always
   sends an `ESC` prefix; 132-column mode. Each is consumed and ignored. The list is what ADR-0333 weighs against
   the claim of the name.
5. **No replies.** The terminal answers no query: not the status report, not the cursor position, not device
   attributes, not a mode report, not titles, colours, the clipboard or answerback. INV-0096-2 stands as written.
   The size a probe would ask for is `TIOCGWINSZ`'s (ADR-0331 item 7). A program that cannot do without an answer
   is ADR-0096's revisit trigger: an ADR that names the program and ties each reply to the write that asked for it
   (Option E's defect).
6. **Colours.** Entries 0 to 15 come from ADR-0161's `term.*` tokens, in the light and the dark set; 16 to 255 are
   xterm's cube and grey ramp; direct colours are stored per cell. A cell grows to 16 bytes `[assumption]`.
7. **Keys.** Beyond ADR-0096's: F1 to F12; Alt as an `ESC` prefix; Shift+Tab as `CSI Z`; Ctrl+Space and Ctrl+@ as
   NUL, Ctrl+^ and Ctrl+_ as their control codes; Shift, Alt and Ctrl on arrows, Home, End, Page Up, Page Down and
   the function keys as xterm's modifier parameter (`CSI 1 ; M A`); arrows, Home and End in their `SS3` forms while
   DECCKM is set. Each is the entry's `k*` string, so the key table is checked against the entry (INV-0332-1).
   Chords with the System key never reach a client (INV-0160-3). Shift+Page Up and Page Down scroll the view.
8. **Mouse, selection and paste.** Without mouse reporting, a drag selects text and System+C copies it (ADR-0170's
   `Copy` and `Offer`); with reporting on, Shift+drag selects. A cell drawn invisible, or with the same foreground
   and background, is copied as a space, so that what is copied is what was seen. System+V pastes: `term` sends
   the text to the shell in `Paste` messages (a new ordinal, flow-controlled, at most 64 KiB `[assumption]`),
   never as `Input`. The shell inserts a paste into its own line without running it. For a program in front it
   passes the text through the discipline, between `CSI 200 ~` and `CSI 201 ~` when the program set 2004, with
   `ESC` and the other control characters except tab and newline removed either way, so that a paste cannot end
   the bracket or send a sequence. A paste that holds a newline, for a program that has not set 2004, is held: the
   shell shows `paste N lines?` on the status line (ADR-0337) and passes it on Enter, drops it on Escape.
9. **Width.** One table gives the width of every code point, and it is musl's own: the terminal's table is
   generated from the `wide.h` and `nonspacing.h` of the musl release pinned in `ports/sources.lock`, the data
   behind `libposix`'s `wcwidth`. So a code point is 0 wide where musl says nonspacing, 2 where it says wide, 1
   otherwise, in the grid, in every ported program, and in `zosh`'s own editor, which uses the same table. A
   width-0 character joins the cell before it (at most two per cell, more are dropped); a width-2 character takes
   two cells and wraps whole. A character with no glyph draws the hollow box at its width. No grapheme clustering,
   shaping or right-to-left layout. `libc.a` is not changed.
10. **The conformance test** has three layers, all without the network:
    - *The entry.* A host tool reads the pinned entry and checks that every capability has a test in
      `zoraya-term`'s host tests (output capabilities by their effect on the grid, key capabilities against the key
      table) or a line with a reason in the exceptions file; `make check` fails on a capability with neither.
    - *Recorded sessions.* Byte recordings of `vi`, `less`, three ncurses test programs and `vttest`'s cursor and
      screen menus, made once on a reference system with `TERM` set as ADR-0333 says, are committed with the grid
      expected at marked points. The expected grids are produced by `zoraya-term`, compared by a person with the
      same recording in xterm when they are first made, and then frozen, as alacritty freezes its reference tests
      (`stage-10/survey.md#testing-a-terminal`). A host test replays them.
    - *In the window.* The GUI scripts run `vi` and `less` and read the screen by glyph, as they read `zosh` today.
    `vttest` and `esctest` by hand are a checklist for the stage's end, not a step of `make test`.
11. **Scrollback** becomes a setting, `term.scrollback`, default 2,000 lines, at most 10,000 `[assumption]`; the
    grid is still not reflowed when the window's width changes.

**Examples.** A person sees none of the sequences; they see that programs work:

```
~/proj> less +G big.log
```

fills the window, the wheel and Page Up scroll it, `q` brings the prompt and the earlier screen back.

```
~/proj> cat strange.bin
```

prints boxes and noise; whatever queries the file holds, nothing is typed for the person (item 5), and the next
prompt works normally (ADR-0331 item 9). Selecting a line with the mouse and pressing System+C, then System+V at
the prompt, inserts the text and waits for Enter. Pasting three lines into `sh` shows `paste 3 lines?` under the
grid first.

**Tensions with accepted ADRs.**

- *ADR-0096, "No answers", and INV-0096-2*: kept. Its revisit trigger ("a ported program that needs a terminal
  reply") is examined and not met by stage 10's programs.
- *ADR-0096's output subset* says `\n` moves to the start of the next line, and allows "up to 8 numeric
  parameters". Item 2 makes LF a line feed (ADR-0331 item 8 says who writes the carriage return); item 1 raises
  the bound to 32.
- *ADR-0096's protocol* gains `Paste` (and `Restore`, ADR-0331), new ordinals under ADR-0052.
- *ADR-0096*'s 500-line scrollback becomes a setting (its open question).
- *ADR-0154 rule 7* (the shell's editor "counts one column for each" scalar value): item 9 makes the editor use
  the width table, since the grid now does.
- *ADR-0100 item 2* (musl's `src/ctype` compiled unchanged) is kept: the table is generated from it, not put in
  its place.
- *ADR-0170*: the terminal becomes a clipboard client like `edit`; nothing in the clipboard's rules changes.
- *ADR-0168* ("nothing a program prints can set the title"): kept; `OSC 0` and `OSC 2` are ignored.
- *ADR-0162*: an italic is drawn by slanting the regular face, since no italic font is embedded.

### Effect of the vision

For target 2 the terminal is where an untrusted program's output meets a person. The parser stays total and
bounded, the list is finite, and there is still no path from output to input: a hostile file, repository or server
cannot type. Paste and selection are the person's gestures, and both are made to carry only what the person saw
and confirmed. A typed native protocol (Option D) stays possible beside this one, as ADR-0096 said. Target 3 gains
copy, paste and the mouse in the terminal. Networking: a remote program over ssh sends the same sequences, so the
list is also what a remote `vi` gets; a remote program that queries gets no answer either. Threads: none.

## Consequences

- **Positive.** `vi`, `less`, curses programs, coloured output and remote full-screen programs work; the set is
  finite and checked against the entry; copy and paste exist; the hazard of misread colour sequences ends;
  ADR-0096's "no answers" survives stage 10 whole.
- **Negative.** The parser and the cell grow. A program that waits for a reply (Vim, when it comes; a size probe
  with no window size) waits in vain. The claimed entry promises a dozen things the terminal ignores (item 4).
  Wide characters are laid out but mostly drawn as boxes until fonts with them are embedded.
- **Follow-up.** The parser, the alternate grid, colours and attributes in `zoraya-term`; selection, mouse, paste
  and the key table in `term` and `zoraya-keymap`; the width table and the editor's use of it; the conformance tool
  with its exceptions file and the recordings; `docs/design.md`'s terminal sections.

## Invariants

- **INV-0332-1.** Every capability of the pinned entry is either covered by a host test of `zoraya-term` (its
  effect, or for a key its bytes) or listed with a reason in the exceptions file. *Check:* the conformance tool in
  `make check`, which fails on a capability with neither and on an exception whose capability has a test.
- **INV-0332-2.** No output produces input: for every query the entry or xterm's document defines (status, cursor
  position, device attributes, mode reports, titles, window reports, colours, the clipboard, answerback) the
  terminal sends nothing to the shell. *Check:* INV-0096-2's host test, extended with those sequences, and a fuzz
  test asserting that the only messages `term`'s logic emits toward the shell come from keys, the pointer, a
  paste or a size change.
- **INV-0332-3.** A well-formed sequence outside items 2 and 3 changes no cell, no mode and no cursor position; the
  parser never panics and the cursor stays in the grid for any input. *Check:* the fuzz test of INV-0096-1,
  extended with the grammar of `CSI`, `OSC` and `DCS` and a comparison of the state before and after each unknown
  final.
- **INV-0332-4.** The terminal's grid, `zosh`'s editor and `libposix`'s `wcwidth` give every code point the same
  width. *Check:* a host test that generates the table from the pinned musl files and compares it, for every code
  point, with the generated table the terminal and the editor link; a check that the generator's inputs are the
  files of the pinned archive.
- **INV-0332-5.** Pasted text reaches a program without `ESC` or another control character except tab and newline,
  between the bracket marks exactly when mode 2004 is set, and a paste with a newline reaches a program without
  2004 only after the person's Enter. *Check:* a host test of the paste path with text holding the closing mark,
  control characters and newlines, with and without the confirmation.
- **INV-0332-6.** The recorded sessions replay to their committed grids. *Check:* `cargo test -p zoraya-term`
  (the replay test), offline.
- **INV-0332-7.** The text copied from a selection holds a space for every cell drawn invisible or with equal
  foreground and background. *Check:* a host test of the selection over a grid with such cells.

## Open questions

- Whether mouse motion with no button (1003) is wanted; nothing in scope needs it.
- Fonts for wide characters and emoji (ADR-0162's follow-up); until then they are boxes of the right width.
- Whether Ambiguous width should be a setting for East Asian use; it would have to change musl's answer too.
- Whether the first grids should also be compared by a tool against a second emulator on the host, not only by eye.
- Whether the window's title should show the program in front, set by the shell from the line (not by output).

## Revisit triggers

- A ported program that needs a sequence outside the lists: add it here by name, with the program.
- A ported program that needs a reply (ADR-0096's trigger): Option E with a tag that binds a reply to its sender.
- The pinned entry is updated (ADR-0333): INV-0332-1 lists what is new.
- A measured cost of the larger cell or of direct colour in the terminal's redraw.

## Architect's decision
