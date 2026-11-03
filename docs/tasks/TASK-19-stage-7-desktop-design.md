# TASK-19: Design for stage 7 — the desktop (proposed ADRs and visual mockups)

## Where this runs

A **cloud session**, in parallel with stage 6 (6a, 6b on the architect's Mac) and with TASK-18 Part 1 (shell design). Needs the repository and web access. **No guest code, no QEMU.** Host-side code is allowed only for the mockup renderer described below.

## Why

Stage 5 gave Zoraya a working GUI: a compositor, a terminal window, a bitmap ASCII font. Stage 7 turns it into a desktop a person enjoys using. The architect's direction:

- **Beautiful in the macOS sense:** one design system applied everywhere, good typography, calm and consistent visuals, smooth motion.
- **Simple and unsurprising in the FreeBSD sense:** one coherent design, the principle of least astonishment (POLA).
- **Native to Zoraya's model:** an application reaches only what it is given (ADR-0002). The desktop is where a person grants things to applications, as the shell is on the command line (TASK-18).
- The isolation direction (running untrusted code such as AI agents) stays open and must not be precluded.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (Vision, "Using the GUI", Known limitations), `docs/adr/index.md`, `docs/backlog.md`.

Read **in full**: ADR-0002, ADR-0051, ADR-0052, ADR-0053, ADR-0090 to ADR-0097 (stage 5), ADR-0100 and ADR-0109 (for the question of a Rust `std` and C libraries). Read the research catalog entries in category 13 (graphics, input, window systems) and 16 (text, encoding, locale) of `docs/research/design-mistakes.md` and `held-up-well.md`, and the Plan 9, Haiku/BeOS, Fuchsia, Genode and KeyKOS/EROS sections of `alternatives-postmortem.md`. If the branch `shell/design` exists on GitHub, read its ADRs on capability grants and command conventions: the desktop must be consistent with them.

ADR numbers **0160 to 0179** are reserved for this task.

## Part 1: research

Write `docs/research/desktop/survey.md`, same evidence rules as TASK-01 (primary sources preferred, every URL fetched, never invent a citation, paraphrase):

- Desktops and their design systems: macOS and Apple's Human Interface Guidelines, Haiku, SerenityOS, GNOME's HIG, and one or two others you find relevant. What makes each feel coherent or not.
- Window management models: server-side vs client-side decorations (the security and consistency arguments), tiling vs floating, how focus and stacking work.
- Text: TrueType/OpenType rasterisation in Rust without `std` (for example `fontdue`, `ab_glyph`), hinting and antialiasing, what scripts need shaping and what Latin and Cyrillic need without it, font fallback. Fonts under licences compatible with redistribution next to BSD code (SIL OFL and similar); record each candidate font's licence.
- Widget toolkits that could run on Zoraya: own `no_std` widgets, LVGL (MIT, C), Slint (record its licence terms and why they matter), egui/iced (need a Rust `std`).
- Capability-oriented desktops: the "powerbox" pattern (a trusted file chooser that grants the chosen file to the application), from CapDesk/E, Polaris, EROS's trusted window system, and macOS's sandboxed open panel.
- Porting Rust's `std`: what a new target needs (platform layer for files, time, threads, synchronisation, environment, processes), what Redox and Fuchsia did, and the cost.

## Part 2: decisions (proposed ADRs, one decision per ADR)

Template-exact, `proposed`, "Architect's decision" empty, at least three options each with a Linux baseline (X11 or Wayland with GTK/Qt) and a non-Unix precedent, steelmanned, with "Effect of the vision". At least:

1. **Window management.** Who draws decorations (recommend with the spoofing argument in mind), moving, resizing, minimising, stacking, focus, multiple terminals; keyboard and pointer behaviour; how ADR-0094's focus rules extend.
2. **Design system.** Colour palette (light and dark), type scale, spacing grid, corner radii, shadows, iconography rules, motion durations and curves, and how the design system is expressed once in code (a tokens crate) so every component uses it. Present the options as the mockups below.
3. **Fonts and text.** Rasteriser (own or a crate), fonts and their licences, sizes and antialiasing, UTF-8 everywhere, the scripts supported in stage 7 (at least Latin, Cyrillic including Ukrainian letters, and Czech diacritics), fallback, what is out of scope (complex shaping, right-to-left) and how it stays possible later.
4. **Keyboard layouts.** At least US, Ukrainian, Russian and Czech layouts, switching between them, where layouts live (ADR-0092 puts the layout in the compositor), and how the GUI terminal and `zosh` receive non-ASCII input.
5. **Display scaling.** A scale factor for high-density displays (the Mac's window), integer or fractional, and how clients render for it.
6. **Widget toolkit.** Own `no_std` widgets, LVGL, or a `std`-based toolkit; what each costs; how a toolkit talks to the compositor (ADR-0093) and uses the design system.
7. **Rust `std` for Zoraya.** Whether to port it in stage 7, and if so how: over `libposix` (a Unix-like target, cheaper, but the native world then depends on the POSIX personality) or natively over `zoraya-rt` (more work, keeps the native world clean). What it unlocks and what it costs.
8. **Granting files and resources in the GUI.** A system file chooser that grants what the person picked (powerbox), how it relates to the shell's grants (TASK-18), what an application can never do (enumerate files it was not given, read other windows).
9. **The first applications.** Which ones stage 7 builds (for example a launcher or panel, a file manager, a text editor, an image viewer, settings) and in what order; which can wait.
10. **Settings and configuration.** How the desktop's settings relate to the system configuration file the shell task designs (one source of truth).

For each ADR give the invariants a later implementation can check, and how (screendump-based tests are available, see stage 5).

## Part 3: visual mockups

The architect decides visual questions by looking, not by reading. Build a small **host tool** (Rust, under `tools/mockups/`) that renders PNG mockups with the rasteriser and fonts the ADRs propose, from design tokens in a data file:

- **Two or three distinct style directions** (for example: light, close to macOS; minimal and quiet, closer to a classic BSD feel; dark), each as the same set of screens:
  - the desktop with two terminal windows, one focused;
  - a terminal showing a `zosh` session with Ukrainian and Czech file names;
  - the file chooser granting a file to an application;
  - a settings window;
  - the window-switching overlay.
- Rendered at scale 1 and scale 2.
- Saved under `docs/research/desktop/mockups/<direction>/*.png`, with `docs/research/desktop/mockups/README.md` describing each direction in a few lines and the tokens behind it.

If the chosen fonts cannot be downloaded in this environment, say so, use fonts that can be fetched and are licensed for redistribution, and record the substitution. Mockups are illustrations, not the implementation; keep the tool small.

Also write `docs/research/desktop/plan.md`: implementation order for stage 7 in tasks of comparable size, with dependencies on ADRs, on stage 6 (if a `std` over `libposix` is chosen) and on TASK-18.

## Review

Independent review by a subagent that did not write the ADRs: citations; options not strawmanned; consistency with accepted ADRs and with the shell design if present (list tensions explicitly); whether the mockups match the ADRs; licences of every font and crate. Findings and dispositions in `docs/research/desktop/review-notes.md`; fix them; one check of the changed lines; if it finds errors, fix and repeat that check at most once more, then report.

## Git

- Branch `adr/stage-7-design` from the latest `main`.
- Configured identity only. If none, stop and ask.
- Commit after each part. **You may push this one branch and nothing else.** Never merge, never push other branches, never force-push.
- Touch only `docs/adr/0160-*` to `docs/adr/0179-*`, `docs/research/desktop/` and `tools/mockups/` (with its own workspace entry, added as a single line). List the `index.md`, `invariants.md` and roadmap rows to add in the final report.

## Definition of done

- Survey; proposed ADRs for decisions 1–10 (merged or split with reasons), each with "Effect of the vision" and invariants.
- Mockups for two or three directions, at two scales, with the README.
- `plan.md`, review and the check of fixes done; branch pushed.
- Final report: each ADR's recommendation in one line, the mockup directions in one line each, tensions with accepted ADRs and with the shell design, and the questions for the architect — first among them, which visual direction.

## Out of scope

Guest code and any change to the compositor or drivers (that is stage 7's implementation, after acceptance); networking; 3D; complex text shaping and right-to-left scripts (only: do not preclude them).
