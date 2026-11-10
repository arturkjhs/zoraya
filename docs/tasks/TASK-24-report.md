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
| What `home` gives a terminal's shell (accepted 2026-11-09, recorded in ADR-0172) | 7d | ADR-0242's condition says the first terminal holds "what the room home gives"; no ADR lists it. Chosen: `+r /`, `+w ~`, `+w /config`, `+w /tmp` (read the volume; change the person's files, configuration and temporary files), so that `room new`, `settings`, the tour and POSIX programs keep working in the first terminal. Narrower (the mockup's `+w ~` alone) breaks those; the choice is the architect's to change. |
| [ADR-0245](../adr/0245-the-choosers-wiring-in-stage-7e.md) (accepted 2026-11-10) | 7e | Accepted with two conditions, built on `fix/7-decisions`: a sheet ignores input for 500 ms from its first commit and any key or button held from before (or pressed within it) until released; show-commands mode shows the grant's `zosh` line (`+w FOLDER APP · gives less: ...` for `Save`). |
| INV-0168-2 against INV-0169-4 (decided 2026-11-10) | 7f | INV-0168-2 gave way: `files` holds the volume read-only and, to change, `~` in `home` or the room's grants, never `/config` (also through `~`, review of the fixes, 2); recorded in ADR-0168. |
| `/config/rooms` and `/config/settings` written in place by a shell (accepted for now, 2026-11-09) | 7d | A crash between truncate and write loses the file; in `docs/backlog.md`. |
| [ADR-0246](../adr/0246-the-display-follows-the-window.md) (accepted 2026-11-10) | 7g | Option C; its two open questions wait for the architect's check by hand. |
| [ADR-0247](../adr/0247-scale-from-the-displays-density.md) (proposed) | fixes | Scale 2 from 1280 pixels at 150 ppi or more, the physical size read from the EDID QEMU's cocoa window fills in; it changes INV-0090-1 (`VERSION_1` and `F_EDID`), which the code already follows at the architect's request: if rejected, `begin(..., 0)` restores the rule. |
| [ADR-0248](../adr/0248-what-the-file-manager-needs.md) (proposed) | 7h | What the file manager needs: System chords the compositor does not take reach the focused client with `SYSTEM` (a change to INV-0160-3); pointer modifiers; `Commands` to every client; one unasked `Offer` within a second of a gesture in the window (a change to INV-0170-2); `DragEnded`'s place on the source's own window; `FreeSpace`; `OpenTerminal`. The file manager takes Ctrl where it takes Command, so it works if points 1 and 2 are refused. |
| An INV-0245-3 for the sheet's guard | fixes | ADR-0245's condition (1) is built and tested (`tests_sheets.rs`) but has no invariant row; the architect may add one (review of the fixes, 12). |
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

Merged to `main` at `f984e21` once pushes were allowed; `stage-7a` and `zosh-1` are on it, tagged under the
architect's rule that a CI failing only on `running_out_of_asids_is_an_error` counts as green (the fault was fixed on
`fix/asid` by another agent). Before that, a full `make test` from the snapshot worktree passed at `ad9becd`.

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

Merged at `a0f8f61`. Its CI on `main` failed: `term` panicked when the compositor answered late at scale 2 under
CI's TCG (`fix/term-wait`, merged at `e73a2b1`, made the wait longer; its CI then failed only on the GUI job's
timeout). `tools/test-tiers` made `term` wait as long as the compositor lives, made the wallpaper cheap (9.3 s to
0.28 s at scale 2 under TCG) and gave the GUI job 30 minutes; its merge `039ba9e` was the first green commit on `main`
containing 7b (run 42683536960), and `stage-7b` is on it.

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

The full CI on the branch passed (run 42684037780); merged at `1b61843`, whose CI on `main` passed after a rerun of
the POSIX scenarios job (Ctrl-Z that did not stop a job, `docs/backlog.md`; still open: it failed again in 7g). `stage-7c`
is on `1b61843`.

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

### After the architect's decisions (2026-11-09)

- **ADR-0242's condition.** A terminal's shell holds a start-up record of exactly its room's grants, never the volume's
  root; the first terminal holds what `home` gives (`+r /`, `+w ~`, `+w /config`, `+w /tmp`, accepted and recorded in
  ADR-0172). `rooms.script` writes outside `~/project` from the room's terminal and reads ADR-0153's hint.
- **The toolchain in rooms** (the architect's decision, ADR-0172): a trusted room's shell also holds `+r /bin`,
  `+r /usr`, `+r /etc` unless a room grant covers one. Holding them was not enough: a terminal launched by the panel had
  no broker, so `echo hi | tr h H` printed nothing. The panel now starts a broker beside every shell it launches, as
  `init` does for its first (the start moved to `user/src/beside.rs`), and a broker with no first shell ends when its
  shell's connection closes and nothing it started runs (`zoraya-broker`'s `abandoned`, with a host test). A shell
  holding a table follows a link inside the covering entry (`/bin/tr -> busybox`), never out of it.
  `tests/posix/room-gui.script`, in `make test-tour-gui`, runs `tr`, `ls` and `uname` in a room's terminal on the
  tour's image. `rooms.script` checks that the room's terminal has exactly five pills (no `+w /`) by where its badge
  falls (a check at scale 1; `make test` runs rooms at scale 1 only).
- **`/config/rooms` written in place**: accepted for now; the crash window is a backlog row.
- **Show-commands** (the architect's review of `commands-qemu.png`): each window's tag is drawn with its window,
  clipped to its frame, at its body's lower left, so a window above covers it and it never lies over another window or
  stacks with another tag; it shows the close gesture's line only (minimise hides a window, which is not the shell's
  `bg`, so it moved to the gestures without text, with the reason); a dock tile whose name and command are the same
  word shows one line; a chip keeps its own width, centred over its line. A host test turns the mode on over two
  overlapping windows and checks that only each window's tag zone changes. The screen is retaken.

### Screens

Under `docs/research/desktop/shots/7d/` (712 KB):

- `rooms-qemu.png` (QEMU, HVF): a `home` terminal and two `project` terminals, the field around `project`'s windows,
  the tinted title bar with its pills (`+w /`, `+w ~/project`, `+r /system`) and its badge, and the chips in the dock.
  Against `clear-field/rooms.png`: the field, the tint, the badge and the chips as in the mockup. Not there: the Rooms
  sheet (System+R), which no 7d ADR asks for (the shell's `room` is the way in); the field's header tag is hidden
  under the `home` window here; the untrusted room (no way to make one, as above); the clock; the other applications'
  tiles. Retaken after ADR-0242's condition: the room's terminal shows its grants and the toolchain (`+w ~/project`,
  `+r /texts`, `+r /bin`, `+r /usr`, `+r /etc`) and no `+w /`; the first terminal shows `home`'s four.
- `dock-qemu.png`: dock B with the terminal tile, its two running dots and the `home` chip, against `clear-field/dock.png`;
  the same materials, sizes and dots, with one tile instead of five.
- `commands-qemu.png` (retaken after the architect's review): show-commands mode. Each window's `kill term` tag sits
  inside its own frame at the body's lower left, covered by the windows above it as the mockup's are; the dock's
  terminal tile has one line (`term` is its name and its command), each chip its own width with its line centred under
  it. Against `clear-field/commands.png`: the same placement and materials; the mockup's room tag on a window
  (`room project`) and its `bg` tag (for a program started with `&`) are not drawn, since minimising is not the
  shell's `bg` and the field's header names the room.
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

After the architect's decisions (ADR-0172's record, the toolchain read-only in a trusted room's shell) and the
show-commands fix, the branch's full CI (run 42686860180) failed in both GUI tours: on CI's slow runner the ends of
long typed lines were lost, as `main`'s were locally with QEMU moved to the efficiency cores. `fix/type-pacing`
(merged at `d03bff1`) makes the GUI runner wait for the guest every eight keys. The branch's CI then passed (run
42707071806, the scenarios job after a rerun); merged at `e161ec7`, green on `main` (run 42710913168); `stage-7d`.

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

### Mutation testing

`make mutants-host` with `--in-diff` against `work/7d`, one crate after another (47 minutes, 44 of them the
compositor); the survivors sorted by the `mutant-triage` subagent; tests added for the holes; every equivalent
rechecked by the main model; then the crates other than the compositor run again (4 minutes), and the compositor's
holes applied by hand one by one, six of them kept in `tools/mutants/desktop-7e.toml` (`make mutants`: 6 caught).

| Crate | Mutants | Missed first | Missed again | Of those, equivalent |
|---|---|---|---|---|
| `zoraya-files` | 31 | 0 | - | - |
| `zoraya-chooser` | 145 | 24 | 4 | 4 |
| `zoraya-fsclient` | 75 | 9 | 0 (10 time out) | - |
| `zoraya-proto-chooser` | 0 (all generated by `protocol!`) | - | - | - |
| `zoraya-panel` | 13 | 0 | - | - |
| `zoraya-app` | 48 | 6 | 0 | - |
| `zoraya-ui` | 45 | 6 | 1 | 1 |
| `zoraya-compositor` | 199 | 56 | not run again (by hand) | 6 |

- **Tests added** (`9c61734` to `af29409`): the drag's keys, the drop's place at both scales, a press from a lost
  tablet, the target's pill, the carried label's pixels and its cleaning; a sheet's refusals (zero width, too tall,
  over the budget), owner, focus, stacking above its owner's popups, presses on its strip and its edge, and the
  focus after it closes; raising a window with its popups and sheet and no other; the chooser's heading kinds, sizes
  near 10 kB, the scroll, the events of other windows, the Up and Open buttons; the file client against replies the
  real service never sends (a token without a handle, an error with one, exactly 16 MiB, no space); `zoraya-app`'s
  sheet, its longest dragged name and `replace`; `zoraya-ui`'s ids 0 and 1.
- **A bug found by the tests** (`dc5d6b0`): the carried label's damaged room was 520 logical pixels, and a 64-byte
  name of wide letters measures over 700, so its end was drawn outside the damage and stayed on screen. The room is
  now the measured name's.
- **The equivalents**: in the compositor, four shifts of the label's damaged room that stay left of or above the
  label, a room of `(520 * 64) * s` clipped to the screen, and dirtying other windows' frames in `add_entry` (both
  recompose an unchanged scene: only the flush's extent grows); in the chooser, `e.dir` in `pending` (Pick lists only
  folders), `<=` at `i == top` in the scroll, `>=` for two lengths that never tie, and the sheet's `Escapes::measure`
  (nothing measures the root; `Window` lays it over the window's size); in `zoraya-ui`, `Button::id` as `None`, which
  shows only when two widgets share an id.
- **Not verified**: the compositor's `2046:16` (`+=` to `*=`) loops forever under the mutant; it was not run by hand,
  since a test that never returns outlives the runner's alarm.

### Merge

After `main` was merged in (7d's later work: the broker beside a shell, the show-commands tags per window), the
branch's full CI passed on the first run (42711274447). Merged at
`32f8a44`, green on `main` (run 42714400871); `stage-7e`.

### Time

From the commits' times: 01:38 to 21:16 on 2026-11-09, interleaved with 7d's decisions, the fixes on `main` and
7f; the mutation runs 47 and 4 minutes, the triage 12 minutes, the tests for its holes about 28 minutes.

## 7f: editor, settings, configuration, clipboard

Branch `work/7f`, from `work/7e`.

### What was built

- **List settings** (ADR-0171): the type `list of TYPE [MIN MAX] [unique]` in `zoraya-describe`, the reader and
  writer in `zoraya-settings`, and `settings NAME ITEM...`, `+ITEM [--at N]` and `-ITEM` in `zosh`.
- **The desktop's settings** (ADR-0169): `gui.theme`, `gui.motion`, `gui.scale`, `gui.wallpaper`,
  `gui.keyboard-layouts` and `gui.keyboard-layout`, declared in `user/describe/system/compd.cmd`, which `compd`
  embeds; the compositor reads `/config/settings` once at start and applies a change only through `ApplySettings` on
  its manager channel, checking every value again; the panel turns the settings window's `SettingsChanged` into
  `ApplySettings`.
- **The settings window** (`settings`, its logic in `zoraya-settings-window`): appearance and keyboard on one page;
  each change written at once by publishing a new file over the old one through `/config` narrowed to `settings`.
- **The editor** (`edit`, its buffer and view in `zoraya-editor`): one file opened from the file manager (read only)
  or through the chooser, saved through `Save`; cursor and selection by characters, undo and redo, find, and a
  question before closing over unsaved changes.
- **The clipboard** (ADR-0170): System+C, System+X and System+V; `Copy`, `Offer` and `Paste`; `edit` copies, cuts
  and pastes. `zoraya-app` repaints its windows in the theme a `Configure` brings.

### The 7f acceptance test

`plan.md`: the settings window changes the theme and the frames follow; TASK-24 adds the clipboard and the
editor. Done: `tests/gui/settings.script` (61 steps, HVF and TCG, in `make test-gui`) turns the theme dark in the
settings window and reads the frames back dark, enables Czech and puts it in use and types by it in the terminal at
once, takes Czech text in `edit` on that layout, saves `poznámky.txt` through the chooser's sheet, and finds it in the
file manager's list. `tests/gui/files.script` opens `.txt` and `.md` files in `edit` and `.test` ones in `t_open`.

### Decisions taken without the architect

- No new ADR. Within ADR-0169: the scale applies at the next session (its open question), and a `settings` command
  in a terminal reaches the running compositor at the next session too.
- Within ADR-0170: an `Offer` is kept only from the client just sent `Copy`, within 500 ms, once; a paste into an
  untrusted room's window is held back as drops are, until the confirming card exists.
- For the architect: INV-0168-2 gives `files` the volume's root, which includes `/config`, while INV-0169-4 says no
  launch but the settings window gets `/config` to write. The panel's test checks the `Config` slot only (review 7f,
  item 7).

### Screens

Under `docs/research/desktop/shots/7f/` (HVF, scale 1):

- `settings-qemu.png` against `clear-field/settings.png`, and `dark-qemu.png` against `settings-dark.png`. As in the
  mockup: the title with its two pills, the page's sections, the theme's two choices, motion as a switch. Not as in
  the mockup: no sidebar of sections and no card around the controls (one page, the toolkit has no card yet), square
  check marks for the choices where the mockup has round radios, the scale as three choices on their own lines, and
  no line of the equivalent `settings` command under the controls. The terminal stays light in the dark theme (its
  screen is read by the light set's glyphs in the GUI tests).
- `czech-qemu.png`: Czech in use, the bar's `CZ`, Czech typed in the terminal.
- `edit-qemu.png`: the editor with Czech text; `save-sheet-qemu.png`: its Save sheet; `files-lists-qemu.png`: the
  file manager listing the saved file.

### Measurements

A change in the settings window, in `docs/design.md`: written in 0.15 to 0.27 ms and applied in 0.3 ms under HVF
(20 ms for the theme, which restyles every window and makes the wallpaper's blur again); under TCG 3.8 to 35 ms and
4.3 to 34 ms.

### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0169-1, -3 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0169-2, -4 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0170-1, -2, -5; the clipboard's halves of -3 and -4 | not yet applicable (stage 7); 7e: drops | checked: `make test-unit` |
| INV-0171-1 to -3 | not yet applicable (stage 7) | checked: `make test-unit` |

### Review

`docs/tasks/TASK-24-review-7f.md`: 2 errors, 5 warnings, 9 notes. Both errors (a clipboard of more than 4 KiB ended
the copying application; a paste of more than about 490 bytes was lost) fixed by reading and writing the protocol's
largest message; every warning fixed. The check of the changed lines confirmed the fixes and found a new error (a
paste over 8 KiB ended a focused terminal), fixed, and two gaps, fixed. The notes left are recorded in the review.

### Mutation testing

`make mutants-host` with `--in-diff` against `work/7e`, one crate after another (14 minutes); the survivors sorted
by the `mutant-triage` subagent; tests added for every hole; every equivalent rechecked by the main model; each hole
applied again by hand from lists in `tools/mutants/` (`editor.toml`, `shell-settings.toml`,
`compositor-settings.toml`, `settings-7f.toml`: 90 mutants, 90 caught).

| Crate | Mutants | Missed first | Equivalent | Holes, now caught |
|---|---|---|---|---|
| `zoraya-editor` | 316 | 128 | 7 | 121 |
| `zoraya-shell` | 22 | 17 | 1 | 16 |
| `zoraya-compositor` | 59 | 10 | - | 10 |
| `zoraya-panel` | 12 | 8 | 5 | 3 |
| `zoraya-describe` | 43 | 7 | - | 7 |
| `zoraya-settings-window` | 33 | 6 | - | 6 |
| `zoraya-app` | 32 | 6 | - | 6 |
| `zoraya-settings` | 16 | 0 | - | - |

- The equivalents: the editor's view `id` (3: its only reader finds no value in the view either way), `<=` for `<`
  where equality assigns the same value (`follow`) or draws an empty rectangle (a selection's cell), Down on the last
  line (2 forms that reach the same end of text); the shell's `>=` for `>` with one word; the panel's `^` for `|`
  over six distinct bits (5).
- The `zoraya-shell` hunk (`settings`' `--at` and `--`) was reached by no host test: the test host now describes
  `settings`. The editor's view painted no text in its one paint test (the cursor bar alone passed it); the new tests
  paint at an offset and check rows, cells, the selection and the bar.
- The settings description said `auto` gives scale 2 from 2048 pixels wide; the compositor's threshold is 1920. The
  description now says 1920.

## 7g: the display follows the window

Branch `work/7g`, from `work/7f`. Added to TASK-24 by the architect on 2026-11-09, after the `zoom-to-fit=on` change to
`tools/run-qemu.sh` (merged at `cc4e2dd`), which makes QEMU's macOS window resizable.

### What was built

- Proposed **ADR-0246**: the display follows the window, superseding ADR-0164's size fixed at boot (its scale,
  protocol and invariants stand). Read in QEMU 11.1.2's source: the resizable cocoa window hands its size to the
  console at each resize, which virtio-gpu reports as its display event; QEMU's QMP has no command that resizes a
  console.
- **`zoraya-proto-display`**: `Resized { width, height }`, the driver's first unasked message.
- **`zoraya-virtio-gpu`**: the display event (`events_read`, cleared through `events_clear`); a second `Attach` at a
  new size makes the other resource, shows it on scanout 0 and only then unreferences the old one; flushes go to the
  resource shown, at its size; `Announce`, which says when the driver may tell a new size.
- **`gpud`** looks for the event every 100 ms between requests, sends `Resized`, and unpins the old framebuffer once
  the new one is shown.
- **The compositor**: on `Resized`, after any flush in flight, the same `Info` and `Attach` as at attach (none when
  the size came back to its own); the scale from `gui.scale` or the width; popups closed, sheets too at a new scale;
  toplevels kept at their logical size as far as the work area holds them and moved back into it, legacy windows moved
  back, every toplevel configured; `ApplySettings` applies `gui.scale` at once. `compd` releases the framebuffer
  before once the new one is shown.
- **`tools/run-qemu.sh --gui-size WxH`**, and `make test-gui` boots the terminal's script at 1600 × 1000 and
  1920 × 1200 as well.

### The 7g acceptance test

TASK-24 asks for boots at two sizes, a host test of the compositor with a fake display that changes size, and a GUI
test if QEMU can trigger a resize. Done: the terminal's script at 1600 × 1000 (scale 1) and 1920 × 1200 (scale 2
under `auto`) in `make test-gui`, beside 1024 × 768 and 2048 × 1536; 15 host tests of the compositor in
`tests_display.rs` (a new size and its scale, a resize behind a flush, a refused attach, a size back at the attached
one, popups, sheets, legacy windows, the work area's edges at scales 1 and 2, the scale from the settings) and three
of `zoraya-virtio-gpu` (the event, the second resource, `Announce`). No GUI test resizes a running guest: QEMU's QMP
cannot. The resize itself is to be tried by hand ("To check by hand").

### Decisions taken without the architect

- Proposed **ADR-0246** (Option C: the driver reports, the compositor re-attaches), over keeping the fixed size,
  restarting the driver at each size, or polling `Info`.
- The display event is looked for every 100 ms instead of on the configuration interrupt: `gpud`'s interrupt wait is
  part of each command, and a register read every 100 ms costs nothing.
- At a new scale sheets close (their clients cancel) and popups close at any re-layout: neither hears `Configure`.

### Screens

Under `docs/research/desktop/shots/7g/` (light, HVF): `terminal-1600x1000-qemu.png` (scale 1: the bar across the
whole width, the dock in its middle), `terminal-1920x1200-qemu.png` (scale 2 under `auto`; the terminal's own 80 × 24
is taller than the work area and runs under the dock, a limitation recorded in `docs/design.md`), and
`settings-scale-qemu.png` (the settings window's note on `Automatic`). There is no mockup of a resized display.

### Measurements

In `docs/design.md`: each new size costs the wallpaper and its blur once (36 and 51 ms under HVF at 1600 × 1000 and
1920 × 1200, 138 and 199 ms under TCG), then a full redraw.

### Invariants

ADR-0246 is proposed: its invariants are not in `docs/adr/invariants.md` until it is accepted. Both have their
checks: INV-0246-1, the compositor's host tests with the fake display; INV-0246-2, `Announce`'s host test.

### Review

`docs/tasks/TASK-24-review-7g.md`: 3 errors, 5 warnings, 6 notes. The errors were all in the handshake: `gpud`
sent a second `Resized` right after `Info`, which the compositor read as `Attach`'s reply and dropped the driver;
`Attach` used the device's newest size instead of the one `Info` gave; a size back at the attached one was attached
again and refused. All fixed (`Announce`; no `Attach` at the same size) with tests, and every warning. The check of
the changed lines confirmed all 14 and added three notes: two fixed, one (the compositor's `call` taking the next
message as its reply) recorded as open.

### Mutation testing

`make mutants-host` with `--in-diff` against `work/7f`: `zoraya-virtio-gpu` 32 mutants, 1 missed (`CONFIG + 0`
read as `CONFIG - 0`: equivalent); the compositor 90 mutants, 32 missed, then tests, then again (21 missed), tests
again, and a third run (27 missed, of which 15 were `carried_rect`'s lines, in the diff only because `work/7f` had
7e's fix of it by then). Of the 12 in 7g's code, 2 were caught by tests added after (applied by hand), 9 are
equivalent (the lower bounds of the move back into the work area, which no window is ever past, since a move keeps
windows on screen and halving a position at scale 2 to 1 keeps it past the bound; and the guards of the upper
bounds on the toplevel path, whose body never exceeds the work area), and 1 is left: the legacy window's upper bound
divided by the border, the same at scale 1. Five of the holes are in `tools/mutants/display-7g.toml` (`make
mutants`: 5 caught). The three compositor runs took 92 minutes, two past the cap.

### Merge

ADR-0246 is proposed, and 7g is merged as the other parts were, at the architect's request in TASK-24's additions.

### Time

From the commits' times: 22:31 on 2026-11-09 to 00:46 on 2026-11-10 for the code, the review and its fixes, and
the tests, with the three mutation runs (92 minutes) overlapping 7e's and 7f's merges.

## The architect's decisions and fixes of 2026-11-10

Branch `fix/7-decisions`, from `main` at `d036de3`.

### What was built

- **Retina** (the architect's check): `make run-gui` on macOS boots at 2048 × 1536 with scale 2 (`GUI_SCALE=1`
  stays); proposed ADR-0247: `gpud` reads the window's physical size from the EDID (`VIRTIO_GPU_F_EDID`,
  `GET_EDID`), `InfoReply` carries it, and `auto` gives scale 2 from 1280 pixels at 150 ppi or more. The settings
  window's note says so. The dock's settings tile is a gear; a tooltip whose name is its line shows one line.
- **ADR-0245 and ADR-0246 accepted** and recorded, with their invariant rows; ADR-0245's two conditions built (the
  sheet's guard, the show-commands line through a new `Commands` event).
- **INV-0168-2 gave way to INV-0169-4**: the panel gives `files` a start-up record (the volume to read, `~` or the
  room's grants to change, nothing reaching `/config`, also through `~`), and no launch gets the root any more
  (`Give::Root` is gone). Recorded in ADR-0168 with the changed rule; its in-QEMU half is 7h's (`files: holds +r /
  +w ~` in `tests/gui/files.script`).
- **A client's size never exceeds the work area**: a new toplevel too tall or wide for the work area is made smaller
  below its cascade place (so the first window's body stays at `first_body`), windows are placed above the dock; a
  test at 1920 × 1200.
- **The Ctrl-Z flake**, traced in the broker (a forked session): the terminal's `Suspend` named the shell's own
  group, which ignores SIGTSTP; the job's `tcsetpgrp` came a moment later on CI's slow runner. The script waits for
  a word the job prints before Ctrl-Z, three rounds; a broker test forces the interleaving (it describes behaviour
  the branch did not change: the bug was the script's).
- **Housekeeping**: the merged remote branches deleted (`work/7a` to `work/7g`, `fix/term-wait`, `fix/term-no-panic`,
  `fix/type-pacing`, `tools/test-tiers`, `tools/zoom-to-fit`); `.claude/agents/` gained `ci-watch`, `log-triage`
  (haiku) and `opus-review` (opus), and `CLAUDE.md` says Sonnet sessions hand reviews, kernel, `unsafe` and security
  boundaries to `opus-review`.

### Review

`docs/tasks/TASK-24-review-fixes.md`: 1 error (a key pressed within the guard reached the sheet through its repeats),
4 warnings, 8 notes; fixed with tests or recorded where they apply. Left as limitations: `Commands` is lost when a
client's channel is full (the next toggle tells it again); the Ctrl-Z fix assumes the terminal and the broker agree on
the foreground once the job has printed.

### Mutation testing

`tools/mutants/display-density.toml` (4 hand mutants of the EDID and the dense rule): 4 caught. The sheet's guard was
checked by removing each half (time, held keys): each removal fails a test.

### Merge

Full CI on the branch green (run 42765790151, after a run cancelled by mistake); merged into `main` at `10267d0`;
`main`'s CI green (run 42768220409). No tag: not a part of the stage; 7h, which contains it, is tagged.

## 7h: the file manager

Branch `work/7h`, from `fix/7-decisions`, with `design/files` (the mockups) merged in. Asked by the architect on
2026-11-10: a mockup first (approved, with a context menu added), then the implementation.

### What was built

See `docs/design.md`, "The file manager (as built)": the model and the window in `zoraya-files`, the program, proposed
ADR-0248's widenings in the compositor, the file protocol and the panel, and the runner's new steps.

### The 7h acceptance test

The architect asked for GUI tests of each feature and screens compared with the mockup.
`tests/gui/files-manage.script` (99 steps, in `make test-gui` under HVF and TCG) runs selection by click, Shift and
Command; the Size and Name headers; arrows, Enter into a folder and Command+Up; search; F2 rename; Command+Shift+N;
Delete with its card; a drag onto a folder that moves and one with Option that copies; a double click; the menu's Copy
path and Open terminal here; the menu in show-commands mode; Up, Back and the Home place. `tests/gui/files.script`
keeps 7e's flow (opening through the panel, the chooser, a drop that grants) on the new window. The file manager
prints its holds, its status line and its order on the console, which the scripts read.

### Decisions taken without the architect

- Proposed ADR-0248 (the seven widenings above) instead of Ctrl-only keys and no Copy path, drop on folders or free
  space.
- A drop on one of the window's folders moves within one place and copies otherwise (or with Option), as Haiku's
  Tracker does; from a place the window only reads it copies.
- The room's own grants in the sidebar carry a neutral ring, not the room's colour: colour means a room and only the
  compositor draws it (ADR-0172).
- The window's title stays "Files" (the mockup's "project — files" names the room, which the frame's badge shows).
- A delete removes a folder whole, as `rm --recursive` would, after the card names it.

### Screens

Under `docs/research/desktop/shots/7h/` (light, HVF, scale 1 and 2), beside the mockups they follow: `files-home-qemu.png` (the
mockup's `files.png`: the places, the toolbar, the list, the status line with free space), `files-select-qemu.png`
(`files-select.png`: a Shift range and a Command pick), `files-rename-qemu.png` (`files-rename.png`),
`files-delete-qemu.png` (`files-delete.png`), `files-menu-qemu.png` and `files-menu-commands-qemu.png` (the menu and
its `zosh` lines), each also `@2x`. As in the mockups but for: `home`'s single place (the mockups show the room
`project`), no `/system` place (not a folder of the volume), a neutral ring for a room's own grants (no room
colour), the title "Files", and the dock and the show-commands tags the compositor draws.

### Measurements

In `docs/design.md`: listing a folder of 2 to 5 items with each item's size and time takes 0.17 to 0.50 ms under HVF
(`files: listed N items in T us`, `tests/gui/files-manage.script`, 2026-11-10); right after a change it takes 6 to
28 ms, the file service committing first.

### Invariants

ADR-0248 is proposed: its four invariants are not in `docs/adr/invariants.md` until it is accepted; each has its host
test (`tests_windows.rs`, `tests_drops.rs`, `zoraya-panel`'s `terminal_at` test). INV-0160-3 and INV-0170-2 are
changed by it and still marked checked for what they say outside its points.

### Review

`docs/tasks/TASK-24-review-7h.md`: 4 errors (the host tests red at the reviewed commit; the gesture offer open to
any key and any client, untrusted ones included; the gesture outliving a focus change and cancelling another's copy;
`OpenTerminal` falling back to `home` for a room gone), 5 warnings, 7 notes. All errors fixed with tests; the
warnings fixed or recorded (the two invariants ADR-0248 changes, the in-place message changes, copy and delete's
partial results).

### Mutation testing

Under the 30-minute cap, for the trust boundary and grants only (the panel's launch logic and the compositor's
changed lines; `zoraya-files`, plain UI code, skipped). `make mutants-host` with `--in-diff` against the fix branch:
`zoraya-panel` 22 mutants (3 unviable), 16 caught, 3 missed, in 14 s; `zoraya-compositor` 52 mutants, 43 caught,
9 missed, in 14 minutes. Missed and answered: `terminal_at`'s control-character and `~user` guards and `--room`
only for `files` (a test each: `terminal_at_refuses_odd_paths_and_only_files_gets_a_room_argument`); a compositor
chord acting on a repeat or a release, and a forwarded chord's modifiers (`a_chord_acts_on_its_press_and_a_forwarded_one_keeps_its_modifiers`);
the place of a drop at scale 2, the gesture surviving a keyboard focus change (the earlier test was vacuous: a press
replaced it) and a client's end (`a_place_is_logical_and_the_gesture_goes_with_focus_and_client`). Equivalent: the
`|` against `^` of the system bit in `modifiers_now` (the keymap never sets that bit) and in the forwarded key's
modifiers (the same). The new tests were not run against the mutants again (the cap).

### The CI's first run

The first full CI run (42773494801) failed in `make test-gui` only: `files-manage` under TCG. Reproduced locally with
`GUI_RUNS=files-manage:tcg:1` (the local default skips TCG; `FULL=1` or a named run does it). Two causes: the
runner sent a modifier key and a click as two devices' events too close together (a Command-click arrived plain), so it
now holds the modifier 400 ms around the click; and a slow guest answered a press later than ADR-0170's 500 ms
`[assumption]` for naming it in `Drag` (the press must still be held): measured, the folder drag fails at 500 ms and
passes at 3 s, so `Drag` takes a held press within 3 s (proposed ADR-0248, point 4b; the copy reply keeps 500 ms).
The runner also holds a drag's button 800 ms after its last move. The second run (42777712396) failed in the same
script at the rename: `serial` matches the whole log, so a wait for a status line that had printed before was
satisfied at once and F2 (the keyboard) overtook the click (the tablet). The runner gained `then TEXT`, which waits for
output after the last input step; `files-manage` uses it throughout, and `files` prints when the search takes the keys,
when the menu opens or closes and when it asks before a delete, for the next key to wait for. Reproduced with QEMU on
the efficiency cores (`taskpolicy -b`), as in 7d.

### Merge

MERGE_7H

## To check by hand

- **The resizable window** (`tools/run-qemu.sh`'s `zoom-to-fit=on`, merged at `cc4e2dd`): no test opens the cocoa
  window. With `make run-gui`, enlarge QEMU's window, then check that a click lands where the pointer points (a title
  bar's close button, a dock tile) and that Command+Space still switches the layout.
- **The display that follows the window** (7g): with `make run-gui`, drag QEMU's window larger and smaller; the bar
  and the dock should follow, windows stay in the work area, and past 1920 pixels wide the desktop turns to scale 2.
  Cocoa reports pixels (read in QEMU's source, ADR-0247); on a Retina screen scale 2 should hold from 1280 pixels.
- **Retina** (the fixes): `make run-gui` on a Retina Mac should open at scale 2 with text of a monitor's size; a
  window made smaller keeps scale 2 down to 1280 pixels (640 points).

To try the desktop by hand: `make run-gui`; the window shows the terminal in JetBrains Mono. `ls /texts/uk`, `cat
/texts/uk/` then Tab and Enter, and the same for `/texts/cz/`, show Ukrainian and Czech text; Shift with Page Up
scrolls back.

From 7b on: drag a window by its title bar, System+Left and System+Right tile it, Alt+Tab
opens the switcher, grouped by room. From 7c: System+Space switches the layout (`us`, `ua`, `cz`, shown at the right
of the bar). From 7d: the terminal tile in the dock (or System+Enter) opens another terminal; in a terminal, `room new
project +w ~/project +r /system` makes a room, its chip appears in the dock; press it, then the terminal tile, and
the new terminal's title bar carries the room's tint, badge and pills, with a field around its windows while `home`
has windows too. System+? shows every gesture's text. From 7e: the Files tile opens the file manager; a `.txt`
opened there opens in the editor, read only, and a drag of it onto another window gives that window the file (from
7h, the file manager of the mockups: see design.md's "Using the desktop"). From
7f: the Settings tile (theme, motion and keyboard layouts applied at once, the scale at the next session), the Edit tile
(open and save through the chooser's sheet), and System+C, System+X, System+V in the editor.
