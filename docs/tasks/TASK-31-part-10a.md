# TASK-31, part 10a: the terminal's parser and grid

Branch `work/10a` (from `origin/main` 0c3ada9). Written by the implementation session of part 10a (a Sonnet agent);
the independent review is `opus-review`'s, afterwards. Nothing here is merged, tagged or pushed to `main`.

## What exists

| What | Where |
|---|---|
| The parser and grid of ADR-0332 items 1 to 3, 6, 9 and 11: `CSI` with 32 parameters and sub-parameters, `OSC 4` and `104`, strings skipped, the alternate screen, scroll regions, tab stops, two character sets, wide and combining characters, 256 and direct colours, attributes, `Restore` and `reinstate`, scrollback as a setting (`Term::with_scrollback`, `set_scrollback`) | `crates/zoraya-term/src/{term,screen,parse,cell,colour}.rs` |
| The 16-byte cell | `crates/zoraya-term/src/cell.rs` (proposed ADR-0344) |
| Drawing: italic slanted, underline, strike, wide cells over two columns, combining marks, the three cursor shapes, reverse screen, `OSC 4` palette | `crates/zoraya-term/src/lib.rs` (`Look`), `term.rs` (`render`), `crates/zoraya-text` (`draw_glyph_over`) |
| The reader of screens by glyph takes any foreground and background (it fits the colour from the coverage), not only the palette's | `crates/zoraya-term/src/read.rs` |
| The width table from the pinned musl's `wcwidth`, used by the grid and by `zosh`'s editor | `crates/zoraya-width`, `tools/term-width/gen.sh`, `crates/zoraya-shell/src/editor.rs` |
| The carriage return written by the shell | `crates/zoraya-shell/src/crlf.rs`, `user/src/bin/zosh.rs` (`show_bytes`) |
| Synchronized output held for 150 ms | `user/src/bin/term.rs` |
| ncurses 6.6 pinned; `tic` and `infocmp` built for the host; the four entries and their manifest | `ports/sources.lock`, `tools/term-terminfo/terminfo.sh`, `ports/terminfo.manifest` |
| The conformance tool, the dump of the entry and the exceptions file | `tools/term-conformance/check.sh`, `crates/zoraya-term/entry/` |
| Recorded sessions (`vi` and `less` of the host, four ncurses test programs of the pinned release), their frozen grids and the replay | `tools/term-record/`, `crates/zoraya-term/recordings/`, `crates/zoraya-term/tests/replay.rs` |
| The GUI script plays a recorded full-screen session | `tests/gui/terminal.script`, `tests/gui/terminal.root/recordings/` |
| Three proposed ADRs | `docs/adr/0343-*.md`, `0344-*.md`, `0345-*.md` |

## What was run, and what it gave

All on the Mac mini (M4, 16 GB), `CARGO_BUILD_JOBS=3`, under HVF, in the worktree of the branch. Nothing was run that
the task forbids (`make test`, `make test-par`, the candidate CI).

| Command | Result |
|---|---|
| `cargo test -p zoraya-term` | 123 unit tests and 2 replay tests pass (the capability tests, the parser tests, the fuzz of totality, the sweep of every unknown `CSI`, `ESC`, `OSC` and string sequence (more than 5,000 a screen), the replay of five recordings) |
| `cargo test -p zoraya-width` | 5 pass (the width of every code point against the runs, the ASCII shortcut against the table) |
| `cargo test -p zoraya-shell` | 155 pass (`crlf`: the three cases of INV-0331-7 and chunk boundaries; the editor's wide characters) |
| `make check` | all 22 steps pass (fmt, clippy `-D warnings`, adr-check with 0 errors, `check-test-caps` now also running the conformance tool and the check of the GUI recordings, `check-test-lanes`, ...) |
| `make check-term-width` | the table is musl's `wcwidth` for every code point (regenerated from the pinned archive and compared) |
| `make check-terminfo` | the four entries recompiled by the pinned `tic` equal `ports/terminfo.manifest`; the dump equals the pinned entry |
| `tools/term-conformance/check.sh` | 287 capabilities: 99 covered by tests, 188 excepted with a reason (the keys, the mouse and paste marks are part 10b's) |
| `python3 -I tools/term-record/compare-tmux.py` | 30 of 31 marks agree with tmux in text; the one that differs shows tmux's own display of the ACS set |
| `GUI_RUNS="terminal:auto:1 terminal:auto:2" make test-gui` (rerun at the final commit, after the mutation tests changed `zoraya-term`) | PASS, 100 and 100 steps (HVF); the first passing run was at scale 1, after two scripts' faults found by running it: a highlight left on the screen by `less`'s search, and a scroll that moved the rows the script looked below |
| `GUI_RUNS="terminal:auto:2 windows:auto:1 wheel:auto:1" make test-gui` | PASS: 100, 63 and 79 steps |
| `make test-qemu` | PASS: 119 passed, 9 expected failures (`t_text`, `t_gui` and the rest of the kernel and user tests build and run with the new `zoraya-term` and `zosh`) |

**Not run, and why.** `make test-changed` as a whole: the change touches `ports/` (the lock, the Makefile), which
the map sends to every port step (`test-apps`, `test-cxx*`, `test-netsurf`, `test-python*`), and those need `make
fetch-ports` and builds of LLVM, curl, CPython and NetSurf that do not fit this machine and this session; the steps
that cover the changed code were run by name (the rows above). `make check-ports` for the same reason (it needs the
whole sysroot); its three new checks (`check-term-width`, `check-terminfo`, `tools/term-conformance/check.sh pin`)
were run on their own. The TCG runs of the GUI scripts and the 1600 x 1000 and 1920 x 1200 sizes were not run
(CLAUDE.md: CI does them). The terminfo manifest was written and checked on a case-insensitive host only; the
case-sensitive half of INV-0333-1 is CI's `check-ports` on Linux.

### Mutation testing (`make mutants-host`, `--in-diff`, under the watchdog)

`make mutants-host CRATE=zoraya-term MUTANTS_HOST_ARGS="--in-diff build/mutants/10a.diff -f **/parse.rs -f **/screen.rs
-f **/term.rs"` (the diff is `git diff origin/main` of `crates/zoraya-term/src` and `crates/zoraya-width/src`):

| Run | Mutants | Result |
|---|---|---|
| 1: `parse.rs`, `screen.rs`, `term.rs` | 1,002 (about 28 minutes, 2 jobs) | 860 caught, 124 missed, 7 timed out, 11 unviable |
| 2: the survivors' kinds, after the first tests added | 151 | 22 missed |
| 3: `resize`, `restore_cursor`, `render`, `csi`, `reinstate`, after the second tests | 62 | 0 missed |
| 4: `zoraya-shell`'s `crlf.rs` and `editor.rs` | 57 | 7 missed; the fixes are tests of `Crlf::after` and `ended_with_cr`, of width-1 rows in the editor, and the removal of a `reserve` (a capacity, not behaviour) |

The 124 were sorted by hand (the `mutant-triage` agent's way: read the mutated line, decide whether any behaviour
differs). The holes were tests missing for: `DEL`, `ESC` and controls in each parser state; a parameter, a
separator or a private marker after an intermediate or after a parameter; the end and the bound of an ignored `CSI`;
control characters inside an `OSC`; the number syntax of `OSC 4` and `104`; `CNL` and `CPL`; `CSI T` with several
parameters; the ANSI and DEC modes, `2026` among them, and a sub-parameter where a mode is; wide characters at the
last columns and in two-column grids; erase modes 1 and 2; `IL` and `DL` on the region's first and last rows; scrolls
past a region; the cursor's limits above, inside and below a region; `same_state`'s every field; the memory bound's
arithmetic; the clamps of resize and `reinstate`; a full reset's tab stops; the three cursor shapes over a wide
character and the block over a narrow one. Each got a test. Dead code the run showed was removed (`was_osc`, an
unused `cell_has`, two assignments in `resize`, a redundant clamp, a `0x1b` arm that did nothing).

**The survivors that are equivalent** (ten, not killable, each reasoned): `ground`'s `utf8 << 6 | x` and `sgr_code`'s
`!(BOLD | FAINT)` and `put`'s `attrs | WIDE` and `attrs | TAIL` with `|` read as `^` (the bit fields are disjoint, so
`|` and `^` agree); in `unwide` and `attach`, `x > 0` as `x >= 0`, `x + 1 < columns` as `<=` or `x * 1`, and the
background taken from `at / 1` or `at * 1` instead of its neighbour (a right half in column 0 and a left half in the
last column cannot exist, and both halves of a character are written with one background).

**Not mutation-run**, for the 45-minute cap: `cell.rs`, `colour.rs` (each has its own unit tests), `lib.rs` (`Look`'s
drawing, covered by the pixel tests and the GUI scripts) and `read.rs` (covered by the reader tests and the GUI
script), and `zoraya-width` (a table and three functions, tested against every code point).

## Decisions taken (no one could be asked)

- **Where the ADRs were silent**, three proposed ADRs with a recommendation each: 0343 (the parser's corners), 0344
  (the cell's layout), 0345 (how conformance is evidenced). Their numbers are from the task's reservation.
- **A generated table, committed, with a check.** ADR-0332 item 9 says the table is generated at build time from
  musl's `wcwidth`. The table is a Rust source file in the repository, written by `tools/term-width/gen.sh`, and
  `gen.sh check` regenerates it from the pinned archive and compares (also that the file names the lock's pin, offline).
  A build that needs the C compiler and the archive to compile `zoraya-term` was the heavier choice, and host tests
  would then need the ports' cache. Reversible: only the generation moves.
- **`tic -x`** (the extensions `Ms`, `XM`, `Smulx` and others are in the pinned entry's 287 capabilities), and the
  alias `vt100-am` that `tic` writes beside `vt100` is left out, so that the four files are exactly ADR-0333's.
- **Two mirrors** in `ports/sources.lock` for ncurses (ftp.gnu.org and invisible-mirror.net, the same SHA-256); the
  detached signature is not verified.
- **The conformance dump** is `infocmp -x -1` of the compiled files, sorted; a capability is covered by a call of
  `cap`, `flag` or `num` (proposed ADR-0345), not by a comment.
- **Until part 10c**, the shell treats a POSIX program's output as `OPOST` and `ONLCR` on (`show_bytes` converts
  everything for the window), and the session's `ttyd` is unchanged (it applies `ONLCR` itself).
- **`term.scrollback` as a setting** has its API (`Term::with_scrollback`, `set_scrollback`, at most 10,000) and
  the default 2,000; declaring it and making `term` read it needs the settings protocol and is left for a part that
  touches settings (see the limits).
- **`Cargo.toml` and `Cargo.lock` map to `check`** in `tools/test-map.txt`, and `user/src/bin/t_text.rs` to
  `test-qemu` (unmapped paths run every step).
- **`vttest` was not recorded.** It is not pinned in `ports/sources.lock`, and fetching and running an unpinned
  program was refused by the session's own safety check. The task's list says "`vttest`'s cursor and screen menus": that
  is open (below). ncurses' `view`, `hanoi` and `ncurses` (attributes, ACS, colours) are in its place, with `vi` and
  `less`.

## Proposed ADRs

| ADR | Recommendation |
|---|---|
| ADR-0343, the terminal's corners that ADR-0332 leaves open | Option B: the ten small choices written down (a line feed keeps the column, only a whole-screen scroll feeds the scrollback, a region needs two rows, a wide character wraps whole, more than 32 parameters ignore the sequence, `ESC` in a string abandons it, `Restore` and `reinstate`), each with a host test |
| ADR-0344, the terminal's cell | Option A: 16 bytes, two BMP marks, nothing per cell on the heap; a mark outside the plane and a third mark are dropped; the cell is private, so Option B (a side table) is a local change |
| ADR-0345, how conformance is evidenced | Option B: the tests expand the dump of the pinned entry, so a call is the cover; recordings of host programs with frozen grids; tmux as a hand-run second opinion |

None changes an accepted ADR: the changes that `rows-to-add.md`'s tensions table lists (LF keeps the column, 32
parameters, the scrollback as a setting, the editor's widths) are made as decided, with rule-6 notes on ADR-0331 to
ADR-0333 and new statuses in `invariants.md` (INV-0331-7, INV-0332-1, -2, -3, -4, -6, INV-0333-1).

## Limits and unfinished

- **`term.scrollback` is not declared or read**: the default (2,000) stands. `Restore` and `reinstate` exist and are
  tested, and nothing sends them (part 10c). The key table, the mouse, the paste, the selection and the focus
  messages are part 10b; the conformance exceptions file lists 188 capabilities for that reason.
- **The sync hold (150 ms) is in `term` and was not exercised by a test** (nothing sends mode 2026 in the GUI scripts);
  `Term` stores the mode and has a host test.
- **A combining mark outside the Basic Multilingual Plane, and a third on a cell, are dropped** (proposed ADR-0344).
- **Wide characters are drawn as boxes** (no font has them); the box spans the two cells.
- **Italic is a shear of the regular face, and an underlined or slanted cell is not matched by the screen reader**
  (the GUI scripts read plain text; the recordings' text rows were not underlined for that reason).
- **`INV-0332-2`'s second half** (the only messages `term` emits toward the shell come from keys, the pointer, a
  paste or a size) is `term`'s binary, which has no host test; 10b adds the pointer and the paste.
- **`INV-0333-1` was checked on a case-insensitive host**; the case-sensitive one is CI's `check-ports`.
- **`make check-ports`, `make test-changed` as a whole, the TCG runs and the large-display GUI runs were not run
  here** (above).
- **The recordings are of the host's `vi` (Vim 9.1) and `less` 668 and of ncurses 6.6's test programs.** Re-recording
  changes the bytes; the grids are frozen and a re-recording's diff is for a person to review.
- **No xterm on this machine** (see the hand-over).

## Hand-over for the architect

1. **The first comparison of ADR-0332 item 10 (not done: no xterm here).** What is here: the replayed grids as
   pictures, `docs/tasks/TASK-31-part-10a-shots/` (`zoraya-vi-edit-2.png`: `vi` in insert mode after typing, with its
   reverse status line and blue tildes; `zoraya-curses-attr-1.png`: every attribute of the entry on one screen
   including the slanted italic and the invisible row; `zoraya-curses-colors-1.png`: the 256-colour matrix;
   `zoraya-less-page-3.png`: `less`'s second page; scale 2, the light set). What is missing is the same recording in xterm
   (XQuartz's `xterm` is not installed). To make the other half: in `xterm -geometry 80x24 -fa 'JetBrains Mono'` run
   `TERM=xterm-256color cat crates/zoraya-term/recordings/vi-edit.rec` (the bytes up to the first mark are
   `head -c 1728`; the marks are in `NAME.marks`), and look at the picture beside ours. `ZORAYA_TERM_SHOTS=dir cargo
   test -p zoraya-term --test replay -- --ignored shots` makes all pictures (`ppm2png.py` turns them into PNG). A text
   cross-check was done instead, with tmux as the other emulator: 30 of 31 marks agree row for row
   (`python3 -I tools/term-record/compare-tmux.py`), the exception being tmux's own display of the ACS set.
2. **Open: `vttest`** (the cursor and screen menus), which the task's list names; it needs a pin in `ports/sources.lock`
   (an architect's decision) and then `record.py` records it like the rest.
3. **Check `make check-ports` once** on the machine that builds the ports (the three new checks run offline from the
   ports' cache, `ncurses` and `musl` archives only), and that CI's Linux run agrees with `ports/terminfo.manifest`.
4. **`ADR-0332`'s exceptions file** is the list part 10b shrinks; the tool fails when a capability is both tested and
   excepted.
5. **The decision on the proposed ADRs 0343 to 0345** and the index rows I added for them (status `proposed`).

## Time

About two hours of wall time on the Mac mini (one Sonnet session): reading and the width table, the parser and grid,
the terminfo and conformance tools, the recordings and the GUI script (three runs of the script to get it passing),
the mutation runs (about 40 minutes of the two hours) and this report.
