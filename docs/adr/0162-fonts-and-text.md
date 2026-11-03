# ADR-0162: Fonts and text rendering

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0012, ADR-0013, ADR-0033, ADR-0095, ADR-0096, ADR-0161, ADR-0164
- **Evidence:** CP-22, GR-015, DM-081, DM-096, DM-097, alternatives-postmortem.md#plan-9--inferno, desktop/survey.md#rasterisers-in-rust-without-std, desktop/survey.md#hinting-and-antialiasing, desktop/survey.md#shaping-and-what-latin-and-cyrillic-need, desktop/survey.md#font-fallback, desktop/survey.md#candidate-fonts-and-their-licences, desktop/survey.md#the-ofl-next-to-bsd-code, desktop/survey.md#keyboard-layouts, X-8

## Context
ADR-0095 chose a 5 by 9 bitmap font for printable ASCII and named Unicode as the revisit trigger. Stage 7 needs proportional text for frames, controls and applications, and a terminal that shows Ukrainian and Czech file names (ADR-0013 names are UTF-8; CP-22, GR-015). The questions are the rasteriser (own or a crate), the fonts and their licences, sizes and antialiasing, which scripts stage 7 supports, how fallback works, and what stays out while remaining possible.

What the survey found (`desktop/survey.md#rasterisers-in-rust-without-std`): every Rust rasteriser flattens outlines to segments and accumulates coverage on scan lines; the smallest, `ab_glyph_rasterizer`, is 555 lines and has no dependency; `fontdue` is 1,972 lines over `ttf-parser` and does no hinting; `ttf-parser` is in maintenance mode and recommends another crate; `rustybuzz` is archived. The letters the keyboard layouts produce for Ukrainian, Russian and Czech are all precomposed code points (`desktop/survey.md#shaping-and-what-latin-and-cyrillic-need`, `desktop/survey.md#keyboard-layouts`), so Latin and Cyrillic need a `cmap` lookup and an advance per character, not a shaper; Arabic, Indic scripts and right-to-left text do need one. Hinting trades fidelity for sharpness and costs an interpreter; grayscale coverage without hinting is deterministic (`desktop/survey.md#hinting-and-antialiasing`). Four font families cover the required letters under licences compatible with BSD code, measured in this session (`desktop/survey.md#candidate-fonts-and-their-licences`, `desktop/survey.md#the-ofl-next-to-bsd-code`): Inter and JetBrains Mono (OFL 1.1, no reserved name), DejaVu Sans and Sans Mono (Bitstream Vera licence) and Noto Sans (OFL 1.1). Text renderers in privileged code are an exploit class (DM-081), and UTF-16 and process-global locale are the catalog's warnings for text APIs (DM-096, DM-097). Measured: the font coverages and sizes in the survey, and `zoraya-font`'s exact-match screendump test. Assumed: that a scan-line rasteriser in fixed point is fast enough for a terminal of 80 by 24 cells at the stage 5 frame rates.

## Decision drivers
1. Total and deterministic: any bytes as a font or as text draw inside the bounds and give the same pixels on every host and in QEMU, so the screendump test keeps its zero tolerance (ADR-0095 drivers 1 and 3, ADR-0012's decoder discipline).
2. Latin, Cyrillic including Ukrainian, and Czech diacritics render correctly in stage 7; complex shaping and right-to-left stay out but are not precluded (the task's scope).
3. Few dependencies, every licence recorded, fonts redistributable next to BSD code (CLAUDE.md, `desktop/survey.md#the-ofl-next-to-bsd-code`).
4. Rendering runs in clients and in the compositor for titles, never in the kernel (DM-081).
5. UTF-8 everywhere with no process-global text state (CP-22, DM-096, DM-097).
6. One renderer for glyphs and for ADR-0161's icons.

## Considered options

### Option A: FreeType and HarfBuzz through `libposix` (Linux)
- **Description:** Port the C libraries with ADR-0109's toolchain and call them from native programs or from POSIX programs.
- **Evidence for:** Any font, any script, hinting, subpixel rendering, shaping of every script; decades of use.
- **Evidence against:** Two large C libraries of hostile-input parsing in every client (DM-081's class, now in user processes), a native desktop that depends on the POSIX toolchain (against ADR-0002's clean native world), and hinting plus FreeType's version-dependent output against driver 1.
- **Cost:** *(estimate)* The port, and a C ABI for every native client.
- **Precedent:** Every Linux desktop.

### Option B: A rasteriser crate (`fontdue` or `ab_glyph` over `ttf-parser`)
- **Description:** Depend on `fontdue` 0.9.5 or on `ab_glyph` 0.2.32 with `ab_glyph_rasterizer` and `owned_ttf_parser` 0.25, all `no_std` with `alloc` (`desktop/survey.md#rasterisers-in-rust-without-std`).
- **Evidence for:** Exists; CFF outlines, variable fonts and font collections come with `ttf-parser`; `ttf-parser` forbids unsafe code and promises not to panic.
- **Evidence against:** `ttf-parser` is "bug fixes only" and recommends moving to `fontations`; `rustybuzz` is archived; the crates use floating point, so determinism across hosts is not promised (driver 1); a dependency review of `ttf-parser`'s many tables for a system that reads six of them; `docs/dependencies.md` lists none today and CLAUDE.md prefers small things in-house.
- **Cost:** *(estimate)* A dependency record and review; three crates and about 20,000 lines of third-party code in every client.
- **Precedent:** `egui` ships `ab_glyph` (`desktop/survey.md#rasterisers-in-rust-without-std`).

### Option C: Zoraya's own `glyf` rasteriser, `zoraya-text`
- **Description:** A `no_std`, `#![forbid(unsafe_code)]` crate with no dependencies that reads a TrueType font with `glyf` outlines (`head`, `hhea`, `hmtx`, `maxp`, `loca`, `glyf` with composite glyphs, `cmap` formats 4 and 12) with a total parser, flattens quadratic Béziers to segments in 26.6 fixed point, accumulates signed coverage on scan lines, and writes 8-bit coverage into a glyph cache; a simple layout maps a `&str` to positioned glyphs with advances and kerning from `kern` format 0 if present. No hinting, no subpixel rendering, glyph origins on whole pixels.
- **Evidence for:** Drivers 1 to 6. The whole of `ab_glyph_rasterizer` is 555 lines and `fontdue` 1,972 (`desktop/survey.md#rasterisers-in-rust-without-std`), so the scope is known; fixed point makes the pixels a function of the bytes alone; ADR-0012's decoder discipline and ADR-0095's bounds test apply unchanged; the same scan-line code draws ADR-0161's icons; the four candidate fonts all have `glyf` outlines (`desktop/survey.md#candidate-fonts-and-their-licences`).
- **Evidence against:** No CFF outlines, no variable fonts, no shaping: a font must be chosen to fit; a few weeks of work and a fuzz target of its own.
- **Cost:** *(estimate)* 1,500 lines of parser, rasteriser, cache and layout, with host tests over every candidate font and a deterministic fuzz test.
- **Precedent:** `ab_glyph_rasterizer` and `fontdue` for the algorithm; `zoraya-font` for the testing discipline.

### Option D: Bitmap fonts in subfonts per Unicode range (Plan 9)
- **Description:** Extend ADR-0095's bitmap font with hand-drawn or converted bitmaps for Cyrillic and Latin Extended, loaded per range as Plan 9 did (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** Exact pixels and no rasteriser; the current test matches glyphs exactly.
- **Evidence against:** One size and no proportional text, which the frames and controls need; every size and weight is a new drawing; no scaling (ADR-0164) without a second set.
- **Cost:** *(estimate)* Hundreds of glyphs drawn by hand per size.
- **Precedent:** Plan 9, the stage 5 font.

## Recommendation
Recommended: **Option C**, with Option B's crates as the fallback if the rasteriser is not done in the first stage 7 task. It is a recommendation, not a decision.

- **Crate** `zoraya-text`: as Option C. The parser follows ADR-0012's discipline: whole table directory read once, every offset and length checked, composite depth bounded at 8, a glyph with more than 4,096 points or 64 contours refused `[assumption]`, and any error gives the box glyph, never a panic.
- **Fonts.** Exactly one pair of families ships, the one that goes with the direction the architect chooses (ADR-0161), and there is no font setting (the architect's decision of 2026-11-03, given in the task thread after the first report; recorded here because the Architect's decision section stays empty while the ADR is proposed): Inter 4.1 Regular and Medium for the UI with JetBrains Mono 2.304 Regular and Bold for the terminal and code (OFL 1.1) for the mist and night directions, or DejaVu Sans and DejaVu Sans Mono, Regular and Bold (Bitstream Vera licence) for the quiet direction. Unmodified files, with their licence texts, in `fonts/` of the repository and in the system image under `/system/fonts` (the subsetting that would shrink them is a modification both licences restrict, `desktop/survey.md#the-ofl-next-to-bsd-code`); `docs/dependencies.md` gains a fonts table. A font is loaded by a client from a file handle the launcher gives it (ADR-0053: no global name; the launcher passes `/system/fonts` as a read-only directory handle to every GUI program). The compositor embeds the UI font for titles.
- **Sizes and antialiasing.** ADR-0161's type scale in logical pixels, rendered at `size × scale` physical pixels (ADR-0164); 8-bit coverage composited in sRGB space with no gamma correction `[assumption: cheap and deterministic; FreeType's authors note the effect matters less at high density]`; no hinting; glyph origins snapped to whole physical pixels; a cache of rendered glyphs per (font, size) with a fixed capacity.
- **Scripts in stage 7.** Latin (Basic, Latin-1 Supplement, Latin Extended-A and B), Cyrillic (U+0400 to U+04FF, which includes the Ukrainian letters ґ є і ї), General Punctuation, the box-drawing characters the terminal uses, and the arrows and symbols of `zoraya-icons`. Correctness means: each code point with a glyph in the font renders that glyph with its advance; precomposed letters, which all required letters are, need nothing more. A combining mark the font maps (U+0300 to U+036F) is drawn at zero advance over the preceding glyph, without GPOS positioning: readable, not typographically correct, and documented as such.
- **Out of scope**, and how it stays possible: complex shaping, right-to-left and bidirectional text, vertical text, colour emoji. The layout entry point is a `Shaper` trait with the simple shaper as its one implementation and the glyph buffer (glyph id, x, y, cluster index) as its output, which is the output a HarfBuzz-style shaper gives; a shaper for those scripts is a second implementation and a font with GSUB and GPOS, with no change to the rasteriser or the callers.
- **Fallback** (`desktop/survey.md#font-fallback`). A text style names a font list: UI (Inter, then JetBrains Mono, then the box), monospace (JetBrains Mono, then Inter, then the box). The first font with a glyph for the code point draws it; in the terminal a fallback glyph is drawn centred in the cell at the cell's advance. The box glyph is drawn by the rasteriser itself, not from a font.
- **The terminal** (ADR-0096) keeps its cell grid: the cell is the monospace font's advance at the terminal size, rounded up; wide characters (East Asian width W) are out of scope and draw clipped in one cell; a combining mark takes a cell of its own in the terminal, drawn at the cell's start, so that ADR-0154's one column per scalar value and the screen agree (in proportional text it is drawn over its base at zero advance). The runner's screendump reader renders each printable glyph with the same crate at the same size and matches cells exactly, as today, so INV-0095-2's distinctness becomes a test over the chosen font's glyphs for the characters the tests type.
- **UTF-8.** Every text API takes `&str`; a name from ADR-0013 is valid UTF-8 by construction; bytes from a POSIX program's output that are not valid UTF-8 (X-8's open question) are drawn as one box per invalid sequence by the terminal's decoder, never re-interpreted. No locale state anywhere.

**Tensions with accepted ADRs.** *ADR-0095*: its Option C is superseded once this crate exists. `zoraya-font`'s three users move in order: `zoraya-term` (and the `term` program in `user/`) draw with `zoraya-text`, `tools/test-runner` reads screendumps with `zoraya-text`'s glyphs, the compositor's titles use `zoraya-text`; then `zoraya-font` is deleted. INV-0095-1 carries over as INV-0162-1 and INV-0095-2 becomes INV-0162-4. *ADR-0096* says "ADR-0095 draws boxes outside ASCII"; that sentence stops being true. *CLAUDE.md's dependency rule* speaks of crates; fonts are a new kind of dependency with their own table, as ADR-0109 did for C sources.

### Effect of the vision
None on the choice. For target 2, a font is hostile input to a sandboxed client only if the client loads its own; the system fonts are read-only handles. Target 3 needs shaping and more scripts, which is the second `Shaper` and more fonts, at a cost that Option B's `rustybuzz` shows to be about 17,000 lines if ported rather than written (`desktop/survey.md#rasterisers-in-rust-without-std`). Networking and threads: none.

### Deferred to stage 6
Nothing here depends on POSIX. `libposix`'s locale functions keep musl's C locale (ADR-0100); nothing in the native text path reads them.

## Consequences
- **Positive.** Real type at any size, Ukrainian and Czech text everywhere, no dependency, deterministic pixels, one renderer for text and icons.
- **Negative.** Only `glyf` fonts; no hinting, so small text is softer than on a hinted system; combining marks are approximate; the font files add about 1.4 MB to the repository and image.
- **Follow-up.** `zoraya-text`, `zoraya-icons`, the fonts table in `docs/dependencies.md`, the runner's new reader, deletion of `zoraya-font`.

## Invariants
- **INV-0162-1.** Rendering any string at any position and size writes only inside the target rectangle and the target slice, and never panics, whatever the font bytes. *Check:* host tests over every candidate font with positions at and past every edge against a guard border, and a deterministic fuzz test over mutated font files and random strings (the `zoraya-fs` decoder fuzz pattern).
- **INV-0162-2.** Rendering is a pure function of the font bytes, the string, the size and the position: the same inputs give identical coverage on any host and in QEMU. *Check:* a host test comparing renders against committed reference hashes per font and size, run on the Mac now and on Linux CI once CI runs (`docs/design.md`, Known limitations), and the same hashes checked in the test kernel under HVF and TCG.
- **INV-0162-3.** Every code point in the stage 7 scripts (the Latin and Cyrillic blocks named above) that the UI font and the monospace font map renders a glyph, and every required letter (ґ є і ї, ě š č ř ž ý á í é ú ů ď ť ň ó, ы ъ э ё, both cases) is mapped by both fonts. *Check:* a host test over the font files' `cmap`.
- **INV-0162-4.** In the terminal's monospace font at the terminal size, every printable glyph the GUI tests type is distinct from every other and from the box, so a screendump reads back as text exactly. *Check:* a host test comparing all pairs of rendered cells; the GUI test's reader.
- **INV-0162-5.** Every font file in the repository is unmodified from its release and ships with its licence text; the table in `docs/dependencies.md` lists each with its SHA-256. *Check:* a host test hashing `fonts/` against the table.

## Open questions
- Whether sRGB-space blending without gamma correction reads well on the Mac's display at scale 2: decided by looking at the mockups, which use the same rule.
- Kerning: `kern` format 0 only, or GPOS pair adjustment as well (Inter ships kerning in GPOS): the first task measures what the files contain.
- Whether the glyph cache's fixed capacity suits a terminal that shows many distinct Cyrillic glyphs: measured.

## Revisit triggers
- A program that needs Arabic, Hebrew, Devanagari or East Asian text: the second shaper and fonts.
- A font the architect wants that has CFF outlines or is variable only.
- A measured frame rate where rasterisation dominates.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
