# TASK-31, part 10b: keys, the mouse, selection and paste

Branch `work/10b`, from `work/10a` at 35f426a (10a was green in CI and is merged to `main` as e3e5774; the dispatcher merges
`origin/main` before the merge). Written by the implementation session of part 10b (a Sonnet agent); the independent review is
`opus-review`'s, afterwards. Nothing here is merged, tagged or pushed to `main`; the branch is pushed as `origin/work/10b`.

## What exists

| What | Where |
|---|---|
| The key table: F1 to F12, Alt as an `ESC` prefix, xterm's modifier parameter, `SS3` forms under DECCKM, the keypad (characters, or `SS3` letters in keypad application mode), Ctrl+Space and Ctrl+@ as NUL, Shift+Tab as `CSI Z`, Shift+Page keys left to the view | `crates/zoraya-term/src/keys.rs`, `crates/zoraya-keymap/src/lib.rs` (key codes, Ctrl+^ and Ctrl+_ in every layout) |
| The key table checked against every `k*` capability of the pinned entry (151 rows) | `crates/zoraya-term/src/tests_keys.rs` |
| Mouse reports (1000, 1002, SGR 1006), the wheel as buttons 64 and 65, focus events (1004), Shift+drag selecting while a program has the mouse | `crates/zoraya-term/src/mouse.rs` |
| Selection (linear, ended by a change to its rows), its drawing in `bg.selection`, the text copied (invisible cells and cells with foreground = background as spaces, control characters as spaces, trailing blanks dropped) | `crates/zoraya-term/src/select.rs`, `term.rs` (`touch`, `mark`, `render`), `lib.rs` (`Look::sel_fg`, `sel_bg`, `cell_at`) |
| `Session`: the terminal as the window runs it, the only code that makes anything to send | `crates/zoraya-term/src/session.rs` |
| `SendQueue`: what waits for a slow shell (input bounded and counted, a paste kept or dropped whole) | `crates/zoraya-term/src/queue.rs` |
| `term`: pointer, wheel, focus, `Copy` answered by `Offer`, `Paste` sent to the shell | `user/src/bin/term.rs` |
| The `Paste` message (ordinal 5) and the cleaning rule (no `ESC` or control character but tab and newline, CR as line end) | `crates/zoraya-proto-term/src/{lib,paste}.rs` |
| The shell's side: parts joined and cleaned, a paste's place in the key stream (bytes 0xF8 to 0xFF), `Editor::paste` (inserts at the cursor, never runs; a newline makes a continuation line), Ctrl+V to show a key's bytes, word keys (Ctrl and Alt with Left and Right, Alt+b, Alt+f, Alt+Backspace) | `crates/zoraya-shell/src/{paste,editor}.rs`, `user/src/bin/zosh.rs` |
| GUI script: drag, System+C, System+V, waits for Enter, two lines with a continuation line, F5, F1, Alt+x and Ctrl+Right shown with Ctrl+V | `tests/gui/selection.script` (in `GUI_RUNS_ALL` and `tools/ci-tests.txt`) |
| Three proposed ADRs | `docs/adr/0383-*.md`, `0384-*.md`, `0385-*.md` (index rows at `proposed`) |
| The conformance exceptions: the 165 `deferred (10b)` lines are gone (157 implemented and tested, 8 re-excepted with a reason: `kbs`, `kPRV`, `kNXT`, `kbeg`, `kp5`, `km`, `smm`, `rmm`; 31 exceptions in all with the 23 by ADR-0332 item 4); the tool now fails if one reappears | `crates/zoraya-term/entry/exceptions.txt`, `tools/term-conformance/check.sh` |

**10a's files touched** (minimal): `crates/zoraya-term/src/lib.rs` (the old `key_bytes` moved to `keys.rs`; `Look` gained `sel_fg`,
`sel_bg`, `cell_at`; modules added), `term.rs` (the `selection` field, `touch`/`touch_all` end a selection, `mark`/`mark_all` do
not, `render` draws it), `tests.rs` (the `key_bytes` test), `crates/zoraya-shell/src/editor.rs` (paste, quote, word keys),
`user/src/bin/term.rs` and `zosh.rs`, `Makefile` (`GUI_RUNS_ALL`), `tools/ci-tests.txt`, `fuzz/Cargo.lock` (`zoraya-proto-term`; checked with
`cargo +nightly-2026-11-12 metadata --locked --offline` in `fuzz/`), `docs/design.md`, `docs/backlog.md`, ADR-0332's open-question note
(rule 6) and `docs/adr/invariants.md` (the Status of INV-0332-2, -5 and -7).

## What was run, and what it gave

All on the Mac mini (M4, 16 GB), `CARGO_BUILD_JOBS=3`, HVF, in the worktree. Never run: `make test`, `make test-par`, the CI candidate.

| Command | Result |
|---|---|
| `cargo test -p zoraya-keymap` | 20 pass (two new: Ctrl+^, Ctrl+_, Ctrl+Space and Ctrl+@ in all four layouts; Ctrl, Alt and AltGr in all four) |
| `cargo test -p zoraya-term` | 182 unit tests and 2 replay tests pass (new: `tests_keys.rs` 8, `tests_session.rs` 28, `queue.rs` 6) |
| `cargo test -p zoraya-proto-term` | 9 pass (the cleaning rule over every code point to U+01FF, the boundaries, the marks and bounds) |
| `cargo test -p zoraya-shell` | 173 pass (the paste end to end, the hostile paste, quote, word keys, the assembler and store) |
| `tools/term-conformance/check.sh` and `selftest` | 287 capabilities: 256 covered by tests, 31 excepted, none deferred |
| `make check` | passes (all 23 steps; at the commits `df29d46`, `235158c`, `a696821`, `31e3d09`; `2f66c30` was committed after `make adr-check` only, 0 errors, and `make check` ran at the next commit) |
| `GUI_RUNS="selection:auto:1" make test-gui` | PASS, 59 steps (first two runs failed in my script: the pointer's glyph over a row the script read, and a missing `window` after `chord ctrl l`; both fixed) |
| `GUI_RUNS="selection:auto:2 selection:auto:1600x1000 selection:auto:1920x1200" make test-gui` | PASS, 59 steps each |
| `GUI_RUNS="selection:auto:1 terminal:auto:1 windows:auto:1 wheel:auto:1 selection:auto:1920x1200 terminal:auto:1600x1000" make test-gui` (at the final code) | PASS: 59, 97, 63, 79, 59, 97 steps |
| `cargo +nightly-2026-11-12 metadata --locked --offline` in `fuzz/` | exit 0 (the lock is consistent) |

**Not run, and why.** `make test-changed` as a whole: the map sends the change (`crates/zoraya-proto-term/`, `user/src/bin/`) to every
step including `test-apps`, `test-cxx*`, `test-netsurf`, `test-python*`, which need `make fetch-ports` and the ports' builds
(`DRY=1 tools/test-changed.sh` lists them); as in 10a. `make test-qemu`: the session's permission check refused the command; the
terminal and shell binaries were exercised by the GUI runs above. `make test-files` and `test-boot` were not started for the same
reason. The TCG GUI runs (CI). `make check-ports`. A raw-mode POSIX test program (10c).

### Mutation testing (`make mutants-host`, `--in-diff`, under the watchdog; about 35 minutes in all)

| Crate, files | Mutants | Result |
|---|---|---|
| `zoraya-term`: `keys.rs`, `mouse.rs`, `select.rs`, `session.rs`, `queue.rs`, `term.rs`, `lib.rs` | 366 | 320 caught, 39 missed, 7 unviable |
| the same, after tests for the holes | 366 | 345 caught, 14 missed, 7 unviable |
| `queue.rs`, `mouse.rs`, `select.rs` again | 197 | 183 caught, 11 missed, 3 unviable |
| `mouse.rs` again, after the last test | 88 | 78 caught, 8 missed, 2 unviable |
| `zoraya-keymap` | 5 | 5 caught |
| `zoraya-proto-term` | 19 | 17 caught, 1 unviable, 1 timeout (the loop that does not end) |
| `zoraya-shell`: `editor.rs`, `paste.rs` (first, after the fixes, last) | 90, 84, 79 | last: 73 caught, 4 unviable, 2 timeouts, 0 missed |

Holes killed by tests: a selection following the pointer as the button stays down, and ending with it; a hover that is no start;
a release reported only for a press that was (and a Shift press never); the wheel's guards on their own and Alt's bit; Shift with another
modifier on Page keys; the selection API on its own (clicks, ends, the tail of a wide character, cells outside the grid, clamps);
a selected cell and a hidden one drawn (pixels); the queue's sums and the room a sent message frees; a quoted space, Alt+Enter and
Alt+Backspace (they run and delete nothing); a paste with control characters given to the editor uncleaned; where an entry is full
counting its lines. Two pieces of code the run showed redundant were removed (the `cut` flag of the assembler, which nothing read; the
CSI and SS3 states of the quote, since every byte after `ESC [` is typed as text anyway).

**The 11 survivors that are equivalent** (each reasoned): `|` read as `^` where the operands are disjoint bits (`code_mods`, the button
number with the modifier bits and 32, the wheel's 64 with the direction bit and the modifier bits: seven mutants); `notches > 0` as `>=`
in the wheel's code (zero notches make no report whichever the code); `x > 0` as `x >= 0` in `selected` and `clamp_cell` (column 0 is never
the right half of a wide character, so `x - 1` is never reached); in the shell, `i += 1` as `i *= 1` in `word_end` (the loop does not
end: a timeout, which is detection).

## Decisions taken (no one could be asked)

- **Backspace stays `0x7f`; the entry's `kbs=^H` is an exception** (ADR-0096 is accepted and binding; ncurses 6.6 built without
  `--with-xterm-kbs` says `^H`). Option C of ADR-0383, building the entry with `kbs=^?`, is the architect's with ADR-0333.
- **"F-keys and Ctrl+arrows show their sequences in a test program"** is met by **Ctrl+V at the prompt**, which puts the next key's bytes
  in the line as `^[[15~` (what `cat -v` shows). A native program reads cooked lines (`zosh` swallows the sequences) and a POSIX raw-mode
  program needs 10c's terminal for programs; the same sequences are also checked byte for byte against the entry by `tests_keys.rs`.
- **The keypad has no NumLock**: it sends characters, or `SS3` letters once a program sets keypad application mode; `kbeg` and `kp5` are exceptions.
- **A selection ends when a cell under it changes** (a row it covers is written, the view scrolls, the window resizes): what is copied is
  what was seen. No word or line selection, no wrap flag (a wrapped line copies as two lines).
- **Where the paste goes**: `term` sends `Paste` parts of at most 4 KiB (whole characters, 64 KiB in all); the shell cleans (the sender
  does not), puts the text in the key stream at its place as one of the bytes 0xF8 to 0xFF (never in UTF-8), and the editor inserts it.
  At the prompt a newline makes a continuation line and runs nothing (there is no `paste N lines?` at the prompt: the continuation lines are
  what the person sees before Enter); a tab is a space; a paste past the editor's limits (4,096 characters, 256 lines) is cut and says so.
- **A paste while a program runs is dropped** with a line saying so (`zosh: paste: not sent: a program is running`); the paste to a program
  (brackets for 2004, the confirmation) is 10c's. The session's `ttyd` ignores `Paste`, as ADR-0331 item says it does.
- **Mouse**: SGR only; 1003 ignored; the wheel is reported (buttons 64 and 65, at most 8 a turn) while a program has the mouse and Shift is
  not held, else it scrolls the view; Shift+drag selects; focus events as `CSI I` and `CSI O`. Focus joins keys, the pointer, a paste and a size
  as a gesture that sends (ADR-0385).
- **`Session`** (`zoraya-term`) is where anything to send is made; `output` cannot reach its outbox, which is what INV-0332-2's second half
  asserts (hostile output of every query kind and 128,000 random bytes leave the outbox empty; the same gestures send the same whatever
  the output).
- **`Paste` is ordinal 5** of `zoraya-proto-term`; `Restore` (10c) and `Status` (10f) take the next numbers there.
- **The GUI script** parks the pointer with a wheel turn that changes nothing, because the compositor draws the pointer and the reader sees
  its glyph on the rows under it; the drags end on rows read no more.

## Proposed ADRs

| ADR | Recommendation |
|---|---|
| ADR-0383, the terminal's keys where ADR-0332 item 7 is silent | Option A: the entry's `k*` strings with ADR-0096's Backspace as the one named exception, the keypad, Alt, Ctrl+Space and Ctrl+^ and Ctrl+_ in four layouts, Ctrl+V to see a key's bytes; Option C (build the entry with `kbs=^?`) when ADR-0333 is next opened |
| ADR-0384, selecting, copying and pasting where ADR-0332 item 8 is silent | Option A: a linear selection that ends when its cells change, `Offer` only when something is selected, `Paste` parts cleaned by the shell, newlines at the prompt as continuation lines; Option B (a selection anchored in the scrollback) is the growth path |
| ADR-0385, the mouse reports, wheel and focus, and one place that makes what is sent | Option A: SGR reports for 1000 and 1002, the wheel as buttons 64 and 65, Shift selects, focus `CSI I` and `CSI O`, `Session` as the only emission point |

None changes an accepted ADR. ADR-0383's Backspace line records a disagreement between ADR-0096 and the pinned entry of ADR-0333 as an
exception, not a change.

## Invariants (ADR-0001 rule 6)

- **INV-0332-1**: the key half passes: the conformance tool, in `make check`, over 287 capabilities (256 tested, 31 excepted with reasons).
- **INV-0332-2**: now checked for the emission points too (`tests_session.rs`); the `term` binary only sends what `Session::take_outbox` gives.
- **INV-0332-5**: checked for the prompt's half (`cargo test -p zoraya-shell`: the closing mark, control characters, newlines, CRLF, no run before
  Enter; `zoraya-proto-term`'s cleaning rule over every code point to U+01FF); the program's half (brackets for 2004, the confirmation of a newline)
  is part 10c's. Status text says "partly".
- **INV-0332-7**: checked (`cargo test -p zoraya-term`: invisible text, equal colours as index and as direct colour, reverse and faint seen, a
  hidden run keeps its length, controls as spaces, trailing blanks dropped).
- ADR-0332's open-question note (rule 6) says all of this; `docs/adr/invariants.md` has the three Status cells changed; no row was added (the
  proposed ADRs have none yet), so the Total is unchanged.

## Limits and unfinished

- No raw-mode program that prints the keys it reads (10c); the F-keys and Ctrl+arrows are seen at the prompt through Ctrl+V.
- A paste to a program in front is dropped (10c); in the session's `ttyd` a paste does nothing.
- No word or line selection, no primary selection or middle-click paste (ADR-0170 refuses it), a wrapped line copies as two, a selection
  does not survive output on its rows or a scroll.
- The pointer is seen at frames; a button released outside the window is seen at the next event.
- No hover (1003) and no mouse encoding but SGR. F12 and Alt+Tab never reach a client. The keypad has no NumLock.
- The key table across four layouts is checked in `zoraya-keymap` (Ctrl and Alt chords) and `zoraya-term` (bytes from text); the GUI script
  runs in the US layout only.
- Not run locally: the TCG GUI runs, `make test-changed` as a whole, `make test-qemu`, `make test-files`, `make test-boot`, `make check-ports`.

## Hand-over for the architect

1. **Decide ADR-0383 to ADR-0385** and the index rows I added (`proposed`). The one that touches another ADR's pin is ADR-0383: ADR-0333's entry
   says `kbs=^H` and ADR-0096 says Backspace is `0x7f`; either build the entry with `kbs=^?` (Option C, one line in `tools/term-terminfo`, a new
   manifest line) or accept the exception.
2. **`vttest`** (10a's open item) is still unpinned.
3. **CI**: the candidate run is the dispatcher's; the new GUI runs are `selection:auto:1`, `selection:tcg:1`, `selection:auto:2`,
   `selection:auto:1600x1000` and `selection:auto:1920x1200` (grouped in `tools/ci-tests.txt`; `make check-ci-jobs` passes).
4. **Merge note**: this branch is from 35f426a; `origin/main` has the 10a merge (e3e5774) and may have more. Conflicts, if any, are most likely
   in `docs/backlog.md`, `docs/design.md` and `docs/adr/index.md` (both sides added rows).

## Time

About three hours of wall time on the Mac mini (one Sonnet session): reading, the key table and its test against the entry, the mouse,
selection and `Session`, the protocol, the shell and the editor, four runs of the GUI script to get it passing (two of my script's faults), the
three ADRs, the mutation passes (about 35 minutes) and this report.
