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
| `cargo test -p zoraya-term` | 97 unit tests and 2 replay tests pass (the capability tests, the parser tests, the fuzz of totality, the unknown-sequence sweep over about 12,000 sequences, the replay of five recordings) |
| `cargo test -p zoraya-width` | 5 pass (the width of every code point against the runs, the ASCII shortcut against the table) |
| `cargo test -p zoraya-shell` | 154 pass (`crlf`: the three cases of INV-0331-7 and chunk boundaries; the editor's wide characters) |
| `make check` | all 22 steps pass (fmt, clippy `-D warnings`, adr-check with 0 errors, `check-test-caps` now also running the conformance tool and the check of the GUI recordings, `check-test-lanes`, ...) |
| `make check-term-width` | the table is musl's `wcwidth` for every code point (regenerated from the pinned archive and compared) |
| `make check-terminfo` | the four entries recompiled by the pinned `tic` equal `ports/terminfo.manifest`; the dump equals the pinned entry |
| `tools/term-conformance/check.sh` | 287 capabilities: 99 covered by tests, 188 excepted with a reason (the keys, the mouse and paste marks are part 10b's) |
| `python3 -I tools/term-record/compare-tmux.py` | 30 of 31 marks agree with tmux in text; the one that differs shows tmux's own display of the ACS set |
| `GUI_RUNS=terminal:auto:1 make test-gui` | PASS, 100 steps (HVF, scale 1), after two scripts' faults found by running it: a highlight left on the screen by `less`'s search, and a scroll that moved the rows the script looked below |
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
