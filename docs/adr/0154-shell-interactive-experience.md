# ADR-0154: The shell's interactive experience: editing, history, completion, suggestions and errors

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0013, ADR-0060, ADR-0095, ADR-0096, ADR-0150, ADR-0151, ADR-0153, ADR-0156, ADR-0157
- **Evidence:** DM-069, DM-071, DM-095, CP-22, CP-25, GR-015, shell/survey.md#freebsd-sh-and-base-system, shell/survey.md#bash, shell/survey.md#zsh, shell/survey.md#fish, shell/survey.md#nushell, shell/survey.md#powershell, shell/survey.md#command-line-conventions

## Context

`zosh`'s editor (`crates/zoraya-shell`) keeps a 128-byte line of printable ASCII, erases with `\x08 \x08`, has
Ctrl-U, Ctrl-C, and Up and Down over the last 32 lines; there is no cursor movement, completion or persistent
history. It runs on two terminals: the serial console, raw bytes with no line discipline in the kernel (ADR-0060),
and the GUI terminal, which speaks a VT subset, sends keys as bytes and its size out of band, and answers no query
(ADR-0096). The GUI font draws ASCII and a box for anything else (ADR-0095). Kernel line disciplines are too weak to
use, which is why serious programs edit lines themselves (DM-069); the shell's editor is that editor.

**What is known.** fish's discoverability comes from the interactive layer: suggestions from history accepted with
Right, highlighting that marks an unknown command red, Tab with a list, prefix history search and Ctrl-R
(`shell/survey.md#fish`). PowerShell's PSReadLine has the same set, with predictions off by default
(`shell/survey.md#powershell`). FreeBSD made `sh` usable by adding editing and history to it
(`shell/survey.md#freebsd-sh-and-base-system`). zsh's spelling correction can report false errors
(`shell/survey.md#zsh`). clig asks for errors written for humans with what to do next
(`shell/survey.md#command-line-conventions`).

## Decision drivers

1. Discoverable and forgiving: the person sees what will run before pressing Enter, and a mistake says what to do
   (`shell/survey.md#fish`, `shell/survey.md#command-line-conventions`).
2. Identical on the serial console and the GUI terminal, using only what both offer: no terminal query (ADR-0096,
   DM-071), only the VT subset.
3. Responsive: nothing the editor does on a key blocks on the filesystem or a program (fish's law of
   responsiveness, `shell/survey.md#fish`).
4. UTF-8 throughout (CP-22, GR-015), and nothing a file name or a background program prints can move the person's
   cursor while they edit (DM-071, DM-095); a foreground program owns the terminal while it runs.
5. Completion must come from the same description as help, so they agree (ADR-0156).

## Considered options

### Option A: A POSIX-style editor (FreeBSD `sh`, `libedit`)
- **Description:** emacs keys, history with `fc` and a history file, file name completion, no suggestions or
  highlighting.
- **Evidence for:** Known, small; what FreeBSD ships (`shell/survey.md#freebsd-sh-and-base-system`).
- **Evidence against:** Not discoverable: options and operands are not completed; nothing shows a mistake before
  Enter.
- **Cost:** *(estimate)* 1,500 lines.
- **Precedent:** FreeBSD `sh`, dash with libedit.

### Option B: bash/readline level
- **Description:** As A, plus Ctrl-R search and programmable completion scripts per command.
- **Evidence for:** What most people's hands know (`shell/survey.md#bash`).
- **Evidence against:** Completion lives in scripts separate from the program and its help, so they drift
  (driver 5).
- **Cost:** *(estimate)* As A plus a completion language.
- **Precedent:** bash.

### Option C: fish level, completion from descriptions
- **Description:** Cursor editing, history with prefix search and Ctrl-R, Tab completion of commands, paths, options
  and operands from each command's description, a suggestion from history shown after the cursor, highlighting of an
  unknown command, errors with position and hint.
- **Evidence for:** Drivers 1 and 5; fish and PSReadLine show it works for daily use (`shell/survey.md#fish`,
  `shell/survey.md#powershell`); Nushell completes from signatures (`shell/survey.md#nushell`).
- **Evidence against:** More code on every key; redrawing the line costs bytes on a slow serial line; a suggestion
  can surprise a person who did not expect text after the cursor (PSReadLine keeps it off by default).
- **Cost:** *(estimate)* 3,000 to 4,000 lines in the host-tested crate.
- **Precedent:** fish, PSReadLine, Nushell's reedline.

### Option D: Rio-style: the window edits, the shell gets lines (Plan 9)
- **Description:** The terminal edits text and sends whole lines; the shell has no editor.
- **Evidence for:** No escape sequences for editing at all (ADR-0096 Option D's precedent).
- **Evidence against:** The serial console has no such window; completion needs the shell anyway; ADR-0096 chose
  bytes and a VT subset.
- **Cost:** *(estimate)* A new terminal design.
- **Precedent:** Plan 9's rio.

## Recommendation

### Decision

Accepted as recommended (architect, 2026-11-03), with the hints for familiar Unix names in rule 6, from the tour.

*Changed by the architect, 2026-11-04 (recorded by an agent at the architect's request):* rule 8's subset gains
`ESC [ n A` (cursor up), so that a line longer than the width is redrawn from its start in place; ADR-0096 holds the
sequence already (its note of this date). INV-0154-2 checks the subset with it.

The original recommendation follows.

Recommended: **Option C**. It is a recommendation, not a decision.

1. **Keys.** Left and Right (also Ctrl-B, Ctrl-F), Home and End (also Ctrl-A, Ctrl-E), Backspace, Delete, Ctrl-W
   (erase the word before the cursor), Ctrl-U (erase to the start), Ctrl-K (erase to the end), Ctrl-L (clear the
   screen and redraw), Enter (run, or continue if a quote or `{` is open, with the prompt `...> `), Ctrl-C (clear the
   line), Ctrl-D on an empty line prints `type exit to end the shell` and does nothing else: ending the shell powers
   the machine off (`docs/design.md`), so it is never one key. *Amended by ADR-0155's Decision:* ending the shell no
   longer powers off, but it ends its jobs and loses its variables and functions, so it is still never one key.
2. **History.** Every line run is kept once it has finished; a line already kept moves to the end instead of being
   kept twice; in `/config/history` (ADR-0157), at most
   `shell.history-size` lines (1,000). Up and Down walk the lines that start with what is already typed; Ctrl-R opens
   an incremental search shown on the line below. `history` lists them; `history --clear` empties the file.
3. **Completion.** Tab completes the word at the cursor: command names (builtins, functions, programs in
   `shell.path`) in command position; `$` variables; `%` job numbers; options from the command's description after
   `-` or `--`; operands by their kind from the description (a `read` file completes files and directories, a
   `tree` operand only directories); paths otherwise. One match is inserted; several insert their common start and
   list the candidates below the line, at most one screen, each option with its summary from the description; a
   second Tab does nothing more. Paths are read through the file protocol (ADR-0055) with a deadline of 100 ms
   `[assumption]`, after which the list says `(listing too slow)`.
4. **Suggestion.** While the cursor is at the end, the newest history line that starts with what is typed is shown
   after the cursor in faint text; Right or Ctrl-F at the end accepts it, any other key ignores it. Before the line
   runs or is abandoned (Enter, Ctrl-C) the suggestion is erased, so the screen keeps only what was typed. It never
   runs unless accepted and Enter is pressed. `shell.suggestions = no` turns it off.
5. **Highlighting.** The command word is shown in red while it names nothing (no builtin, function or program), and an
   unclosed quote is red to its end. Nothing else is coloured. `shell.colour = no` turns colour off everywhere.
6. **Errors.** A parse error shows what, where and what to try: the message, the line, a caret under the place, and
   a hint when one is known. An unknown command suggests the nearest known names at edit distance at most 2, never
   running them. *Added by the Decision:* a familiar Unix name that Zoraya does not have gets a hint naming the Zoraya
   command instead:

   | Typed | Hint |
   |---|---|
   | `grep` | `on Zoraya this is search: search TEXT FILE...` |
   | `wc` | `to count lines: count --lines FILE...` |
   | `man` | `on Zoraya this is help: help NAME` |
   | `less`, `more` | `to print a file: cat FILE (help pages page by themselves)` |
   | `which` | `help NAME says what NAME is and where it comes from` |

   The table is the shell's, kept beside the descriptions (ADR-0156); it never runs the suggested command.
7. **Text.** The editor takes UTF-8 and refuses an invalid sequence with a bell-free message. It moves by Unicode
   scalar value and counts one column for each, which matches the GUI font (ADR-0095); on the serial line a wide
   character can misalign the cursor, a stated limitation. File names and anything else the shell prints cannot
   contain control characters (ADR-0013), and the shell replaces any C0 or C1 control character in text it prints
   itself (values of variables, history) by `?`. Output of a background job that arrives while a line is being
   edited is printed above it: the shell erases the line, writes the output with every control character but `\n`
   and `\t` replaced by `?`, and redraws the line. A foreground job's output is passed as it is (ADR-0152 rule 1).
8. **Both terminals.** The editor writes only the ADR-0096 subset: text, `\r`, `\n`, `\x08`, `ESC [ n C/D`,
   `ESC [ K`, `ESC [ J`, `ESC [ H`, and `ESC [ m` with bold, red and the new faint (below). The width is the GUI
   terminal's `Size` message, or `console.columns` (80) on the serial line, where nothing can be asked. A line longer
   than the width wraps, and the editor redraws from the line's start.
9. **Start.** An interactive shell starts in `shell.home` (ADR-0157), prints `Type help to begin, or help tour.`
   and its first prompt.

**Examples.**

```
~> ec<Tab>
~> echo
~> ls no<Tab>
notes/  notes.txt
~> ls notes
~> cp --<Tab>
--force    replace TO if it exists
--help     show this help
~> cp --
~> sl
zosh: sl: no such command
  hint: did you mean ls?
~> grep milk notes.txt
zosh: grep: no such command
  hint: on Zoraya this is search: search TEXT FILE...
~> for f in *.txt { echo $f
...> }
a.txt
b.txt
~> echo 'unclosed
...> '
unclosed

~> if test -f a.txt { echo yes } else echo no }
zosh: line 1: 'else' needs a block
  if test -f a.txt { echo yes } else echo no }
                                     ^
  hint: write: else { echo no }
~> ^D
type exit to end the shell
```

While typing `mak`, with `make -C src` in history, the line reads `~> make -C src` with `e -C src` faint after the
cursor; Right accepts it, and Enter without it runs `mak` and leaves `~> mak` on the screen.

### Effect of the vision

For target 2 the person watching an agent's session sees the same screen; suggestions come only from that session's
history file, which its launcher chooses (ADR-0157). Target 3: the same editor serves every terminal window.

**Tensions with accepted ADRs.** *ADR-0096*'s output subset lists `m` with reset, bold, reverse, colours and default
colours. The suggestion needs a faint style: this ADR adds `ESC [ 2 m` (faint: the foreground at half intensity) and
`ESC [ 22 m` (normal intensity) to the subset. Accepting this ADR replaces that sentence of ADR-0096 (ADR-0001 rule
1); the terminal's parser gains two parameters and INV-0096-1 to -3 keep holding. On the serial line the host's
terminal draws faint itself. CP-25's out-of-band control is unaffected: these are output sequences only.

## Consequences
- **Positive.** A shell that teaches itself: completion shows options with their meaning, suggestions recall
  history, mistakes are caught before Enter.
- **Negative.** More output per key: a redraw after each key costs up to a line of bytes on the serial console. The
  editor must be tested against both terminals' rendering. Faint is a change to an accepted ADR.
- **Follow-up.** The editor in `zoraya-shell`; the faint attribute in `zoraya-term`; the GUI and serial test runners
  ignore SGR sequences when they match text.

## Invariants
- **INV-0154-1.** The editor is total: any byte sequence of input, at any width from 1 to 500 columns, leaves the line
  buffer valid UTF-8 within its bound and the cursor inside the line, without panic. *Check:* a deterministic random
  test in `make test-unit` and a target in `fuzz/`.
- **INV-0154-2.** The editor's output uses only the sequences listed in item 8. *Check:* a host test that feeds random
  keys and parses every byte written with an independent checker of the subset.
- **INV-0154-3.** A suggestion or completion never runs a command: with any keys but Enter, the runner is never
  called. *Check:* a host test of the editor with a runner that fails if called.
- **INV-0154-4.** The screen after any key sequence is the same on both terminals. *Check:* the tour's scripted
  session (TASK-18) on the serial console and in the GUI, compared line by line after SGR sequences are dropped.

## Open questions
- Whether suggestions should also come from completions, as fish's do, or stay history-only (simpler, predictable).
- Whether a history line should be kept when it failed to parse.
- The right path-listing deadline once listing a large directory is measured.
- *Note, 2026-11-03 (TASK-18 Part 2, step 4):* rule 8's subset has no cursor-up, so a line wider than the terminal
  cannot be redrawn from its start in place once the cursor is on a later row. The editor (`zoraya-shell`'s
  `editor`) redraws the row in place while prompt and line fit on one row; past that, typing and Backspace at the end
  stay incremental, and any other edit draws the prompt and line again on a new row, leaving the old rows above.
  Adding `ESC [ n A` to the subset would allow redrawing in place; that is a change to ADR-0096 and this ADR for the
  architect. INV-0154-2 is checked against the subset as listed. *Resolved 2026-11-04:* the architect added it (the
  Decision above).

## Revisit triggers
- The serial console measured to lag while typing because of redraws: redraw only from the cursor.
- A terminal that reports its size or width of characters (a later ADR-0096 revision): use it instead of the setting.

## Architect's decision

Accepted as recommended, 2026-11-03. Decided by the architect; recorded by an agent at the architect's request. Addition from the tour: hints that name the Zoraya command for
`grep`, `wc`, `man`, `less` and `which` (rule 6).
