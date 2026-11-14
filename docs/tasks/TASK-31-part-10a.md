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
