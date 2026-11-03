# Stage 7 outline

Non-binding, part of TASK-19. It orders the work; the ADRs decide. Numbers refer to proposed ADR-0160 to ADR-0169 in
`docs/adr/`. Nothing here starts before the architect accepts the ADRs it depends on and chooses a mockup direction
(the values of ADR-0161's tokens), and every task ends with `make test` passing from a clean checkout. The tasks are
cut to comparable size (each a few thousand lines of code and tests, by estimate) and so that each has a visible
result on its own: a screen the architect can look at.

Dependencies outside stage 7: on the accepted shell ADRs, ADR-0153 (the grant vocabulary ADR-0167 uses, and its
`narrow`), ADR-0157 (the configuration file and its crate, which ADR-0169 uses) and ADR-0155 (`exit` in the GUI
terminal), built by TASK-18 Part 2; on
**stage 6** only if the architect chooses a `std` over `libposix` (ADR-0166 recommends none in stage 7, so nothing
below waits for stage 6) and for POSIX programs started from the desktop, which is deferred.

| Task | Work | Depends on | Done when | Risk |
|---|---|---|---|---|
| 7a | **Tokens and text.** `zoraya-tokens` from the chosen direction's token file with its `const` parser, light and dark; `zoraya-text` (the `glyf` reader, the fixed-point scan-line rasteriser, the glyph cache, the simple shaper behind the `Shaper` trait, fallback by font list, the box glyph); `fonts/` with the files and licences and the table in `docs/dependencies.md`; the runner's screendump reader over the new crate; the terminal on the new text with its cell grid; INV-0161-1 to -4 and INV-0162-1 to -5 checks. | ADR-0161, ADR-0162, the direction chosen | `make test-gui` reads Ukrainian and Czech names typed into the terminal back from a screendump at scale 1; the mutation fuzz of the font reader runs under `make test-unit` | the fixed-point rasteriser's determinism across HVF and TCG; the cost of rendering a full 80 by 24 grid of Cyrillic under TCG |
| 7b | **Frames, windows and scale.** ADR-0160 in the compositor: frames from the tokens, title text, buttons, drag to move and resize, minimise and close, popups, several windows per client, `Configure`, `CloseRequested`, the system key and its chords, the switcher overlay, system windows; ADR-0164's scale in `Display` and `Configure`, `tools/run-qemu.sh --gui-scale 2`, the second `make test-gui` run; `init` passes the scale setting until `confd` exists. INV-0160-1 to -6 and INV-0164-1 to -3. | ADR-0160, ADR-0164, 7a | the GUI test moves, minimises, restores and closes a terminal window over QMP, switches with Alt+Tab and F12, and passes at both scales | the Mac's Command key under QEMU; compose cost of shadows under TCG |
| 7c | **Keyboard layouts.** The four tables, the dead-key machine, switching, the `Layout` event and the frame indicator; the `Title` message from `zosh` (ADR-0168). INV-0163-1 to -4. | ADR-0163, 7b, ADR-0154 | the GUI test switches to `ua`, types a Ukrainian name, switches to `cz`, types `příklad.txt` through dead keys, and reads both back | AltGr on the Mac keyboard under QEMU |
| 7d | **Toolkit and panel.** `zoraya-ui` (the first dozen widgets, layout, raster, events) and `zoraya-app` (loop, windows, popups, platform trait with a host fake); the panel: launcher, window list, layout indicator, clock, launch records, as the program that starts applications; `init`'s session table shrinks (ADR-0168). INV-0165-1 to -4, INV-0168-1 and -3. | ADR-0165, ADR-0168, 7b | a second terminal starts from the panel with the handles its launch record lists, hidden windows restore from the panel, and the widget tests run on the host against the tokens | the toolkit's event model meeting the compositor's one-commit-in-flight rule |
| 7e | **Chooser and files.** `zoraya-proto-chooser` and the `chooser` service as a system window; `zoraya-fsclient`; the file manager with its list, open-with through the panel, new folder, rename, delete; the application data directory per launch. INV-0167-1 to -4, INV-0168-2. | ADR-0167, 7d, ADR-0153's `narrow` (TASK-18 Part 2) | the GUI test opens a file from the file manager in a test application through the chooser over QMP, the application reads it and cannot write it (`Open`) or can (`Save`) | the chooser's rights narrowing against the service's `clone`; two programs holding the root |
| 7f | **Editor, settings, configuration.** `edit` (multi-line text view, selection, undo, find, open and save through the chooser, unsaved-changes dialog); the `gui.*` declarations, `SettingsChanged` and `ApplySettings` over ADR-0157's file and crate; the settings window; the compositor and the panel applying; `Configure` carrying theme and motion; the stage 7 roadmap row, `docs/design.md`'s desktop sections and the backlog. INV-0169-1 to -4. | ADR-0169, 7c, 7e, ADR-0157's crate (TASK-18 Part 2) | the GUI test changes the theme and the active layout in settings and sees the frame and the indicator change without a restart; `edit` saves a Czech file the file manager then lists | TASK-18's file format and location; the atomic publish of the file from a service that is not `fsd` |

Order of risk: 7a carries the one unknown that everything else shows (whether the rasteriser is fast and
deterministic enough), so it goes first and measures; 7b is compositor work on known ground; 7c is small; 7d is
the largest in lines; 7e and 7f depend on TASK-18 Part 2 (`narrow`, ADR-0157's crate) and can swap places with 7c and 7d if it is late.
7a and the panel half of 7d could run in parallel, since the panel needs only ADR-0095's font until 7a lands.

What each task measures on the way, for `docs/design.md` and the task report: 7a the time to rasterise and draw a
full terminal grid and a glyph cache's hit rate under HVF and TCG; 7b compose time per frame with frames and
shadows, and the switcher's cost; 7c nothing; 7d the toolkit's paint time for the settings window; 7e the
chooser's round trip; 7f the settings write and the `ApplySettings` round trip.

What is deliberately not in stage 7: an image viewer (a PNG decoder first), the clipboard and drag-and-drop
(ADR-0170, to be written), a Rust `std` (ADR-0166), per-window keyboard layouts, fractional scaling, tiling beyond
System+Left and System+Right, accessibility, rich text, POSIX programs started from the desktop, person-installed
themes and layouts. Each has a revisit trigger in its ADR.
