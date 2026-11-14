# ADR-0332: What the terminal emulates: the sequences of the entry it claims, a short list beside it, and replies made only of numbers

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0096, ADR-0154, ADR-0160, ADR-0161, ADR-0162, ADR-0163, ADR-0170, ADR-0331, ADR-0333
- **Evidence:** CP-22, CP-25, DM-071, DM-072, DM-073, GR-015, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#how-the-terminal-ecosystem-is-layered, stage-10/survey.md#pattern-b-a-trusted-intermediary-that-asks, stage-10/survey.md#programs-and-libraries-without-curses, stage-10/survey.md#terminfo-entries-compared, stage-10/survey.md#testing-a-terminal, stage-10/survey.md#the-terminal-as-built, stage-10/survey.md#utf-8-and-character-width, stage-10/survey.md#what-a-small-terminal-implements, stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal

## Context

ADR-0096 gave the terminal a subset of the VT sequences that `zosh` and the stage 6 ports needed, and said that
"later programs (`vi`, `less`) will need scroll regions, insert and delete line, the alternate screen and cursor
visibility: each is a new sequence in the parser". Stage 10 is that later. The parser today knows nine `CSI` finals
and drops every private mode; every character is one cell; there are no function keys, no mouse, no paste; a
256-colour sequence sets wrong attributes; and no query is answered, by an invariant
(`stage-10/survey.md#the-terminal-as-built`). This ADR decides which sequences, colours and keys the terminal
supports, what it answers, how wide a character is, and how conformance is tested. ADR-0333 decides the name the
terminal claims; the two are one design, because the claimed entry is what programs read.

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
- Programs without curses (toybox, busybox `vi`, kilo) ask the terminal for the cursor position when they cannot
  get the size another way; Vim asks for the position and the terminal's version
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
3. **No output becomes input that a program chose** (DM-071, DM-072, CP-25).
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
- **Evidence against:** The document has about 426 mode and parameter lines
  (`stage-10/survey.md#how-the-terminal-ecosystem-is-layered`); st's authors measure xterm at more than 65,000 lines
  and built st to avoid that (`stage-10/survey.md#what-a-small-terminal-implements`). The reports include the ones
  DM-071 records as attacks. The set is not finite in practice: it grows with xterm.
- **Cost:** *(estimate)* Tens of thousands of lines, most for sequences no ported program sends.
- **Precedent:** xterm; VTE and others approximate it and differ in the corners.

### Option B: The capabilities of the claimed entry, a named list beside it, and number-only replies

- **Description:** The terminal implements every capability of the terminfo entry ADR-0333 pins, plus a short list
  of sequences programs hard-code, ignores every other well-formed sequence, and answers only the queries whose
  reply is fixed text and numbers of its own.
- **Evidence for:** Drivers 1 to 6. The entry is a finite, versioned list, and it is exactly what curses, `less` and
  Python's REPL read (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`). foot, st and alacritty are
  this size and publish their lists (`stage-10/survey.md#what-a-small-terminal-implements`).
- **Evidence against:** Programs that assume xterm beyond its entry meet gaps, which is foot's own warning about
  terminals that claim an xterm name (`stage-10/survey.md#what-a-small-terminal-implements`). Answering any query
  gives up ADR-0096's absolute "no answers".
- **Cost:** *(estimate)* 2,500 lines in `zoraya-term` and its tests, 400 in `term` (mouse, selection, paste, keys),
  a width table crate, a conformance tool.
- **Precedent:** foot (264 capabilities), alacritty (269), st (214), each with a documented list.

### Option C: A small VT100-class set with no replies (the Linux console's size)

- **Description:** Add only what `vi` and `less` need: cursor moves, erases, insert and delete line, a scroll
  region, reverse index, the alternate screen, cursor visibility; eight colours; no mouse, no paste mode, no
  replies. A small own entry describes it.
- **Evidence for:** The smallest parser; ADR-0096's "no answers" stands untouched (driver 3 by construction). The
  Linux console shows that 119 capabilities and eight colours are a usable terminal
  (`stage-10/survey.md#terminfo-entries-compared`).
- **Evidence against:** `less` and Vim send mouse and paste sequences from hard-coded strings whatever the entry
  says, so the set must at least parse them (`stage-10/survey.md#what-full-screen-programs-ask-of-a-terminal`).
  Programs without curses lose their size probe. An own small entry exists on no other machine, which ADR-0333
  weighs. 256 colours are what `less`, `git diff` and `ls` output assume today.
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

## Recommendation

**Recommendation (not a decision): Option B.**

1. **The rule.** `zoraya-term` implements every string capability of the pinned `xterm-256color` entry (ADR-0333),
   the list of item 3 beside it, and nothing else. Any other well-formed sequence is consumed and ignored: it never
   prints, never answers and never changes state. The parser stays total; a `CSI` holds at most 32 parameters and
   sub-parameters, a string at most 4096 bytes (INV-0096-1 and INV-0096-3 stand).
2. **What the entry brings**, by group (the table of `stage-10/survey.md#terminfo-entries-compared` has each
   sequence):
   - C0: BS, HT, CR, LF, and VT and FF as LF. **LF moves down and keeps the column**, where today it also returns to
     column 0; `zosh` and the discipline write CR LF (ADR-0331), so nothing a person sees changes. SO and SI select
     the line-drawing set.
   - Cursor: CUU, CUD, CUF, CUB, CNL, CPL, CHA, VPA, CUP, HVP, save and restore (`ESC 7`, `ESC 8`), tab stops
     (HTS, TBC, CHT, CBT).
   - Erase and edit: ED 0 to 3 (3 clears the scrollback), EL, ECH, ICH, DCH, IL, DL, REP, insert mode (IRM).
   - Scrolling: DECSTBM, SU, SD, IND, RI, NEL.
   - Modes: DECCKM (1), reverse video (5), origin (6), DECAWM (7), DECTCEM (25), the alternate screen (47, 1047,
     1048, 1049), keypad application mode. The alternate screen is a second grid with no scrollback.
   - SGR: 0 to 9, 21 to 29, 30 to 37, 39, 40 to 47, 49, 90 to 97, 100 to 107, and 38 and 48 with `5;N` (256 colours)
     and `2;R;G;B` (direct colour), in the semicolon and the colon forms. An extended colour's parameters are
     consumed as one unit, which ends today's misreading. Bold is a weight and no longer brightens the colour;
     blink is stored and not animated; italic is drawn slanted.
   - Character sets: G0 and G1 with `B` (ASCII) and `0` (DEC line drawing), which `acsc` needs.
   - `ESC c` (full reset).
3. **Beside the entry**, because programs send them from hard-coded strings:
   bracketed paste (2004); mouse reporting 1000 and 1002 with the SGR encoding 1006, and no other encoding; focus
   events (1004); synchronized output (2026), held for at most 150 ms `[assumption]` and then shown;
   cursor shape (`CSI Ps SP q`); `OSC 0` and `OSC 2` (the window's title while the job is in front, printable
   characters, at most 240 bytes); `OSC 4` and `OSC 104` (set and reset palette entries 16 to 255). Modes 12, 1003,
   1005, 1015, 1034 and 2027 are recognised and have no effect.
4. **Colours.** Entries 0 to 15 come from ADR-0161's `term.*` tokens, in the light and the dark set; 16 to 255 are
   xterm's cube and grey ramp; direct colours are stored per cell. A cell grows to 16 bytes `[assumption]`.
5. **Replies: made of numbers only, to the job in front only.** The terminal answers exactly these:
   - `CSI 5 n` with `CSI 0 n` (status);
   - `CSI 6 n` with `CSI ROW ; COLUMN R` (cursor position);
   - `CSI c` and `CSI > c` with one fixed string each, naming a VT220-class terminal and a version (device
     attributes);
   - `CSI ? Ps $ p` with `CSI ? Ps ; STATE $ y` (whether a mode is set).

   Every reply is fixed text and decimal numbers that the terminal computes from its own state; none contains a byte
   the program supplied, a newline or a control character other than its `ESC`. A reply is handed to the shell in a
   message of its own kind (`Reply`, a new ordinal of `zoraya-proto-term`), and the shell passes it to the
   discipline only when the job in front has canonical mode off; otherwise it is dropped. So a query printed by `cat`
   of a hostile file, or left behind by a program that has ended, puts nothing on the shell's line. At most 16
   replies wait; more are dropped. **Never answered:** titles and icon names, the window's size or position, colour
   and palette queries, `DECRQSS`, `XTGETTCAP`, the clipboard (`OSC 52`), answerback (`ENQ`), and anything else.
6. **Keys.** Beyond ADR-0096's: F1 to F12; Alt as an `ESC` prefix; Shift+Tab as `CSI Z`; Ctrl+Space and Ctrl+@ as
   NUL, Ctrl+^ and Ctrl+_ as their control codes; Shift, Alt and Ctrl on arrows, Home, End, Page Up, Page Down and
   the function keys as xterm's modifier parameter (`CSI 1 ; M A`); arrows, Home and End in their `SS3` forms while
   DECCKM is set. Each is the entry's `k*` string, so the key table is checked against the entry (INV-0332-1).
   Chords with the System key never reach a client (INV-0160-3). Shift+Page Up and Page Down scroll the view.
7. **Mouse, selection and paste.** Without mouse reporting, a drag selects text and System+C copies it (ADR-0170's
   `Copy` and `Offer`); with reporting on, Shift+drag selects. System+V pastes: `term` sends the text to the shell in
   `Paste` messages (a new ordinal, flow-controlled, at most 64 KiB `[assumption]`), never as `Input`. The shell
   inserts a paste into its own line without running it. For a program in front it passes the text through the
   discipline, between `CSI 200 ~` and `CSI 201 ~` when the program set 2004, with `ESC` and the other control
   characters except tab and newline removed either way, so that a paste cannot end the bracket or send a sequence.
8. **Width.** One table, in a `no_std` crate generated from one pinned version of the Unicode data files, gives the
   width of every code point: 0 for nonspacing and enclosing marks and format characters, 2 for East Asian Wide and
   Fullwidth, 1 for everything else, Ambiguous included (the rule of Kuhn's `wcwidth`,
   `stage-10/survey.md#utf-8-and-character-width`). `zoraya-term` uses it for the grid, and `libposix`'s `wcwidth`
   and `wcswidth` are the same table, in place of musl's. A width-0 character joins the cell before it (at most two
   per cell, more are dropped); a width-2 character takes two cells and wraps whole. A character with no glyph draws
   the hollow box at its width. No grapheme clustering, shaping or right-to-left layout.
9. **Scrollback** becomes a setting, `term.scrollback`, default 2,000 lines, at most 10,000 `[assumption]`; the grid
   is still not reflowed when the window's width changes.
10. **The conformance test** has three layers, all without the network:
    - *The entry.* A host tool reads the pinned entry and checks that every capability has a test in
      `zoraya-term`'s host tests (output capabilities by their effect on the grid, key capabilities against the key
      table) or a line with a reason in a list of exceptions; `make check` fails on a capability with neither.
    - *Recorded sessions.* Byte recordings of `vi`, `less`, three ncurses test programs and `vttest`'s cursor and
      screen menus, made once on a reference system with `TERM` set as ADR-0333 says, are committed with the grid
      expected at marked points. The expected grids are produced by `zoraya-term`, compared by a person with the
      same recording in xterm when they are first made, and then frozen, as alacritty freezes its reference tests
      (`stage-10/survey.md#testing-a-terminal`). A host test replays them.
    - *In the window.* The GUI scripts run `vi` and `less` and read the screen by glyph, as they read `zosh` today.
    `vttest` and `esctest` by hand are a checklist for the stage's end, not a step of `make test`.

**Examples.** A person sees none of the sequences; they see that programs work:

```
~/proj> less +G big.log
```

fills the window, the wheel and Page Up scroll it, `q` brings the prompt and the earlier screen back.

```
~/proj> cat strange.bin
```

prints boxes and noise; whatever queries the file holds, the next prompt is empty (item 5) and types normally
(ADR-0331 item 7). Selecting a line with the mouse and pressing System+C, then System+V at the prompt, inserts the
text and waits for Enter.

**Tensions with accepted ADRs.**

- *ADR-0096, "No answers", and INV-0096-2* ("the terminal's input to the shell is a function of keys alone"). Item 5
  ends the absolute rule. What it protected is kept in a narrower form, INV-0332-2 and INV-0332-3: no reply carries
  a program's bytes, and none reaches anything but the job in front in raw mode. ADR-0096's revisit trigger names
  this case ("a ported program that needs a terminal reply"). Accepting this ADR replaces INV-0096-2 by those two.
- *ADR-0096's output subset* says `\n` moves to the start of the next line. Item 2 makes LF a line feed; the shell
  and the discipline write CR LF.
- *ADR-0096's protocol* gains `Reply` and `Paste` (and `Restore`, ADR-0331), new ordinals under ADR-0052.
- *ADR-0096*'s 500-line scrollback becomes a setting (its open question).
- *ADR-0170*: the terminal becomes a clipboard client like `edit`; nothing in the clipboard's rules changes, and
  `OSC 52` stays unanswered and ignored, since a program must not copy without the person's gesture.
- *ADR-0160 and ADR-0168*: a program in front may set the window's title text; the room's badge and the grants on
  the frame are the compositor's and are untouched (INV-0172-1).
- *ADR-0162*: an italic is drawn by slanting the regular face, since no italic font is embedded.

### Effect of the vision

For target 2 the terminal is where an untrusted program's output meets a person. The parser stays total and
bounded, the list is finite, and the one path from output to input carries numbers only and stops at the job that
asked. A typed native protocol (Option D) stays possible beside this one, as ADR-0096 said. Target 3 gains copy,
paste and the mouse in the terminal. Networking: a remote program over ssh sends the same sequences, so the list is
also what a remote `vi` gets. Threads: none.

## Consequences

- **Positive.** `vi`, `less`, curses programs, coloured output and remote full-screen programs work; the set is
  finite and checked against the entry; copy and paste exist; the hazard of misread colour sequences ends.
- **Negative.** The parser and the cell grow. ADR-0096's absolute "no answers" is given up for a bounded form. A
  program that assumes xterm behaviour outside the entry and the list finds it missing. Wide characters are laid
  out but mostly drawn as boxes until fonts with them are embedded.
- **Follow-up.** The parser, the alternate grid, colours and attributes in `zoraya-term`; selection, mouse, paste
  and the key table in `term` and `zoraya-keymap`; the width crate and `libposix`'s use of it; the conformance tool
  and the recordings; `docs/design.md`'s terminal sections.

## Invariants

- **INV-0332-1.** Every capability of the pinned entry is either covered by a host test of `zoraya-term` (its
  effect, or for a key its bytes) or listed with a reason in the exceptions file. *Check:* the conformance tool in
  `make check`, which fails on a capability with neither and on an exception whose capability has a test.
- **INV-0332-2.** Every byte sequence the terminal emits in reply to output is one of item 5's forms, with decimal
  numbers bounded by the grid's size and the mode numbers, and no byte copied from the output. *Check:* a host fuzz
  test of `zoraya-term` asserting that each emitted reply matches one of the four patterns; a host test feeding
  every query sequence of ADR-0096's INV-0096-2 test and asserting silence for all but the four.
- **INV-0332-3.** A reply reaches a program only when that program's job is in front and its terminal is in
  non-canonical mode; in every other state it is dropped, and the shell's own line never receives one. *Check:* a
  host test of the shell's input path with a query answered before, during and after a job, in both modes.
- **INV-0332-4.** A well-formed sequence outside items 2, 3 and 5 changes no cell, no mode and no cursor position
  and emits nothing; the parser never panics and the cursor stays in the grid for any input. *Check:* the fuzz test
  of INV-0096-1, extended with the grammar of `CSI`, `OSC` and `DCS` and a comparison of the state before and after
  each unknown final.
- **INV-0332-5.** The terminal's column count for a string equals the sum of `wcwidth` in `libposix` for its code
  points. *Check:* a host test over every code point of the pinned data comparing the two exports of the width
  crate, and a check that `libc.a`'s `wcwidth` is the crate's symbol.
- **INV-0332-6.** Pasted text reaches a program without `ESC` or another control character except tab and newline,
  and between the bracket marks exactly when mode 2004 is set. *Check:* a host test of the paste path with text
  holding the closing mark and control characters.
- **INV-0332-7.** The recorded sessions replay to their committed grids. *Check:* `cargo test -p zoraya-term`
  (the replay test), offline.

## Open questions

- The exact device-attribute strings; 10a takes them from xterm's document for a VT220 with colour.
- Whether mouse motion with no button (1003) is wanted; nothing in scope needs it.
- Fonts for wide characters and emoji (ADR-0162's follow-up); until then they are boxes of the right width.
- Whether Ambiguous width should be a setting for East Asian use; the table and `wcwidth` would change together.
- Whether the first grids should also be compared by a tool against a second emulator on the host, not only by eye.

## Revisit triggers

- A ported program that needs a sequence or a reply outside the lists: add it here by name, with the program.
- A reply found to carry anything a program can choose.
- The pinned entry is updated (ADR-0333): INV-0332-1 lists what is new.
- A measured cost of the larger cell or of direct colour in the terminal's redraw.

## Architect's decision
