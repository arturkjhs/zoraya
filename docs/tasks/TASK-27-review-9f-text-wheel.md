# TASK-27 review of 9f-text and 9f-wheel

An independent review by the `opus-review` subagent, which did not write the code, of two branches before they merge
to `main`:

- `git diff origin/main...origin/work/9f-text` (head `de83a81`): ADR-0326 (accepted), proposed ADR-0327,
  `crates/zoraya-fontset`, `crates/zoraya-posix/src/ffi/text.rs`, `zoraya_window_scale`, the fonts grant in
  `crates/zoraya-shell/src/grants.rs` and `user/src/bin/zosh.rs`, `ports/netsurf/font_zoraya.c`, patch 0008,
  `tools/run-disk.sh`, the runner's `ink` step, the `zoraya-ar` NOBITS fix.
- `git diff origin/main...origin/work/9f-wheel` (head `858eef0`): proposed ADR-0329, `Event::Wheel`, `WheelSteps`,
  `wheel_rows`, the rebuilt `ScrollView`, `replace_keeping_scroll`, the wheel in `zoraya-app`, the file manager, the
  editor, settings and the chooser.

The code read around the diffs: `crates/zoraya-text/src/lib.rs` (`GlyphCache`, `Simple::shape`, `clamp_px`),
`raster.rs` (`Bitmap`), `crates/zoraya-posix/src/ffi/window.rs` (`after_fork_in_child`), `crates/zoraya-posix/src/lock.rs`,
`grants.rs` (`through_links`, `merge`, `window_pills`, `trusted_description`), `zosh.rs` (`posix_plan`, `plan_with`,
`posix_entries`, `add_room_grants`), `tools/zoraya-ar/src/elf.rs`, `zoraya-ui`'s `Window`, `hit`, `paint_tree`,
`focus_ring` and every widget's `event`, `zoraya-files`' `follow` and `paint`. Checked against ADR-0326, ADR-0327,
ADR-0329, ADR-0317 (INV-0317-1), ADR-0318, ADR-0322 (`WHEEL_MAX`), ADR-0162 and ADR-0268 (the untrusted gate).

The reviewer ran, in a worktree with its own target directory, `CARGO_BUILD_JOBS=3`:
- on `work/9f-text`: `cargo test -p zoraya-fontset -p zoraya-ar -p zoraya-shell` and `cargo test -p zoraya-posix`:
  all pass. `make adr-check`: 0 errors. `sh tools/check-untrusted-gate.sh`: ok, the refusal in place.
- on `work/9f-wheel`: `cargo test -p zoraya-ui -p zoraya-app -p zoraya-files -p zoraya-editor -p zoraya-chooser
  -p zoraya-settings-window`: all pass. `make adr-check`: 0 errors. `check-untrusted-gate.sh`: ok.
- Four scratch tests, not committed. They reproduce finding T1, measure T3 and T4, and reproduce W1.

There was no QEMU run, so the GUI scripts were read and not run. Dispositions are left to the main model.

Severity: **must-fix** blocks the merge, **should-fix** before the stage tag, **note** to record or decide.

## 9f-text

| # | Severity | Where | Finding |
|---|---|---|---|
| T1 | should-fix | `tools/zoraya-ar/src/elf.rs:48-50`, `:74` | The NOBITS fix brings back a panic. The module says "Total: no input panics". Reproduced. |
| T2 | should-fix | `crates/zoraya-posix/src/ffi/text.rs:111-113` | A fork child keeps a `Fontset` that another thread may have been changing in the middle. |
| T3 | should-fix | `crates/zoraya-fontset/src/lib.rs:163-166`, `crates/zoraya-text/src/lib.rs:238` | A page sets the glyph size, and the glyph caches hold up to about 400 MB. Measured. |
| T4 | should-fix | `crates/zoraya-fontset/src/lib.rs:220-262`, `:289-313` | Every call shapes the whole text, so laying out a long paragraph takes quadratic time. Measured. |
| T5 | should-fix | `tests/gui/netsurf.script:23-24` | The check for the new pill passes when the pill is missing. |
| T6 | note | `docs/adr/0326-…md` INV-0326-1, `grants_tests.rs:766-790` | The invariant says "nothing gives `/system/fonts` but a `window` line". `+r /system/fonts` and room grants also give it. |
| T7 | note | `ffi/text.rs:69-84`, `:201-216`; `font_zoraya.c` | `read` returning `EINTR` loses a font for the whole process. `zoraya_text_metrics` is exported but not called, and it writes through its pointers without a NULL check. |
| T8 | note | `font_zoraya.c:47`, `zoraya-fontset/src/lib.rs:130-132` | At scale 3 the dpi is clamped to 250, so the page is drawn at 2.78 times, not 3. |
| T9 | note | `ffi/text.rs`, ADR-0326 item 4 | Nothing tests the FFI layer. Nothing says what a future untrusted room's NetSurf gets. |

### T1 (should-fix): `zoraya-ar` panics on a symbol table linked to a NOBITS section

`section()` no longer checks that a `SHT_NOBITS` section's range is inside the file. `defined()` takes the string
table through `section(elf, link)` and then indexes the file directly: `&elf[str_off..str_off + str_size]` (line 74).
A malformed object can make that string table a NOBITS section with a size past the end of the file. Before the
fix, `section()` returned `None` there, so the object gave no symbols. Now it panics with "range end index out of
range" at `elf.rs:74:26`. A scratch test reproduced this: it took `object(&[("x", GLOBAL, 1, 1)])`, made section 3
type 8 with size `1 << 20`, and called `defined_globals`.
**Fix:** make `section()` give the file range only for sections that occupy the file, and have `defined()` refuse a
NOBITS string table. Or use `elf.get(str_off..str_off.checked_add(str_size)?)?` at line 74. Add the scratch test
above to `tests.rs`.

### T2 (should-fix): the fork child keeps a half-changed `Fontset`

`after_fork_in_child` resets `TEXT` and keeps `SLOT`, on the grounds that "the fonts are the parent's bytes, so they
stay". But the slot holds more than the leaked font bytes. It holds four `Text` values whose `GlyphCache` and
`Vec`s change on every call (`slots[first] = Some(..)`, `take()`, the reallocations in `line()`). Suppose thread A
is inside `zoraya_text_glyphs`, in the middle of inserting into the cache or growing a `Vec`, when thread B forks.
The child gets A's memory at that instruction, and the lock is then forced free. The child's first text call works
on a structure caught in the middle of a change. That can be a `Vec` whose old buffer is already freed while its
pointer still names it, which is a use after free in safe-looking code. `window.rs` treats the same situation
soundly with `forget_taken`.
**Fix:** if the lock was held at the fork (`TEXT.is_locked()` before `reset`), forget the slot with
`crate::lock::forget_taken(..)`. The child then calls `zoraya_text_init` again and reads its fonts again. If the
lock was free, the slot is consistent and can stay.

### T3 (should-fix): a page can make NetSurf's glyph caches hold about 400 MB

ADR-0162's `GlyphCache` has 1024 slots, sized for the desktop's fixed text sizes. Here the size comes from the page:
`px(size, dpi)` reaches `MAX_PX` 512 at about 410 pt at scale 1, or half that at scale 2 (a `font-size` of a few hundred CSS pixels). Each of the
four faces has a cache of its own (`Fontset::of`: `Text::new` per face). A scratch test plotted 116 Latin and
Cyrillic glyphs at 512 px: 11,241,032 bytes of coverage, 96,905 bytes a glyph. One face's cache full of such glyphs
is about 99 MB, and the four faces together about 400 MB.
**Scenario:** a page with `font-size: 2000px` that holds about 1000 distinct characters (Latin, Cyrillic and Greek,
in regular, bold and monospace). NetSurf's heap grows by hundreds of MB. Under `--mem 768M` it aborts on the first
allocation that fails, and on a person's machine it takes memory from the other programs. ADR-0326 driver 2 relies
on ADR-0162's bounds, but those bounds assumed sizes that a hostile page does not choose.
**Fix:** bound the caches in bytes, not in slots. Or, in `zoraya-fontset`, do not cache glyphs above a size (render
and drop them), or keep fewer slots in the faces NetSurf uses. Add a test that fills a face at `MAX_PX` and checks
the bound.

### T4 (should-fix): every call shapes the whole text, so layout is quadratic

`width`, `position` and `split` each build a complete `Line` over all of `bytes`. That is four `Vec`s and about 70
bytes per character, with the whole text shaped every time. NetSurf's layout (`layout.c`) calls `width` and then
`split` on what is left of a box for every line it breaks. A scratch test ran that loop on 220 KB of Cyrillic words
(`"слово "` 20,000 times), with a host release build at 600 px: **300 lines took 1.10 s**, 3.7 ms a line. The whole
paragraph, about 1,800 lines at this width, costs several seconds per layout on the host, and NetSurf lays a page
out again during its load and on every resize. Under TCG it is about ten times slower. One call on 5 MB of text with
no spaces allocates about 350 MB.
ADR-0326's open question ("the cost ... not measured") is where this belongs, but this cost depends on the text's
length, not on the page. NetSurf's own fonts, internal and FreeType, stop `split` and `position` at `x`.
**Fix:** stop `position` and `split` at the first character past `x`. Shaping is left to right, so the pen is known
as it goes. Compute `width` without the `bounds` and `chars` vectors. Then add a test that bounds the work per call,
for example the shaped length as a function of `x`, and record the measured cost in ADR-0326's open question.

### T5 (should-fix): the three-pill check cannot fail when the fonts pill is missing

The new line is `region 663 116 4 8 f1f1f3`. Like the `788` line beside it, it asserts the title bar's own colour.
With the fonts pill missing, two pills are right-aligned against the frame (the net pill next to `/system/tls`).
Position 663 is then plain title bar, and position 788 is the gap between those two pills. Both regions still pass.
The comment claims that INV-0317-1 is checked for the new grant ("three pills"), but nothing on screen shows that
`+r /system/fonts` is there. The Cyrillic `ink` and `region` lines cover the grant only indirectly: without fonts,
the boxes make both lines equally wide.
**Fix:** also assert the pill fill inside the fonts pill (the `fcfcfc` of line 22, at an x between 663 and 788), so
that the check fails when that pill is gone.

### T6 (note): INV-0326-1 promises more than its check covers

The invariant says that nothing gives a POSIX program `/system/fonts` except a `window` line. But a line's own
`+r /system/fonts` (the `exec.grants` in `plan_with`) gives it, and so does a room grant (`add_room_grants`), for
any POSIX program. The `zoraya-shell` test checks only `with_window`, and nothing tests the zosh wiring
(`posix_fonts` at `zosh.rs:1446` and `:3816`). This is harmless: the data is the system's own read-only font files.
**Fix:** reword the invariant as "nothing *implicit*" or "only `with_window` adds it unasked". Or, if the line's
own grants are meant to be refused here, refuse them.

### T7 (note): smaller FFI points

- `Files::read` (`text.rs:69-84`) treats every `n < 0` as failure, including `EINTR`. `zoraya_text_init` stores the
  set it got (`get_or_insert_with`), so a signal during start-up leaves that font out for the rest of the process,
  and nothing logs it except the count. Retry on `EINTR`.
- `zoraya_text_metrics` is one of the seven names in `libc.a` (ADR-0326 item 2), but `font_zoraya.c` never calls
  it. It also writes `*ascent` and `*descent` without the NULL check that `position` and `split` make for
  `actual_x`. Either use it (fbtk's text height) or drop it from the block, and either way check for NULL.

### T8 (note): scale 3 is clamped

`fb_font_init` passes `90 * scale` to `browser_set_dpi`, which clamps to 250, and `zoraya_fontset::dpi(3)` is 250
too. At scale 3 the page is therefore 2.78 times, not 3 times, and does not match the desktop. ADR-0326 item 6 says
"90 times the scale" without this limit. Record it in ADR-0326 or in `docs/design.md`.

### T9 (note): untested FFI, and the untrusted case

`ffi/text.rs` has no host test: NULL style, NULL text, the busy answer, or `plot` NULL. ADR-0326 item 4 gives
`/system/fonts` to NetSurf in any room. The untrusted gate is closed today (`check-untrusted-gate.sh`: ok), but the
ADR does not say what an untrusted room's NetSurf would get. Read-only system data looks right, but it should be
written down where ADR-0268's list will look for it.

### What was checked and found sound (9f-text)

- The grant is `Access::Read` on the constant `/system/fonts` only. It is added only through
  `trusted_description(..).and_then(with_window)`, so only the image's own descriptions, matched by program path,
  get it. It goes through `merge`, `through_links` (a link at `/system/fonts` is refused unless covered) and
  `posix_entries` (missing means missing, and the program still runs). It is shown by `window_pills` as `+r` and by
  `grants`. A native program does not get it.
- The library builds paths only from `DIR` and `FILES`, with no environment, option or search. `Files::read`
  bounds the memory at `MAX_FILE` plus one 64 KiB chunk, and `load` checks again.
- In `text.rs`, every `unsafe` has a sound `// SAFETY:` under the documented C contract. `bytes` handles NULL and
  length 0. `Plot`'s coverage is `width × height` (`Bitmap` is row-major, and the test asserts the length). The lock
  is `try_lock`, so a re-entrant call from `plot` or from a signal gets the busy answer, not a deadlock.
  `zoraya-fontset` forbids `unsafe`.
- The arithmetic: `px` saturates in `i64` and clamps to 1..512. The pen saturates in 26.6. `decode` keeps byte
  offsets for invalid UTF-8. `split` never returns 0 for non-empty text, and `position` and `split` always return a
  boundary in `0..=len`. The random test exercises all of this.
- `run-disk.sh` copies `fonts/` into `/system/fonts` and puts the fonts in the key. `test-run-disk.sh` checks a
  changed font and a missing `fonts/`. The new `ink` step refuses a count of 0. The Cyrillic `ink` and `region`
  lines can fail on boxes or on one-cell widths.
- ADR bookkeeping: ADR-0318's note uses the open-question form, and its decision section is untouched. The index
  rows and the INV-0326 and INV-0327 rows match the ADRs, and `make adr-check` passes.

## 9f-wheel

| # | Severity | Where | Finding |
|---|---|---|---|
| W1 | should-fix | `crates/zoraya-ui/src/lib.rs:479-483`, `crates/zoraya-app/src/lib.rs:532` | A key that the focused widget ignores scrolls a `ScrollView` without any repaint. Reproduced. |
| W2 | note | `crates/zoraya-ui/src/lib.rs:131-137` | After the direction reverses, a part of a row kept from the other way shortens the first notch: 2 rows, not 3. |
| W3 | note | `crates/zoraya-ui/src/lib.rs:543-555` | `replace_keeping_scroll` restores by path alone, not by kind of widget. |
| W4 | note | `crates/zoraya-editor/src/view.rs:199-202` | Every key event clears `free`, including a modifier key on its own. |
| W5 | note | `docs/adr/invariants.md` | The INV-0329 rows say `checked` while ADR-0329 is proposed. INV-0327-1 says "not yet applicable (until accepted)". The two branches also conflict on the Total row (637 against 636). |

### W1 (should-fix): an ignored key scrolls a `ScrollView` and nothing repaints

`Window::event` calls `reveal_focus()` before every key reaches the focused widget. In a `ScrollView` this changes
`first` and sets `stale`. The response, though, is the widget's own. If the widget ignores the key (`Response::None`),
`App::ui_event` neither lays the window out again nor repaints it (`r != Response::None` at `zoraya-app` line 532).
A scratch test reproduced this: with `ten()`, Tab to button 1, turn the wheel by 240 (`first` 6), then send an
unhandled key code. The response is `None` and `first` is 0.
**Scenario:** in settings, the person turns the wheel so that the focused switch scrolls out of view, then presses
a letter. The model is now at the top while the screen still shows the bottom. The next wheel notch starts from the
top, and the page jumps by many rows at once. The next repaint, for any reason, jumps there too.
**Fix:** have `Window::event` return `Response::Repaint` when `reveal_focus` moved something (make `reveal` return
whether it moved), and add the scratch test above.

### W2 (note): a reversal after a fine wheel's part

`turn` keeps `rest` with its sign across a change of direction. After `rest = 39`, a notch the other way (`-120`)
gives `-81 / 40 = -2` rows. A notched wheel never leaves a rest, since `120 % 40 = 0`, so only high-resolution
wheels are affected. The test `turns_become_rows_three_a_notch_with_parts_kept` pins this as intended ("what was
kept counts against it"), but ADR-0329 does not say so. Either drop `rest` when the sign of `d` differs from it
(as GTK does), or say in ADR-0329 that it is intended.

### W3 (note): scroll restored by path, of whatever kind

`replace_keeping_scroll` gives a `Scrolled` to any widget at the same path that returns `Some` from `scrolled()`.
If a rebuilt tree puts a different kind of scrolling widget at that path, it gets the other widget's position: for
example, an editor's `top` 500 given to a `ListView`. The editor would also give `free` and `cursor`. The position
is clamped at layout or paint, so nothing breaks, but the view lands at an arbitrary place. Recording the widget's
kind in `Scrolled`, or matching on `id()`, would make the restore exact. Neither of today's users (settings with
one `ScrollView` root, edit with one editor) hits this case.

### W4 (note): a modifier on its own brings the editor back to the cursor

The editor sets `free = false` on every `Event::Key`. If the compositor sends a key event for Shift or Ctrl on its
own, then turning the wheel and pressing Ctrl, as the person starts a shortcut, snaps the view back to the cursor.
That matches the ADR's words "until a key", but it may surprise the person. Record it, or clear `free` only for
keys that move the cursor or change the text.

### W5 (note): bookkeeping

INV-0329-1 and INV-0329-2 have the status `checked: …` under a proposed ADR, while 9f-text marks INV-0327-1 of
proposed ADR-0327 "not yet applicable (until ADR-0327 is accepted)". One convention should hold. The architect
decides which. Merging both branches conflicts on the `**Total**` row of `invariants.md` (637 against 636; the
merged total is 639), on `index.md`, `design.md` and the TASK-27 report. `make adr-check` after the merge will catch
a wrong total.

### What was checked and found sound (9f-wheel)

- The 64-notch cap: `zoraya-app` clamps `dx` and `dy` to `gui::WHEEL_MAX`, and `WheelSteps::turn` clamps both `d`
  and the sum to `64 × 120`. `i32::MIN`, `i32::MAX` and `usize::MAX` are tested, and `rest` stays within ±39.
  `scrolled_by` uses `unsigned_abs` and saturating operations, so it can neither underflow nor go past `last`.
- `wheel_rows`: at an end it gives `None` and clears the rest, so the turn goes to the ancestor (tested). Inside, a
  partial turn counts toward a side it can still go. A turn of `dy == 0` scrolls nothing.
- `ScrollView`: `Logical` is an unsigned saturating type, so the heights left after the bottom are 0, never
  negative. Children before `first`, or below the bottom, get empty rectangles. `paint_tree` clips to them and
  `hit` skips them. `last_first` and `reveal` are correct when one child is taller than the frame. A frame that
  grows brings `first` back (tested).
- The wheel moves no focus, selection or cursor (the `a_wheel_moves_no_focus` tests, the file manager's and the
  editor's tests). The file manager ignores the wheel outside its list and while its menu or card is open. `follow`
  clamps `top` in both modes.
- `zoraya-app` decodes ordinal 46 into logical points with the window's scale and lays the window out again after a
  wheel or a key that was handled, so a press after a turn hits what the person sees.
- The mutants list `tools/mutants/wheel-9f.toml` and the `occurrence` fixes in `editor.toml` name lines that are in the code (each `find` string was seen in the diff).
  `ci-tests.txt` and `GUI_RUNS_ALL` add the wheel runs.
