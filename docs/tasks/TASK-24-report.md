# TASK-24 report: stage 7, the desktop

A running report, one section per part, updated after each (TASK-24, "Report"). The architect's queue (proposed ADRs,
decisions taken without the architect) is at the top; each part's section says what was built, the screens, the
measurements, the invariant delta, the review, the mutants, what was merged and the time it took.

## The architect's queue

| Item | Part | One-line recommendation |
|---|---|---|
| [ADR-0240](../adr/0240-fonts-embedded-until-the-launcher.md) (accepted 2026-11-09) | 7a | Until 7d's launcher hands out `/system/fonts`, the programs `init` starts embed the unmodified fonts through `zoraya-fonts`; a gap ADR-0162 leaves, not a change of it. |
| [ADR-0241](../adr/0241-window-corners-and-client-pixels.md) (accepted 2026-11-09) | 7b | Round a window's lower corners over its body, as ADR-0161 and the mockups want, reading INV-0164-1 inside the window's shape; square lower corners are the one-line alternative. |
| [ADR-0242](../adr/0242-the-panel-beside-init-in-stage-7d.md) (accepted 2026-11-09) | 7d | `init` keeps the first terminal and its rules and starts the panel with the client manager; the panel launches every other terminal into the current room with its record. A staging of ADR-0168, so 7d is not mergeable without the architect. |
| [ADR-0243](../adr/0243-a-rooms-tint-on-the-frame-only.md) (accepted 2026-11-09) | 7d | The room's tint on the title bar only; the mockups' 0.10 body tint would change client pixels (INV-0164-1). |
| [ADR-0244](../adr/0244-the-toolkits-own-events.md) (accepted 2026-11-09) | 7d | `zoraya-ui` takes a key press and a logical pointer of its own, and `zoraya-app` translates the protocol into them. |
| What `home` gives a terminal's shell | 7d | ADR-0242's condition says the first terminal holds "what the room home gives"; no ADR lists it. Chosen: `+r /`, `+w ~`, `+w /config`, `+w /tmp` (read the volume; change the person's files, configuration and temporary files), so that `room new`, `settings`, the tour and POSIX programs keep working in the first terminal. Narrower (the mockup's `+w ~` alone) breaks those; the choice is the architect's to change. |
| Pushing, merging and tagging | all | `git push` was refused until 2026-11-09, when the architect allowed it: `work/7a` to `work/7e` pushed; parts merged in order after CI is green and tagged `stage-7X` (see each part's Merge). |

## 7a: tokens and text

Branch `work/7a`, from `main` at `522269a`.

### What was built

- **The two backlog fixes, each its own commit with a test.**
  - *The TCG key-burst loss.* Two places lost keys. `term` sent one message per key to `zosh` (or `ttyd`) and dropped
    every key that found that 64-message queue full; it now holds up to 4 KiB of keys, sends them together and waits for
    `WRITABLE` (`9f6a6aa`). With that fixed a TCG run still lost 29 of 90 keys of one burst, with no drop counted
    anywhere in the system: QEMU's own trace (`-trace virtio_input_queue_full`, one diagnostic run) showed the
    virtio-input device dropping events, 14 times. Its event queue holds 64 events, 16 keys (a press and a release, each
    with its `SYN_REPORT`), and QEMU drops what arrives while the guest has not handed buffers back; QMP injects a key in
    well under a millisecond, which no keyboard does and which a guest under TCG, busy composing, does not keep up with.
    The runner now presses one key every 8 ms, USB full speed's polling interval, so the queue holds 128 ms of typing
    (`d7960a2`). The GUI tests type whole lines again: `tests/gui/terminal.script`'s long line in one burst, the tour's
    lines whole (`zoraya-tour` no longer cuts them in pieces of 12), and a 64-key line through `ttyd` in
    `tests/posix/gui.script`.
  - *The runner's `EAGAIN` panic* (`7605991`). `tools/run-qemu.sh` gives QEMU stdin from `/dev/null` in every test run
    without `--keys` and in the EL2 probes, which ran QEMU in interactive mode with the terminal as stdin (the likely
    source: `make test`'s `EL2_OR_SKIP`); the runner prints through `say!`/`esay!`, which retry a write that would block
    and never panic (host tests in `tools/test-runner/src/say.rs`).
- **`zoraya-tokens`**: `tokens.txt` is `clear-field`'s file, byte for byte (a test compares them), read at compile time
  by a `const` parser into `Logical` sizes (constructors private to the crate), `Scale`, `Color`, `Millis`, easing curves,
  the shape choices and both palettes. A missing name or a bad value fails the build.
- **`zoraya-text`**: ADR-0162's Option C, 1,100 lines without tests: the total `glyf` reader, the 26.6 fixed-point
  rasteriser with exact integer coverage, the two-way glyph cache, the `Shaper` trait and the simple shaper, the box,
  clipped drawing, and reference digests checked on the host and in QEMU.
- **Fonts**: `fonts/` with Inter 4.1 Regular and Medium, JetBrains Mono 2.304 Regular and Bold and both licences,
  unmodified (fetched by `tools/mockups/fetch-fonts.sh`, checked against its archive hashes), in a fonts table of
  `docs/dependencies.md` that a test checks by SHA-256; `zoraya-fonts` gives them as bytes.
- **The terminal on the new text**: JetBrains Mono at `text.mono` (8 × 18 cells at scale 1, as the mockup's terminal),
  `padding.terminal`, the `term.*` colours with derived bright and faint forms, bold in the bold font.
  `zoraya_term::read` reads cells back exactly; the GUI runner uses it.
- **The compositor's three stage 5 colours** are tokens (`bg.desktop`, `border.strong`, `accent`), so that `make
  check-tokens` covers every GUI crate that exists. `zoraya-font` is deleted.

### The 7a acceptance test

`plan.md`: "`make test-gui` reads Ukrainian and Czech names typed into the terminal back from a screendump at scale 1;
the mutation fuzz of the font reader runs under `make test-unit`". Done, with one difference: Ukrainian and Czech
letters cannot be typed before 7c's layouts (the keymap is US, and QMP sends key codes), so the names come from files
in the image (`rootfs/texts/uk/звіт-2026.txt`, `rootfs/texts/cz/příklad.txt`), are put on the command line by Tab
completion, and are read back from the screen with their contents; 7c's test types them. The font reader's fuzz
(`truncated_and_mutated_fonts_never_panic_and_draw_inside`: every truncation of a synthetic font and seeded mutations
of it and of both real fonts, drawn against a guard border) runs in `make test-unit`.

### Decisions taken without the architect

- **Homoglyphs.** INV-0162-4 asks that every typed glyph be distinct in the terminal's font. Twenty pairs of Cyrillic
  and Latin letters (А A, В B, С C, Е E, Н H, І I, К K, М M, О O, Р P, Т T, Х X, а a, с c, е e, і i, о o, р p, х x,
  у y) are drawn alike by JetBrains Mono, as by any font that draws them correctly. The test asserts that exactly
  these classes are equal and every other pair distinct; the reader reads a class as its Latin letter and the runner
  compares a script's text in the same classes. Recorded here, not as an ADR: it is how INV-0162-4's check is built.
- **Terminal colours.** `term.black`..`term.white` are SGR 30..37; the default foreground and background are `term.fg`
  and `term.bg` (new indices, so that SGR 37 is not the light set's dark text). Bright forms are a quarter of the way
  to `term.fg`, faint half way to `term.bg` (ADR-0161 lets the terminal derive them).
- **The grid** stays 80 × 24 (ADR-0096); the mockup's terminal window is wider (107 columns). 7b's `Configure` resizes
  it.

### Screens

In `docs/research/desktop/shots/7a/` (108 KB), all at scale 1:

| File | What | Against the mockup |
|---|---|---|
| `terminal-qemu.png` | The whole display after `tests/gui/terminal.script`'s Ukrainian and Czech steps, a QEMU screendump (`make test-gui`, HVF; the TCG dump is byte for byte the same). | `clear-field/terminal.png`: the same type (JetBrains Mono at 13 px), the same 8 × 18 cell and 14-pixel inset, `term.bg` and `term.fg`. Not yet: the frame, title bar, room tint and grant pills (7b, 7d), the bar and the dock (7b, 7d), the wallpaper (7b), and the wider window (107 columns in the mockup; 80 here until 7b's `Configure`). The background and the one-pixel frame are stage 5's in the tokens' `bg.desktop` and `accent`. |
| `terminal-qemu-text.png` | The terminal's surface alone, cut from the same dump. | Glyph for glyph as the mockup draws its terminal: same rasterising model (8-bit coverage, no hinting, whole-pixel origins), the system's in fixed point where the tool's is `f32`, so single pixels of coverage can differ by one level. |
| `terminal-host-light.png`, `terminal-host-dark.png` | The terminal in both sets, rendered on the host by the same crates (`cargo test -p zoraya-term -- --ignored samples`): red, green, blue, magenta, bold, faint, the cursor. The dark set cannot be shown in QEMU before 7f's settings. | The dark set's `term.*` colours as in `terminal-dark.png`; the prompt colours of the mockup (the room-coloured `project ~/project>`) are the shell's, not yet built. |
| `text-sample.png` | Inter at 13 and 20 px and JetBrains Mono at 13 and 26 px with Ukrainian and Czech text and the box glyph, from `zoraya-text` on the host. | The mockups' fonts at the token sizes. |


### Measurements

| | Host (M4, release) | HVF (debug, test kernel) | TCG (debug, test kernel) |
|---|---|---|---|
| A full 80 × 24 grid of Cyrillic, empty glyph cache | 0.52 ms | 0.47 to 0.57 ms | 4.5 to 4.6 ms |
| The same from the cache | 0.30 ms | 0.36 to 0.44 ms | 3.7 to 3.8 ms |
| Cache after two grids | 64 misses, 3,776 hits | the same | the same |

From `t_text` in every `test-qemu` configuration of one `make test` (three HVF, four TCG) and `zoraya-text`'s
`grid_cost` on the host. The rasteriser is fast enough and deterministic under TCG (the reference digests match
in every configuration, and the HVF and TCG screendumps are identical), so no proposed ADR and no conservative path
was needed. Kerning: Inter keeps its kerning in GPOS only and JetBrains Mono has none, so `kern` format 0 kerns
nothing in the system's fonts (ADR-0162's open question; GPOS pair adjustment would be a later addition).


### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0161-1 | not yet applicable (stage 7) | checked: `make check-tokens` |
| INV-0161-2, -3, -5, -6 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0161-4 | not yet applicable (stage 7) | not yet applicable (stage 7): needs `zoraya-ui` (7d) |
| INV-0162-1, -3, -5 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0162-2 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-qemu` (`t_text`) |
| INV-0162-4 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0095-1, -2 | checked: `make test-unit` | replaced by INV-0162-1 and -4 (`zoraya-font` deleted) |
| INV-0240-1 (new, proposed ADR) | | checked: `make check-tokens` |

### Review

`docs/tasks/TASK-24-review-7a.md`: 4 errors (three panics on hostile fonts and sizes, reproduced by the reviewer;
one invariant status that claimed too much), 5 warnings, 4 notes. All fixed in `831a350`, with tests; the check of the
changed lines found no open error. One more round was not needed.


### Mutation testing

`make mutants-host` (cargo-mutants 27.1.0, `--in-diff` against `main`, two jobs, timeouts from the baseline, under
the watchdog), 65 of the part's 90 minutes:

| Crate | Mutants | Caught | Missed | Unviable | Timeouts | Time |
|---|---|---|---|---|---|---|
| `zoraya-text`, first run | 1,032 | 798 | 189 | 18 | 27 | 21 min |
| `zoraya-tokens` | 344 | 123 | 27 | 191 | 3 | 18 min |
| `zoraya-term` | 121 | 103 | 10 | 8 | 0 | 4 min |
| `zoraya-text`, after the new tests | 1,055 | 910 | 98 | 18 | 29 | 22 min |

- `zoraya-text`: the 189 survivors were sorted by function by the main model; most lay in code no test reached
  (composites with scales and matrices, the choice of `cmap` and `kern` subtables, contours off the curve, the box,
  the cache's ways). `tests_holes.rs` (`e28665b`) covers them, and one of its tests, against each pixel's exact area,
  found a real bias: the floor division of edge crossings made shallow edges' pixels up to three levels too full;
  crossings now round to the nearest (the reference digests changed with it). 98 survive the second run, not sorted
  within the cap: 19 in `Raster::row`'s splitting (changes within the exact-area test's tolerance of two levels, by
  sampling), 12 in the cache's slot hash and ways (any slot function gives the same pixels), the rest in defensive
  bounds of the font reader and in `draw_cell`'s centring of a fallback glyph. They are the first thing to sort when
  `zoraya-text` is next worked on.
- `zoraya-tokens` and `zoraya-term`: the `mutant-triage` subagent sorted all 37. Equivalent, rechecked by the main
  model: `Color::blend`'s and `parse_color`'s `|` to `^` (disjoint bits), `content_end`'s second loop start (the first
  `if` returns on a leading `#`), `starts_with`'s guard (unreachable on trimmed keys), `find`'s, `word`'s and `lines`'
  `<` to `<=` on a pass that reads nothing, `yes_no` to false (no token is `yes`), `put`'s `c < BRIGHT` to `<=` (no
  colour index 8 reaches it), and `read`'s `&&` to `||` over colours that equal the background. The 26 holes have tests
  (`7dfa524`), and the one marked unsure (`canonical` of blank and `_`) has a test in every look.


### Merge

Not merged: see the queue. `work/7a` is complete locally at `ad9becd`, where a full `make test` from the snapshot
worktree passed (every step, 1,055 host tests, the seven QEMU configurations, both GUI runs); 7b starts from it.

### Time

From the commits' times (2026-11-08) and the logs; builds and tests overlap with writing, so the rows are wall
time for each step, not exclusive.

| Step | Time |
|---|---|
| Reading the task, the ADRs, the code; fetching the fonts | 15 min |
| The two backlog fixes (writing, the TCG trace, three GUI runs) | 1 h 10 min |
| `zoraya-tokens`, `zoraya-text`, `zoraya-fonts`, the terminal and the reader (writing and host tests) | 1 h 30 min |
| Full `make test` (two runs) | 50 min |
| Review and its fixes | 30 min |
| Mutation runs and their tests | 1 h 20 min (65 min of mutants) |


## 7b: frames, windows and scale

Branch `work/7b`, from `work/7a` (7a could not be merged into `main`: see the queue), with 7a's later commits
merged in.

### What was built

- **`zoraya-paint`**: the compositor's drawing in integer arithmetic: rounded rectangles with the smooth corner (a
  cubic superellipse spanning 1.6 radii, supersampled), outlines, soft shadows (three box passes), the `deep`
  wallpaper in 16.16 fixed point with an integer sine, region blur, segments and polygons through `zoraya-text`'s
  rasteriser, a clipped canvas that copies client pixels unchanged, and the trusted material (halo, glass, tint, band
  of light, rim, border).
- **The window protocol** (ADR-0160, ADR-0164), new ordinals: `DisplayScale`, `CreateToplevel`, `CreatePopup`,
  `CommitTo`, `SetBuffer`, `SetTitle`, `Close`; events `Configure`, `CloseRequested`, `Minimised`, `WindowFocus`,
  `WindowPointer`. Stage 5's `Create` and `Commit` stay (ttyd's window backend and `t_gui` use them).
- **The compositor**, rewritten around windows: several per client, popups, frames drawn from the tokens (title bar,
  title in Inter Medium, cross and dash, border, smooth corners, two-layer shadow), moves, resizes, minimise and
  close (and close anyway), the system key's chords, Alt+Tab's switcher in the trusted material grouped by room,
  F12, the top bar on the blurred wallpaper, the scale from the display width (`auto`), and the review's limits
  (popups only from the focused window and clipped below the bar, resizes within the work area, a per-client
  budget of buffers).
- **`term`** as a toplevel that resizes its grid on `Configure` and ends on the close button; **`init`** takes that
  as a closed window and opens a new terminal and shell.
- **Tests and tools**: `tests/gui/windows.script` (move, minimise, F12, Alt+Tab, close); `make test-gui` runs both
  scripts under HVF and TCG at scale 1 and under HVF at scale 2; `tools/run-qemu.sh --gui-scale 2`; the runner reads
  at the display's scale, takes logical coordinates, and has `click`, `drag` and waiting `region` steps.

### The 7b acceptance test

`plan.md`: "the GUI test moves, minimises, restores and closes a terminal window over QMP, switches with Alt+Tab and
F12, and passes at both scales". Done: `tests/gui/windows.script` and `tests/gui/terminal.script` pass in all six
runs of `make test-gui` (HVF and TCG at scale 1, HVF at scale 2). Scale 2 runs under HVF only: TCG at 2048 × 1536
would add minutes to every `make test` for what the HVF run and the host tests at scale 2 already show; recorded
here as a decision.

### Decisions taken without the architect

- **Proposed [ADR-0241](../adr/0241-window-corners-and-client-pixels.md)**: round lower corners over a client's
  pixels, INV-0164-1 read inside the window's shape (Option A, square lower corners, is one line).
- **Placement**: the first frame at (72, 42) logical below the bar (the mockups' terminal at (72, 70)), cascaded by
  32, never above the bar.
- **F12** goes to the next window in the switcher's order (the focused one, the stack from the top, hidden ones
  last), not in creation order.
- **Close anyway**: a press between 5 and 20 s after an unanswered `CloseRequested`; a commit to the window clears the
  ask (ADR-0160 gives 5 s as an assumption and no expiry).
- **Deferred** to 7c and 7d, and reaching no client meanwhile: System+Space, System+Enter, System+R, System+?.
- **No clock** in the bar: the wall clock (ADR-0226) is not built.
- **Budgets**: eight toplevels and four popups a client, buffers at most four displays' worth, 32 windows in all.

### Screens

In `docs/research/desktop/shots/7b/` (864 KB):

| File | What | Against the mockup |
|---|---|---|
| `desktop-qemu.png` | The desktop in QEMU at scale 1 (HVF; the TCG dump is byte for byte the same): the bar, the wallpaper, the terminal's frame and shadow. | `clear-field/terminal.png` and `desktop.png`: the same bar, wallpaper, title bar, buttons, corners and shadow. Not yet: the dock and the room chips (7d), the room badge, tints and grant pills (7d), the clock (no wall clock), the wider window. |
| `desktop-qemu-2x.png` | The same at scale 2 (2048 × 1536). | The mockup's `desktop` screen at 2x: every frame dimension and glyph at twice the size, read back as the same text by the GUI test (INV-0164-2). |
| `windows-light.png`, `windows-dark.png` | Two windows of the real compositor on the host (an ignored test, `samples`), focused and not, both sets. | The focused window's coloured buttons and stronger shadow against the other's grey ones, as `desktop.png` draws them. |
| `switcher-light.png`, `switcher-dark.png` | The switcher open over them, both sets. | `switcher.png`: the material with its halo, the group line, a row per window with its own pixels shrunk and the chosen one highlighted. The mockup's two rooms and their badges come with 7d. |

### Measurements

`t_gui` in the test kernel, from `make test` on `work/7c` at 8640331 (it includes 7b), QEMU 11.1.2 on the M4 Mac mini;
also in `docs/design.md`:

| | HVF | TCG |
|---|---|---|
| Full redraw of a 1024 × 768 legacy surface with its frame and shadow: compose per frame | 6.0 ms (155 frames/s) | 54 ms (17 frames/s) |
| The frame that opens the switcher (blur of the scene under it) | 18.5 ms | 102 ms |
| A frame committed under the open switcher | 18.6 ms | 100 ms |

A commit under the open switcher blurs the scene again, so it costs what opening does; the pointer alone reuses the
last frame. That `make test` run passed 19 of 21 steps, every GUI script under HVF and TCG at both scales included;
the two that failed, `check-test-caps` (`zoraya-keymap`'s tests had no heap cap) and `check-tokens` (it exempted
`tests.rs` but not the compositor's `tests_windows.rs`), are fixed in `013ed16` on `work/7c`.

### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0160-1, -3 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0160-2, -4, -5, -6, -7 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0164-1, -3 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0164-2 | not yet applicable (stage 7) | checked: `make test-gui` |
| INV-0094-2 | checked | replaced by INV-0160-2 |
| INV-0241-1 (new, proposed ADR) | | checked: `make test-unit` |

### Review

`docs/tasks/TASK-24-review-7b.md`: 1 error (an unfocused client's popup over another client's window), 9 warnings,
6 notes; all fixed or answered in `c2962fd`, with tests; no open error.

### Mutation testing

`make mutants-host` with `--in-diff` against `main`, two at a time.

- **`zoraya-paint`**: 166 survivors in the first run, in the rounded-rectangle coverage, the outlines, the blur's
  passes and the wallpaper's integer sine; tests for each hole in `ba92c8e` (exact coverage at corner pixels, outline
  depths, a blur against a direct average, the wallpaper's digest). The run's full counts were overwritten by the
  compositor's run and are not reported.
- **`zoraya-compositor`**: capped at 90 minutes after 460 of its mutants: 304 caught, 133 missed, 5 timed out, 18
  unviable. The survivors were mostly the drag clamps (`drag_to`) and `reconfigure`'s rescaling; tests of every clamp,
  resizes from each side and at scale 2, and rescaling in `2e8aaa4`. The mutants past the cap were not run.

### Merge

Not merged (the queue).

### Time

From the commits' times: the build to the design notes 46 minutes (18:17 to 19:03), the review and its fixes 15
minutes, the mutation runs and their tests about an hour (19:49 to 20:49), overlapping 7c's writing.

## 7c: keyboard layouts

Branch `work/7c`, from `work/7b`, with 7b's later commits merged in.

### What was built

- **`zoraya-keymap`**: `us`, `ua`, `ru` and `cz` as tables of four levels per key (plain, Shift, AltGr,
  Shift+AltGr), from the reference layouts the survey records; thirteen accents and their compositions over Latin-1
  and Latin Extended-A; the dead-key machine (compose, Space for the spacing accent, any other key cancels, a second
  dead key replaces the first); Caps Lock on case pairs; the right Alt as AltGr where a layout has a third level;
  Ctrl's control characters from the active layout's Latin letters (the US ones in a Cyrillic layout); switching over
  an enabled list.
- **The compositor**: System+Space switches (Shift: back) over `us ua cz`, clears a dead key at a focus change and
  keeps a chord's keys out of it, shows the layout in the bar, and sends `Layout { name }` (ordinal 24) to the focused
  client when it gains focus and at every switch.
- **The title** (ADR-0168): `zosh` sends `Title` (its directory as the prompt shows it) at start and after each `cd`;
  `term` titles its window `zosh — <dir>`, never from output.
- **The GUI runner** types under a layout (`layout NAME`), a letter with no key through a dead key, and `chord meta`
  presses the system key; `tests/gui/layouts.script` runs in `make test-gui` under HVF and TCG.

### The 7c acceptance test

`plan.md`: "the GUI test switches to `ua`, types a Ukrainian name, switches to `cz`, types `příklad.txt` through dead
keys, and reads both back". Done: `tests/gui/layouts.script` types `звіт-2026 ґанок` in `ua` and `příklad.txt ďóň` in
`cz` (`ř`, `í` and `á` on their own keys as in the reference layout, `ď`, `ó` and `ň` through the dead caron and acute),
reads the line back, and types `ls` after switching round to `us` (INV-0163-4's GUI half); it passes under HVF and
TCG, whose screens are byte for byte the same.

### Decisions taken without the architect

- **The enabled layouts** are `us ua cz` and the active one `us` at start, ADR-0163's example, until 7f's settings
  (`gui.keyboard-layouts`, `gui.keyboard-layout`).
- **Third and fourth levels** are only Czech's and the Ukrainian ґ and backslash; the others' are left out until
  something needs them, rather than guessed (listed in `layouts.rs`).
- **Caps Lock** follows ADR-0163's case-pair rule, so it does not capitalise Czech's number-row letters as XKB's
  semi-alphabetic key type does; stated in `docs/design.md`.
- **Ctrl** uses the active layout's Latin letter (QWERTZ's Ctrl+Z is the key labelled Z), the US one in a Cyrillic
  layout, as XKB desktops do (review finding 5).

### Screens

`docs/research/desktop/shots/7c/layouts-qemu.png`: the terminal with `echo звіт-2026 ґанок příklad.txt ďóň` typed
under `ua` and `cz`, the window titled `zosh — ~` by the shell's `Title`, and `CZ` at the right of the bar
(QEMU, HVF; the TCG dump is the same). Against `clear-field/desktop.png`: the layout indicator at the right of the bar
as in the mockup (`UA` there), without the clock beside it (no wall clock yet). Light only: the indicator and the
title are the bar's and the frame's, whose dark set 7b's screens show.

### Measurements

`plan.md` lists none for 7c.

### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0163-1, -2, -3 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0163-4 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |

### Review

`docs/tasks/TASK-24-review-7c.md`: 2 errors (a dead key survived Enter, Backspace or a Ctrl chord), 5 warnings, 6
notes; all fixed or answered in `f1594dd`, with tests; no open error.

### Mutation testing

`make mutants-host` on `zoraya-keymap` with `--in-diff` against `main` (run on `work/7d`, after 7d's `make test`):
122 mutants in 42 seconds, 13 missed, 82 caught, 27 unviable. Eleven were holes: an index past the layouts, the dead
key's accessors (`dead`, `clear_dead`), the right Alt's release in either role, AltGr's fallbacks to the third and the
plain level, and the Ukrainian digits (a mutated `const` loop left them empty). Tests for each in `a618df7` (on
`work/7d`, since 7c is not merged): 2 missed after them. Both are equivalent for the shipped tables, as the main model
checked: `text`'s fallback guard `level >= 2` made `true` changes only keys with an empty first or second level, and
`has_altgr`'s `||` made `&&` only a layout with a third level and no fourth; a test of the tables now asserts that
neither exists (`every_key_has_its_first_two_levels`).

### Merge

Not merged (the queue).

### Time

About 2 h 30 min of writing, including two GUI runs; the review and its fixes 30 min.

## 7d: toolkit, dock and rooms

Branch `work/7d`, from `work/7c`, with 7c's later commits merged in.

### What was built

- **`zoraya-ui`** (ADR-0165): the first widgets (label, button, text field, check box, switch, radio group, list,
  scroll view, separator, menu, dialog, progress bar; row, column and grid), a layout of logical units, painting
  through `zoraya-paint` and `zoraya-text` with the tokens' Field set, keyboard focus and Tab order; it depends on no protocol crate (INV-0165-1)
  and has no colour of its own (INV-0165-3).
- **`zoraya-app`**: the client loop over a `Platform` trait (with a fake compositor in the tests): connect, open a
  toplevel, repaint with one commit in flight (ADR-0093), Configure, close, and `wait()` for what the application must
  act on; the toolkit's own key and pointer events (proposed ADR-0244).
- **Rooms** (ADR-0172): `/config/rooms` read and written by `zoraya-settings` (total reader, a writer keeping every
  other byte, colour slots); `room`, `room new NAME +w PATH...`, `room NAME` and `room remove NAME` in `zosh`, with the
  prompt naming the room; the compositor's records per client (room, colour, entries), the title-bar tint, the badge
  with its dot, the grant pills (folded into `+N`), the untrusted edge and word, and the fields around a room's windows
  drawn only when two or more rooms have windows.
- **Dock B and the top bar**, drawn by the compositor: tiles of neutral material, room-coloured running dots, the
  untrusted ring, room chips and `+`, tooltips, show-commands mode (System+?), System+Enter for a new terminal, the
  switcher grouped by room.
- **The panel** (ADR-0168, proposed ADR-0242): a program `init` starts with the client manager; it reads the room
  table, tells the compositor the table and the dock (`SetRooms`, `SetDock`), and on `DockPressed` launches a terminal
  into the current room with the handles its launch record lists, or enters a room. `init` keeps the first terminal.
- **Untrusted rooms**: drawn and tested (the reserved hue, the two-pixel edge, the word, INV-0172-10), but `room new
  --untrusted` is refused naming ADR-0056, and the panel launches nothing in a hand-written untrusted line (review
  finding 1); tests reach them only through the compositor's host tests.

### The 7d acceptance test

`plan.md`: "a second terminal starts from the panel with the handles its launch record lists, hidden windows restore
from the panel, and the widget tests run on the host against the tokens". Done: `tests/gui/rooms.script` (47 steps)
presses the dock's terminal tile and types in the new terminal, hides both windows by their dashes and brings them
back from the dock's icon, makes a room in the shell (`room new project +w ~/project +r /system`), sees its chip
appear, launches a terminal in it, reads its pills, its badge and its field from the screen, and shows the commands;
it passes under HVF and TCG in `make test-gui`. The widget tests run in `make test-unit`.

### Decisions taken without the architect

- **The panel beside `init`** (proposed ADR-0242): the first terminal stays `init`'s, which sends its own record
  (`home`, `term`, `+w /`) with `AddRoomClient`; every other terminal is the panel's. This stages ADR-0168's decision
  that the panel starts applications, so **7d is not mergeable without the architect** (TASK-24, "If a part cannot be
  merged"); 7e starts from `work/7d`.
- **The room's tint on the title bar only** (proposed ADR-0243): the mockups' body tint would change the client's
  pixels, which INV-0164-1 forbids.
- **The toolkit's own events** (proposed ADR-0244).
- **Every terminal holds the root** until 7e's chooser and `narrow` in launches: the pills show the room's record,
  but INV-0172-2 ("exactly the room's entries") is back to not yet applicable (review finding 2), with the reason in
  ADR-0172's open questions.
- **The dock's applications** are `term` only until 7e and 7f add theirs.

### Screens

Under `docs/research/desktop/shots/7d/` (712 KB):

- `rooms-qemu.png` (QEMU, HVF): a `home` terminal and two `project` terminals, the field around `project`'s windows,
  the tinted title bar with its pills (`+w /`, `+w ~/project`, `+r /system`) and its badge, and the chips in the dock.
  Against `clear-field/rooms.png`: the field, the tint, the badge and the chips as in the mockup. Not there: the Rooms
  sheet (System+R), which no 7d ADR asks for (the shell's `room` is the way in); the field's header tag is hidden
  under the `home` window here; the untrusted room (no way to make one, as above); the clock; the other applications'
  tiles. The terminal's pills include `+w /` because every terminal holds the root (above).
- `dock-qemu.png`: dock B with the terminal tile, its two running dots and the `home` chip, against `clear-field/dock.png`;
  the same materials, sizes and dots, with one tile instead of five.
- `commands-qemu.png`: show-commands mode, each tile and chip with the text of its command, against
  `clear-field/commands.png`, with the same reduction.
- `rooms-light.png`, `rooms-dark.png`: the compositor's own rendering on the host (the ignored test `rooms_samples`)
  of three rooms with an untrusted one, in both themes: the reserved hue, the two-pixel edge, the word `untrusted`,
  the folded pills and the fields cut back between rooms, which the QEMU desktop cannot show yet.

### Measurements

`plan.md` asks for the toolkit's paint time for the settings window: a settings-like page (headings, a radio group, a
switch, a check box, a list, a field, two buttons) paints in 0.35 ms with an empty glyph cache and 0.23 ms after under
HVF, 5.0 ms and 3.2 ms under TCG (`t_gui`'s `toolkit_paint`, in `docs/design.md`).

### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0165-1, -2, -4 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0165-3 | not yet applicable (stage 7) | checked: `make check-tokens` |
| INV-0168-1 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0168-3 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0172-1 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0172-5 to -10 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0172-2 | not yet applicable (stage 7) | unchanged, with the reason in ADR-0172's open questions |
| INV-0242-1 (proposed) | new | checked: `make test-unit` |

### Review

`docs/tasks/TASK-24-review-7d.md`: 2 errors (an untrusted line in the file was usable; INV-0172-2 claimed), 7
warnings, 5 notes; all fixed or answered in `ba5234d`, with tests; no open error.

### Mutation testing

`make mutants-host` with `--in-diff` against `work/7c` (7d's own lines), one crate after another, capped at 90
minutes in all; the survivors sorted by the `mutant-triage` subagent, every equivalent rechecked by the main model.

| Crate | Mutants | Missed first | Missed after the tests | Of those, equivalent |
|---|---|---|---|---|
| `zoraya-settings` (rooms) | 121 | 18 | 0 | - |
| `zoraya-panel` | 28 | 1 | 0 | - |
| `zoraya-app` | 67 | 12 | 0 | - |
| `zoraya-ui` | 443 | 196 | 9 | 5 |
| `zoraya-compositor` | 1075 (529 run) | 431 | not run again | 11 |

- **Settings, panel, app** (`18d193e`): the bounds of a grant path, a line's words, the room count and a writer's
  grants; the exact cut of a long rooms file; a table of exactly `MAX_ROOMS_TEXT`; buffers and commits at scale 2, a
  stale reply, two windows, ignored Configures, focus kept when another window loses it.
- **`zoraya-ui`** (`f3fcda8`, `68c1bad`): the rectangles of rows, columns and grids, where a press lands and what it
  focuses, Tab's order, a field's edits at its cursor, the controls' pixels and states. The triage found a real bug:
  `ScrollView` clamped its offset only on a scroll, so a frame that grew showed a blank past the content; the clamp
  is now in `layout`. The 5 equivalents: `children`' empty slice either way, `select`'s `<=` at `i == top`, two `>=`
  in `ScrollView::layout` that assign the same value at equality, and `h + offset` there, the same as `h - offset`
  now that the offset is clamped. The 4 holes left: the radio label's gap (antialiased pixels only) and the
  `paint` of `ScrollView`, `Menu` and `Dialog`, hidden under their children's ground.
- **`zoraya-compositor`**: 529 of 1075 mutants ran in the 85 minutes left by the cap (431 missed, 91 caught, 7
  unviable, 2 timeouts); `rooms.rs` (`set_rooms`, `set_dock`, `Record::of`, `dock_press`), `fields.rs` and
  `draw_rooms.rs` past line 582 were **not reached**. Of the misses, 11 are equivalent for scales 1 and 2, the only
  ones `Scale::new` accepts (`(si + 1) / 4` is 0 for both, `(3 * si + 1) / 2` equals `(3 + si + 1) / 2`,
  `(3 * k).div_ceil(2)` equals `(3 + k).div_ceil(2)`, a circle's radius clamped to half its side, a skipped dock draw
  clipped anyway). The rest were holes, nearly all in drawing arithmetic; tests were added (`68c1bad`) first for the
  trust boundary: a manager request with the wrong number of handles is refused and its handles closed, the panel
  stays attached when its channel is empty and when `init`'s closes, each badge's text, the untrusted edge on every
  side at scale 2, the switcher's room order against the order of use; then the dock's dots, chips, hover, tooltip
  and divider, the title beside the pills, tiling at scale 2. They were not measured again under the cap. **Left for
  a later run**: the compositor's whole 7d diff (about 3 hours at 2 workers), first `rooms.rs` and `fields.rs`.
- **Noted by the triage, not changed in 7d**:
  - The driver manager's channel closing (init gone) leaves it watched, as in stage 5; harmless while `init` never
    exits.
  - In the light set a dock tile and its hover lift are both white, so hovering shows only the tooltip.
  - A button's label is measured in the regular face and drawn in the medium one, so it sits a little off centre.
  - Escape closes a dialog or menu only at a window's root.
  - `Dialog`'s comment says Enter activates the default button; any focused button does.

  These are for 7f's settings window, the toolkit's first real user.

### Merge

Not merged: proposed ADR-0242 stages an accepted ADR's decision, and `git push` is refused (the queue).

### Time

About 3 h 30 min of writing (the crates, the panel, the GUI test and the review, to 22:25), 1 h for `make test` and
its fixes, the mutation runs 90 min (capped), and their triage and tests 1 h 20 min.

## 7e: chooser and files

Branch `work/7e`, from `work/7d`.

### What was built

- **`zoraya-proto-chooser`** (`Open`, `Save`, `PickDirectory`, `Chosen`, `Cancelled`; the panel's `AddApplication`; the
  file manager's `OpenWith`), **`zoraya-fsclient`** (a file-protocol client over any link, tested against the real file
  service in-process), **`zoraya-chooser`** (what a sheet lists, what may be picked, the grant each pick gives in
  ADR-0153's rights, and the sheet itself over `zoraya-app`), **`zoraya-files`** (the file manager's decisions).
- **The compositor**: sheets for system clients (`AddSystemClient`, `CreateSheet`, `Granted`), drawn with the
  trusted strip, emblem and heading, and the bar's mark; drops (`Drag`, `Drop`, `DragEnded`, ADR-0170), with the press
  serial in `WindowPointer`.
- **Programs**: `chooser`, `files`, `t_open`; the panel launches applications with a chooser channel, a data directory
  `/data/ROOM/APP` and, from the file manager, the opened file read-only.
- **The toolkit**: a text field answers Enter; a field keeps its cursor when rebuilt; `zoraya-app` takes a drop's
  handle with its message and starts drags only from a held press.

### The 7e acceptance test

`plan.md`: "the GUI test opens a file from the file manager in a test application through the chooser over QMP, the
application reads it and cannot write it (`Open`) or can (`Save`)". Done: `tests/gui/files.script` (58 steps, HVF and
TCG, in `make test-gui`) opens `notes.txt` from `files` in `t_open` (read, the write refused), opens it again
through the chooser's sheet (the same), saves `nový.txt` through it (written; a second name beside it refused), drags
`notes.txt` from `files` onto `t_open` (read, the write refused), and lists both files in the first terminal.

### Decisions taken without the architect

- Proposed **ADR-0245**: the chooser's wiring (registration by the panel with the compositor tag, sheets only from
  system clients, `Save` as the folder narrowed to one name, the data directory, `OpenWith` read-only). It fills gaps
  of ADR-0167 and ADR-0168 and changes neither.
- Drops follow ADR-0170 with the press serial as a field of `WindowPointer` counted per client, `DragDone` standing
  for `DragStarted`, and the pill in ADR-0170's words, written by the compositor.

### Screens

Under `docs/research/desktop/shots/7e/` (light, HVF, scale 1):

- `files-qemu.png`: the file manager over the first terminal, with its pills (`+w /data/home/files`, `+w /`).
- `open-qemu.png`, `save-qemu.png`: the chooser's sheets under `t_open`, against `clear-field/chooser.png`. As in the
  mockup: the strip with the emblem `Z` and "Open a file for t_open", the same heading in the bar, the sheet hung under
  its owner's title bar, the sentence and the grant's line, Cancel and the primary action. Not as in the mockup: no
  file-type icons and no frame around the list (the toolkit's list has neither yet), no breadcrumb chevrons (the sheet
  shows `~` and an Up button), and the sheet is wider than its small owner (the mockup's owner is the editor).
- `drop-qemu.png`: after the drop, `t_open`'s frame with the compositor's pill `dropped: notes.txt read`, against
  `clear-field/drop.png`, which shows the drag in flight with its label; the screendump is taken after the release.

### Measurements

The chooser's round trip, in `docs/design.md`: about 0.9 ms from the request to the sheet and 9 to 10.5 ms from the
confirming key to the answer under HVF; 17.5 to 22.5 ms and 70 to 75 ms under TCG.

### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0167-1, -2, -4 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0167-3 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0168-2 | not yet applicable (stage 7) | checked: `make test-unit` (the in-QEMU half waits for `edit`, 7f) |
| INV-0170-3, -4 | not yet applicable (stage 7) | checked: `make test-unit` (the clipboard's half in 7f) |

### Review

`docs/tasks/TASK-24-review-7e.md`: 2 errors, 10 warnings, 14 notes. Both errors fixed (a dragging client wrote the
target's pill; applications learned the path of what they were given), and most warnings; the rest are recorded in
ADR-0245's open questions. The check of the changed lines confirmed the fixes and found a new warning (the name
field's cursor jumped to the end when the sheet was rebuilt), fixed with a test, and two small gaps, fixed.

## To finish by hand

`git push` was refused in this session, so these steps are the architect's (or a session allowed to push), in order,
for each part that is complete:

```
git push -u origin work/7a
# merge as TASK-24 says: from a detached HEAD at origin/main
git fetch origin && git switch --detach origin/main
git merge --no-ff work/7a -m "Merge work/7a: stage 7a, tokens and text (TASK-24)"
git push origin HEAD:main          # then wait for every CI job
git tag stage-7a && git push origin refs/tags/stage-7a
```

Then the same for `work/7b` and `work/7c` (each from the new `origin/main`, merging `origin/main` into the branch
first). `work/7d` waits for the architect's decision on ADR-0242; once it is accepted (or changed and 7d brought in
line):

```
git push -u origin work/7b work/7c work/7d
git fetch origin && git switch --detach origin/main
git merge --no-ff work/7b -m "Merge work/7b: stage 7b, frames, windows and scale (TASK-24)"
git push origin HEAD:main          # CI green, then:
git tag stage-7b && git push origin refs/tags/stage-7b
# likewise work/7c ("keyboard layouts", stage-7c) and work/7d ("toolkit, dock and rooms", stage-7d)
```

To try the desktop by hand: `make run-gui`; the window shows the terminal in JetBrains Mono. `ls /texts/uk`, `cat
/texts/uk/` then Tab and Enter, and the same for `/texts/cz/`, show Ukrainian and Czech text; Shift with Page Up
scrolls back.


From 7b on (`work/7d` has everything): drag a window by its title bar, System+Left and System+Right tile it, Alt+Tab
opens the switcher, grouped by room. From 7c: System+Space switches the layout (`us`, `ua`, `cz`, shown at the right
of the bar). From 7d: the terminal tile in the dock (or System+Enter) opens another terminal; in a terminal, `room new
project +w ~/project +r /system` makes a room, its chip appears in the dock; press it, then the terminal tile, and
the new terminal's title bar carries the room's tint, badge and pills, with a field around its windows while `home`
has windows too. System+? shows every gesture's text.
