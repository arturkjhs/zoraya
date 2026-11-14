# TASK-31: review of part 10a, the terminal's parser and grid (branch `work/10a`)

An independent review by the `opus-review` subagent, which wrote none of the reviewed code, of
`git diff origin/main...work/10a` at `67b35e5`: `crates/zoraya-term` (`parse.rs`, `term.rs`, `screen.rs`, `cell.rs`,
`colour.rs`, `read.rs`, `lib.rs`, the tests, `tests/replay.rs`, `entry/`, `recordings/`), `crates/zoraya-width`,
`crates/zoraya-shell` (`crlf.rs`, `editor.rs`), `user/src/bin/term.rs`, `zosh.rs`, `t_text.rs`, `crates/zoraya-text`,
`tools/term-width`, `tools/term-terminfo`, `tools/term-conformance`, `tools/term-record`, `ports/sources.lock`,
`ports/terminfo.manifest`, `tests/gui/terminal.script` and its recordings, `docs/design.md`, `docs/dependencies.md`,
`docs/adr/invariants.md`, the rule-6 notes on ADR-0331 to ADR-0333 and the proposed ADR-0380 to ADR-0382. Read against
ADR-0331 (item 8), ADR-0332 (items 1 to 11, INV-0332-1 to -7), ADR-0333, plan row 10a and the part's report
(`docs/tasks/TASK-31-part-10a.md`), whose claims were re-run, not taken.

## What was run

All in the worktree of `work/10a` (and a detached scratch copy of `67b35e5` for the breaks, removed afterwards),
`CARGO_BUILD_JOBS=3`:

| Command | Result |
|---|---|
| `make check` | all checks pass |
| `cargo test -p zoraya-term -p zoraya-width -p zoraya-shell` | 123 + 2 (replay) + 5 + 155 pass, 2 ignored |
| `make test-gui GUI_RUNS="terminal:auto:1"` (HVF) | `PASS: 100 steps` |
| `make check-host-paths` | passes |
| A scan of every file in `crates/zoraya-term/recordings/`, `tests/gui/terminal.root/recordings/` and `tools/term-record/` for `/Users/`, `/home/`, `/private/`, `/var/folders`, the host's user and host name and e-mail addresses | nothing found |

Hand breaks, each applied alone in the scratch copy and reverted (the test that fails is named):

| Break | Result |
|---|---|
| A. `mc0` removed from `exceptions.txt`, and `// Not a test: cap("mc0", &[])` appended to `tests_caps.rs` | **passes** `tools/term-conformance/check.sh` ("100 covered, 187 excepted"): finding 2 |
| B. `CSI y` (not in ADR-0332's lists) added to `csi_known` and given an effect (`self.pen.attrs \|= BOLD`) | **passes every test** of `zoraya-term`: finding 3 |
| B2. `CSI ? s` added to `csi_known` and made to set insert mode | **passes every test**: finding 3 |
| C. `fix_row` removed from `delete_cells` | fails `overwriting_half_a_wide_character_blanks_the_other_half` and the fuzz (`tests_total.rs:76`, the wide-pair check) |
| D. the `MAX_STRING` cut removed from `string()` | fails `memory_is_bounded`, `strings_are_cut_at_their_bound_and_not_run`, `memory_stays_within_its_bound` |
| F. `38;5;N` read as `N ^ 1` | fails `tests_caps::colours`, `extended_colours_are_one_unit_in_both_forms`, a render test, and `tests/replay.rs` (the frozen grids) |
| G. `OSC 52` made to set palette entry 0 | fails `no_query_is_answered_and_none_changes_anything` and `an_unknown_control_sequence_changes_nothing` |
| H. `SYNC_HOLD_MS` 150 to 1500 | fails only `the_bounds_are_the_adr_s` (a constant; the hold itself has no test: finding 5) |

Measured, release build on the host (M4), 80 × 24: `CSI 65535 b` (REP) costs **84 ms of CPU per KiB of output**
(200 sequences, 1,600 bytes, 131 ms); `CSI 65535 I` and `CSI 65535 Z` 3.4 and 3.6 ms per KiB; plain text and line
feeds under 0.1 ms per KiB.

## Findings

No must-fix found that is a defect of the code against an accepted ADR's text. Findings 1 to 4 should be settled
before the merge (1 by the architect's decision, 2 to 4 are small).

1. **should-fix (needs the architect's decision before merge).** `crates/zoraya-term/src/parse.rs:601-653` (the new
   private modes), `parse.rs:58-67` and `:120` (character sets, SO), with `Term::restore` (`term.rs:529`) unwired
   until 10c. **What.** Before 10a the parser dropped every private mode, so nothing a program printed could leave the
   shell's prompt unusable. 10a adds the alternate screen, hidden cursor, the scroll region, insert mode, autowrap off,
   reverse screen, DEC line drawing in G0/G1, synchronized output and `OSC 4`, and nothing sends `Restore` yet, so
   whatever a program or a file leaves set stays set for the shell. zosh writes no reset before its prompt (only
   `\x1b[m`-style SGR in the editor). **Scenario.** Measured on the grid: after
   `\x1b[?1049h\x1b[?25l\x1b[8m\x1b(0\x1b[?2026h\x1b[2;3r\x1b[4h` (any `cat` of a hostile or binary file, or a program
   killed while full-screen), the next prompt `~> ls` shows as `·> ┌⎽` on a blank alternate screen, with the cursor
   hidden, insert mode on and every later update held 150 ms; it lasts until something sends `ESC c`. ADR-0332's own
   example ("`cat strange.bin` ... the next prompt works normally (ADR-0331 item 9)") is false on `main` from this
   merge until 10c. **Fix.** Either the architect accepts the window explicitly (a line in `docs/backlog.md` and in
   `docs/design.md`'s known limits, which today say nothing of it), or a stop-gap until 10c. Sending `Restore` needs 10c's
   protocol, so the smallest stop-gap is the shell writing a fixed reset (`\x1b[?1049l\x1b[!p\x1b[?25h\x1b(B\x0f\x1b[m`) before each prompt that follows a
   foreground command, with a host test of the editor's output. That is in-band and a small decision; it is removed by
   10c.

2. **should-fix.** `tools/term-conformance/check.sh:46`. **What.** A capability counts as covered when the text
   `cap("name"`, `flag("name"` or `num("name"` appears anywhere in a `tests*.rs` file, comments included. ADR-0382's
   driver 1 and Option A's rejection are exactly that a comment must not be a cover. **Scenario.** Break A: deleting
   `mc0` from the exceptions and writing `// cap("mc0", &[])` passes `make check`; INV-0332-1 is then met on paper for a
   capability nothing tests. A test deleted while a comment mentioning its call is kept passes in the same way.
   **Fix.** Strip `//` comments (and `///`, `//!`) before the grep (`sed -e 's://.*$::'`), and add a self-test of the
   tool: a fixture file with a commented call that must not count.

3. **should-fix.** `crates/zoraya-term/src/tests_total.rs:269` and `:297` (INV-0332-3's sweep). **What.** The sweep
   skips every sequence for which `csi_known` or `escape_known` returns true, and those are the implementation's own
   functions. The invariant is "a well-formed sequence outside items 2 and 3 changes nothing"; the oracle for "outside
   items 2 and 3" must not be the code under test. **Scenario.** Breaks B and B2: adding `CSI y` (DECTST) or `CSI ? s`
   (XTSAVE) to `csi_known` with an effect passes all 123 tests, the fuzz and the replay. A later edit that makes a
   query or a window operation "known" by mistake is not caught unless it happens to be in the query list of
   `no_query_is_answered_and_none_changes_anything`. **Fix.** An independent table in the test, written from
   ADR-0332 items 2 and 3 (the finals `@ABCDEFGHIJKLMPSTXZbdfghlmr`, `? h`, `? l`, `SP q`, `! p`; the `ESC` finals
   `78DEHMc=>` and `(B (0 )B )0`), and an assertion that `csi_known`/`escape_known` agree with it for every
   (marker, intermediate, final); then the sweep uses the table. Also compare `last` (REP's character) and
   `alt_saved_cursor` in `same_state`: both are state a program can observe.

4. **should-fix.** `crates/zoraya-term/src/parse.rs:562-568` (`REP`), ADR-0380 item 9. **What.** `CSI 65535 b` is 8
   bytes that print 65,535 characters, with a scroll of the whole screen (a `copy_within` of every row) per wrapped
   row. Measured: 84 ms of CPU per KiB of output on the M4 host, about 840 times plain text. `term` parses in its event
   loop, so while it runs no key, close or resize is handled. **Scenario.** A 1 MiB file of `\x1b[65535b` (or a remote
   program over ssh) keeps the terminal window frozen for about 86 s on the host and several times that in the guest;
   Ctrl-C cannot reach the shell because `term` is the one busy. **Fix.** Bound the work, not only the count: clamp
   the repeat to `rows * columns` (beyond a full screen only the scrollback's contents differ, which ADR-0380 can say),
   or fill whole rows directly for a long repeat; record the choice in ADR-0380 item 9 (proposed) and add a test with
   a time-independent bound (for example, a counter of `put` calls or of scrolls under `cfg(test)`).

5. **should-fix.** `user/src/bin/term.rs:234-253` and `:531-548` (the 150 ms hold). **What.** Untested, as the report
   says, and two defects by reading. (a) When the deadline passes while a commit is outstanding (`configure` commits
   during a hold), `show` returns at `if self.outstanding` with `sync_since` still set, so `port_wait` is called with a
   deadline in the past, returns `DeadlineExceeded` at once, and the loop spins until the compositor's `CommitDone`
   (one compositor round: bounded, but a busy loop). (b) A program that sets 2026 and never resets it gets every later
   update held 150 ms, the shell's echo of typed keys included, until `ESC c` (finding 1 makes this persist).
   **Scenario.** (b): `printf '\033[?2026h'` at the prompt makes every key echo arrive 150 ms late for the rest of the
   window's life. **Fix.** Move the decision into `zoraya-term` as a pure function (`hold(now, since, synchronized,
   outstanding) -> (draw, deadline)`), with host tests for: hold then draw at 150 ms, draw at once on reset, no past
   deadline while a commit is outstanding (wait without a deadline instead). Leave (b) to `Restore` in 10c, but write it
   in the known limits.

6. **should-fix.** ADR-0332 item 10 (accepted) asks for recordings of "`vi`, `less`, three ncurses test programs and
   `vttest`'s cursor and screen menus, made ... with `TERM` set as ADR-0333 says". **What.** (a) `vttest` is not
   recorded; the reason (no pin) is in the part's report only, not in ADR-0332's rule-6 note or `docs/backlog.md`. (b)
   `vi-edit` and `less-page` were recorded with the host's terminfo database (`record.py:74` sets `TERM` only; the two
   sessions have no `env TERMINFO=@terminfo@`, which the curses sessions use), so their bytes come from macOS's old
   `xterm-256color`, not the pinned 6.6 entry the terminal claims. Vim was also run with `t_RV= t_u7=`, which removes
   the two queries a real Vim sends (fine for the grid, but the recording then does not exercise INV-0332-2).
   **Scenario.** A sequence that only the pinned entry has and that `less` or `vi` would use under it (for example
   `E3`, `rep`, `Smulx`) is never in a recording; a difference between the two entries is invisible. **Fix.** (a) a line
   in ADR-0332's rule-6 note and in the backlog for the architect's pin of `vttest`; (b) re-record `vi-edit` and
   `less-page` with `env TERMINFO=@terminfo@` and freeze the new grids (a person reviews the diff), and keep one
   recording without `t_RV= t_u7=` so the queries are in a replayed stream.

7. **should-fix.** `crates/zoraya-term/src/parse.rs:385-391` (strings cut at 4,096 bytes), proposed ADR-0380 item 5.
   **What.** A `DCS`, `APC`, `PM` or `SOS` stores nothing, yet it is cut at 4,096 bytes and the rest is printed as
   text. **Scenario.** Any sixel image, kitty graphics `APC _G` payload or tmux passthrough longer than 4 KiB (common:
   tens of KiB of base64) prints kilobytes of garbage over the screen; an over-long `OSC 8` hyperlink prints its tail.
   ADR-0380's own driver 1 (a stuck state is escapable) is already met without the cut: `CAN`, `SUB` and the `ESC` the
   shell's editor writes end the string, and `Restore` (10c) resets the parser. **Fix.** For the architect on ADR-0380:
   keep skipping to `ST`/`BEL`/`CAN`/`SUB` without storing past the bound (an `OSC` past its bound is marked and not
   run), as xterm and st do; the memory bound is unchanged.

8. **note.** `docs/adr/invariants.md`, INV-0331-7 and INV-0332-2 marked `checked`. **What.** INV-0331-7's check
   (`crlf`) tests the converter; the shell calls it with `onlcr` always `true` (`zosh.rs`, `show_bytes`), and the
   relay (the invariant's check names "a host test of the shell's relay") has no host test: removing the conversion
   from `show_bytes` passes every host test (only the GUI scripts would show the staircase). INV-0332-2's second half
   (`term`'s emission points) is 10b's. Both are said in the rule-6 notes, so the status is honest only with its
   parenthesis; ADR-0333's note says the case-sensitive half of INV-0333-1 is checked "by CI", which has not run yet.
   **Fix.** Keep, but write "partly" in both statuses, and change ADR-0333's note to "to be confirmed by CI's
   `check-ports`" until that run is green.

9. **note.** `crates/zoraya-term/entry/exceptions.txt` (INV-0332-1's 188 of 287). **Judgement.** 158 keys, 2 mouse
   strings, 2 paste marks and `xm` are deferrals to 10b, not exceptions by design; 26 are ADR-0332 item 4's own list
   and are right. Using the exceptions file as a to-do list is acceptable for one part, because the tool fails when a
   deferred capability gets a test, but nothing fails if 10b leaves one in. **Fix.** Mark deferrals distinctly (a
   reason beginning `deferred (10b):`), print their count separately, and have 10b's plan row (or `make check` after
   10b) fail on any left.

10. **note.** `crates/zoraya-term/src/read.rs` (the reader "fits any colour") and `tests/gui/terminal.script`.
    **Judgement.** Not weaker for text: a cell still reads as a character only when some foreground reproduces every
    pixel exactly with the rasteriser's blend (`fit`), so a wrong glyph, a cut wide character or noise still fails.
    It is weaker for colour: before, text drawn outside the palette read as `NO_MATCH`; now no colour fault can fail a
    GUI script, and the line `screen red bold blue_background,_yellow_text orange` checks words only. Colour is
    checked by the frozen grids instead (break F fails `tests/replay.rs`), which is the better place. `docs/design.md`
    should say the GUI script reads text only and colours are the replay's.

11. **note.** `crates/zoraya-term/src/screen.rs:130-137` (`attach`). With autowrap off, a narrow character written in
    the last column leaves the cursor there with no pending wrap, so a following combining mark joins column
    `columns - 2`, not the character it follows. Fix: remember the column of the last character put (or treat
    `x == columns - 1` with autowrap off like a pending wrap); a test.

12. **note.** `crates/zoraya-shell/src/editor.rs:151-161` (`advance`). In a grid one column wide the terminal puts a
    wide character in one cell (`screen.rs:72-75`), the editor counts two (`width_tests::a_wide_character_wraps_whole`
    asserts 4 for `"中中"`). Harmless at that width, but it is the one place where editor and grid disagree, against INV-0332-4's
    spirit; count `columns(c).min(width)`.

13. **note.** `tools/term-width/gen.sh:52`: `check` exits 0 when the musl archive is absent (offline only the table's
    first line is compared with the lock), while `terminfo.sh check` exits 2. `check-ports` runs after `fetch-ports`
    in CI, so CI is fine, but a local `make check-term-width` that passes says nothing. Exit non-zero, or print
    `SKIPPED` in the summary. The conformance tool runs inside `check-test-caps` (`Makefile:1590`), so its failure is
    reported under a heap-cap step's name: give it its own `check-*` target.

14. **note.** Facts of the report and `docs/design.md`. There are seven recordings (`vi-edit`, `less-page`,
    `curses-acs`, `curses-attr`, `curses-colors`, `curses-hanoi`, `curses-view`), not "five", from three ncurses
    programs (`ncurses`, `hanoi`, `view`), not "four". Plan row 10a's demonstration is "`cat` of a recorded `vi`
    session"; the GUI script plays `less` (the report does not say why). `term.scrollback` is left to "a part that
    touches settings" without a name: give it a part in the plan or the backlog. ADR-0381 says the cell is "private to
    `zoraya-term` behind accessors", but `Cell` is public with a public `ch` field (`cell.rs:63-64`) and used by `t_text`
    and `read.rs`; the marks are private, which is what Option B's locality needs, so reword the ADR. The history
    `Vec` may hold twice its lines (`memory_bound`); a `reserve_exact` at the first push makes the bound the nominal
    2.5 MB.

## Checked and found sound

- **The parser's states** against ADR-0332 items 1 to 3: `CSI` with at most 32 parameters and sub-parameters counted
  together (`begin_param`, the `overflow` flag ignores the whole sequence); private markers only first; intermediates
  bounded at two; `CsiIgnore` for malformed orders; `CAN`/`SUB` end every non-ground state; `DEL` ignored everywhere;
  raw C1 bytes are U+FFFD, never 8-bit `CSI`; overlong UTF-8, surrogates and values past U+10FFFF are U+FFFD and a
  decoded C1 control is never acted on. `ESC` inside a string: `ESC \` ends it, anything else abandons it.
- **No reply path.** `Term::write` returns `()`, there is no buffer of replies in `Term`, and `term`'s diff adds no
  write toward the shell; every query of the list in `tests_parser.rs:613` (DSR, DA1/2/3, DECRQM, DECRQSS, XTGETTCAP,
  window reports, `OSC 10/11/12/4 ?`, `OSC 52`, titles, `ENQ`) is consumed and changes nothing; `OSC 4` with `?` sets
  nothing. `OSC 0/1/2/7/52/133` are ignored; titles reach the window only from the shell's out-of-band `Title`
  (ADR-0168). `CSI > 4;2 m` (Vim's modifyOtherKeys) does not underline.
- **Bounds.** Every index in `graphics`, `semicolon_colour` and `colon_colour` stays below `nparams`; `OSC 4` indices
  are four digits at most and `< 256`; `parse_spec` cannot overflow; every count (`CUF`, `ICH`, `DCH`, `ECH`, `IL`,
  `DL`, `SU`, `SD`) is clamped to the grid; `resize`, `set_scrollback`, `restore` and `reinstate` keep the cursor,
  region and view inside, as the fuzz checks after each of them. The memory bound holds under a megabyte of each
  unterminated string kind.
- **Wide characters.** `put`, `unwide`, `fix_cells` (after every edit and on resize, history included) keep the pair
  invariant; the fuzz checks it on both screens and the history, and break C shows it fails when broken.
- **The fuzz** (`tests_total.rs`) checks real invariants (cursor, region, view, history length, grid sizes, memory,
  pairs, and that `CAN` frees a stuck state), over six sizes including 1 × 1, with restore, resize, scrollback changes
  and rendering mixed in; breaks C and D fail it.
- **CR/LF.** Every byte the shell sends to `term` goes through `show_bytes` and one `Crlf` state, which is the last
  byte written, so a `\r` at a chunk's end is respected; `ttyd` applies `ONLCR` under `OPOST` itself
  (`zoraya-tty` `output`), so the POSIX session in a window is unchanged; the editor's `\n` gets its `\r` and draws as
  before; the editor's `printable` maps every width −1 character to `?`, one column, as the grid counts it.
- **Width table.** Generated from the pinned musl 1.2.5 archive (SHA-256 checked) by compiling its own `wcwidth.c`;
  every code point compared in `zoraya-width`'s tests with the ASCII shortcut; the grid and the editor use the same
  `columns`.
- **Pins and licences.** ncurses 6.6 in `ports/sources.lock` with two mirrors and one SHA-256; X11 licence; host tools
  listed in `docs/dependencies.md` (ncurses, musl's `wcwidth.c`, Python standard library, tmux); nothing enters the
  image. The manifest names exactly the four files; `vt100-am` is removed after `tic`.
- **Unsafe.** None added; `zoraya-term` and `zoraya-width` are `#![forbid(unsafe_code)]`.
- **Recordings and paths.** No host path, user or host name or e-mail address in the recordings, the GUI root's
  recordings or the recording tools (scan above), and `make check-host-paths` passes; `lrtest`, whose output held an
  address, was replaced by `view`.

## Recommendation

Merge after findings 2, 3 and 4 are fixed (each is small and local) and the architect has decided finding 1 (accept
the window until 10c in writing, or the stop-gap reset). Findings 5 to 7 can go with the merge if they are recorded as
open items for 10b and 10c; 8 to 14 are corrections of text and small tests.

## Check of the fix pass

A check of the changed lines of `git diff 67b35e5 8757e88` on `work/10a` (the fix pass `a99661f`, the candidate fixes
`cb5d881` and `8757e88`; the merge of `origin/main` left out), by the same reviewer. Read only: nothing was built or
run in this check, so every statement below is by reading the code at `8757e88`, and the arithmetic is shown.

### Findings 1 to 7: fixed?

- **1 (stop-gap reset): fixed, with the notes C4.** `crates/zoraya-shell/src/reset.rs:14` is a constant
  (`ESC[?1049l ESC[!p ESC[?25h ESC(B SI ESC[m`); program output cannot change it. `zosh.rs:5064-5073` pushes it into the
  same `out` as the prompt, so reset and prompt are one `Output` message, only when `terminal()` is a window. It has no
  line feed, so INV-0331-7 is untouched. It works from any state the program left the parser in: in a string the `ESC`
  abandons it and `[` starts a `CSI` (`parse.rs:185-195`), mid-`CSI` or mid-escape the `ESC` restarts. `leave_alt` is a
  no-op off the alternate screen (`screen.rs:450`), and `soft_reset` keeps the cursor's place (`screen.rs:466`), so
  after a command that printed plainly nothing moves. The host test covers the hostile case.
- **2 (comments as covers): fixed.** `check.sh` strips `//` before the grep. `selftest` exercises the same `covered_in`
  and runs in `make check`. See C5.
- **3 (independent sweep table): fixed.** `ADR_CSI`/`ADR_ESC` (`tests_parser.rs`) match ADR-0332 items 2 and 3 as
  written (checked against the ADR's list, final by final). `the_parsers_lists_are_the_adr_s` makes `csi_known` and
  `escape_known` agree over five markers, seven intermediates and every final, and the sweep uses the table.
  `same_state` compares `last` and `alt_saved_cursor`. Breaks B and B2 now fail by construction.
- **4 (REP bound): the cost is bounded, but the result is wrong; see C1.**
- **5 (hold): fixed.** `sync_hold` is pure and tested, including "no deadline while outstanding" and "a deadline is
  never in the past". `term`'s loop calls `show()` on every iteration (after each wake, deadline or packet), so a hold
  that ran past its deadline during a commit is drawn at the `CommitDone` wake. No spin is left.
- **6 (recordings): half fixed; see C3.** `vttest` is open in ADR-0332's note and the backlog. `vi-edit` was re-recorded:
  its queries are now in the stream (`ESC[>c` once, `ESC[6n` twice, 2,659 bytes against 2,590). `less-page` was not.
- **7 (strings swallowed whole): fixed.** `string()` keeps at most `MAX_STRING` bytes of an `OSC`, sets `osc_over`, and
  `finish_string` does not run an overrun `OSC`. Every exit path clears `osc_over` (`abort`, `escape`, `finish_string`).
  Nothing of a string is printed. (b) A string ends only at `ST`, BEL (for an `OSC`), `CAN`, `SUB` or an `ESC` that
  starts something else, so an unterminated one swallows plain text for as long as plain text keeps coming. It cannot
  swallow the prompt: the stop-gap's first byte is `ESC`, and the editor's redraw after output that arrived while
  editing begins with `\r\x1b[J` (`editor.rs` `hide`). The one exception is the first prompt of a session (no reset)
  after a `START` script that left a string open, when the prompt has no escape. That is negligible.
- **(d) terminal script.** Reading only `line 60`, `line 70`, the colour row and `(END)` does not touch INV-0332-6: that
  invariant's check is `tests/replay.rs` at the recording's own 80 × 24, not the GUI script. The script is now a smoke
  test of a 24-row stream drawn into a grid that may have fewer rows (the cursor addressing is clamped), and it says
  so. `cat …; sleep 300` with `chord ctrl c` also exercises the stop-gap in QEMU (`screen ~> _` after the interrupt).
- **(e) the reader / runner test.** `gui.rs:1677` now writes `\r\n`, as the shell does under ADR-0332 item 2. The test
  is as strict as before. No other host user of `Term::write` feeds a bare `\n` (only `term.rs` and this test construct
  a `Term` outside the crate).

### New findings

C1. **should-fix.** `crates/zoraya-term/src/parse.rs:573`, with `tests_parser.rs:1543` (`rep_is_bounded_by_the_screen`),
    proposed ADR-0380 item 9. **What.** `n.min(rows * columns)` changes the result of the repeat, not only the
    scrollback. The final cursor column depends on `n mod columns`, and the clamp drops that. The top visible row's
    prefix also comes from the original contents instead of the repeated character. Finding 4's own suggestion ("beyond
    a full screen only the scrollback's contents differ") was wrong, and the fix followed it. **Scenario.** 80 × 24,
    `a ESC[65535b`: 65,536 `a`s end at linear position 65,535, column 15. The bottom row should hold 16 `a`s with the
    cursor at column 16. The clamp gives 1,921 `a`s: the bottom row is `a`, the cursor is at column 1, and every later
    character lands 15 columns off. The new test asserts `line(23) == "a"`, which freezes the divergent state. **Fix.**
    Keep the period. With `full = rows * columns + columns`, when `n > full` repeat
    `full + (n - full) % p` times, where `p = columns` for a narrow character, or with autowrap off. For a wide
    character `p = max(1, columns / 2)`, because a row holds `columns / 2` wide characters and an odd last column is
    padded. Assert 16 `a`s and cursor `(16, 23)`, and add a check that compares `CSI n b` with `n` plain `put`s for
    several `n` above the bound on small grids (both narrow and wide, autowrap on and off, inside a region). Reword ADR-0380 item 9 to
    "the work is bounded by a screenful; the result is that of the full repeat but for the scrollback".

C2. **should-fix.** `crates/zoraya-term/src/term.rs:453-460` (`memory_bound`) with `term.rs:280-297`
    (`set_scrollback`). **What.** The fix pass dropped the doubling slack from the bound because `push_history` now
    calls `reserve_exact`. `set_scrollback` still rebuilds the history with `Vec::new()` and `extend_from_slice`, line
    by line, so it grows by doubling, and the history can no longer reach the reserve in `push_history` to correct it
    once `kept == capacity`. **Scenario.** `Term::with_scrollback(80, 24, 2000)`, more than 2,000 lines output,
    `set_scrollback(1500)`. 1,500 lines are 120,000 cells. The vector's capacity goes 80, 160, … 81,920, 163,840 cells.
    `memory()` then exceeds `memory_bound()` by 43,840 × 16 − 4,096 ≈ 697 KB. With an `OSC` buffer already at 4 KiB
    the slack is zero, and any shrink to a non-power-of-two line count breaks the bound. The fuzz (`set_scrollback`
    up to 11 lines on small grids) stays inside the `MAX_STRING` slack, which is why it passes. `term` does not call
    `set_scrollback` yet; 10c will (`term.scrollback`). **Fix.** In `set_scrollback`, start from
    `Vec::with_capacity(lines.min(old.len()) * columns)` and `reserve_exact` up to `lines * columns`, as `resize` does
    with its `with_capacity`. Add a test: fill 2,000 lines, shrink to 1,500 and to 3, and assert
    `memory() <= memory_bound()` with the `OSC` buffer full first.

C3. **should-fix.** `crates/zoraya-term/recordings/less-page.session` (the added `env TERMINFO=@terminfo@`) and the
    report's row 6 ("`vi-edit` and `less-page` re-recorded … with the pinned entry"). **What.** `less-page.rec`,
    `.grids` and `.marks` (and the GUI cut `less-end.rec`) are unchanged since `45cdd3f`. The pinned entry's `smcup` is
    `\E[?1049h\E[22;0;0t` (`entry/xterm-256color.caps`). The pinned-entry sessions carry `ESC[22;0;0t` (`curses-acs.rec`
    does). `less-page.rec` has `ESC[?1049h` alone, which is the host's entry. The session file now describes a
    recording that is not the committed one, and the next `record.py` run will change the bytes and the GUI cut.
    **Fix.** Re-record `less-page`, re-freeze its grids (a person reviews the diff), and re-cut
    `tools/term-record/gui-files.sh write`. Alternatively, remove the line, say in the report that `less` was not
    re-recorded, and keep the finding open.

C4. **note (the stop-gap's edges, for the architect; none blocks).**
    (a) It fires before every prompt after the first (an empty line, a builtin, a Ctrl-C at the prompt), not only
    after a foreground command as `reset.rs`, the backlog and the report say. It is harmless (idempotent, the screen is
    kept), but say so.
    (b) `soft_reset` clears a pending wrap (`screen.rs:476`). After output that fills the last column without a newline
    (`printf '%80s' x`), the prompt's first character now overwrites that column, where before it wrapped to the next
    row.
    (c) It is not `Restore`: the palette (`OSC 4`) is kept on purpose, while Restore clears it (ADR-0380 item 10). A
    file that sets entries 1 to 15 to the background colour leaves coloured output (`ls`, syntax colours) invisible
    until `ESC c`. `ESC]104 BEL` (bare: all entries, `parse.rs:443-455`) would close that gap, at the price of a
    palette theme set from the shell. Either way, write the choice next to the constant.
    (d) A cursor shape a person sets at the prompt (`printf '\e[5 q'`) is undone at the next prompt, as Restore will do.
    (e) A job stopped with Ctrl-Z loses its modes (alternate screen, DECCKM, a hidden cursor, the saved cursors), and
    `fg` gives nothing back. A native element is `job_suspend`ed and cannot redraw. A POSIX one gets `SIGCONT` and
    usually re-inits. That is INV-0331-4's second half (`reinstate`, 10c). Until then the backlog line should say that
    `fg` of a full-screen job is not restored.
    (f) Output from a background job while the editor is active is not followed by a reset.

C5. **note.** `tools/term-conformance/check.sh` `covered_in`: a call inside `/* … */` still counts as a cover, and a
    `//` inside a string literal drops a real call after it on that line (this fails safe). Consider stripping block
    comments too, or require the call to be at the start of a statement.

C6. **note (text).**
    - ADR-0332's new rule-6 note says "four ncurses sessions"; there are five (`curses-acs`, `-attr`, `-colors`, `-hanoi`,
      `-view`).
    - The backlog's 2026 line says "`Restore` (10c) ends it". The stop-gap's `ESC[!p` already ends it at the next prompt,
      as `docs/design.md` says, so the hold lasts until the command ends.
    - `rep_is_bounded_by_the_screen` says "a thousand of them" and loops 100 times.

### Recommendation of the check

Findings 2, 3, 5 and 7 are fixed. Finding 1 is fixed, as a stop-gap with the edges in C4. Finding 6 is fixed for `vi`
and `vttest`, but not for `less` (C3). Finding 4's fix bounds the cost but changes the result (C1), and the tightened
memory bound is now false after `set_scrollback` (C2). C1 and C2 are small and local to `zoraya-term`, with host
tests. C3 is a re-record, or a correction of the session file and the report. **Merge after C1 and C2 are fixed and C3 is
re-recorded or truthfully stated**; C4 to C6 can go with the merge as text (C4 (c) is the architect's call).
