# ADR-0095: Text rendering

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0093, ADR-0096
- **Evidence:** DM-081, CP-22, GR-015, alternatives-postmortem.md#plan-9--inferno

## Context
The terminal draws text into its surface. Text needs a font and a renderer. `docs/dependencies.md` allows only permissively licensed `no_std` crates, with a recorded reason, and prefers writing small things in-house; an embedded font file needs a licence that allows it. The GUI test reads the screen back, so whatever renders text must be exact and repeatable. Measured: nothing on Zoraya.

## Decision drivers
1. Rendering is total: no input, including any UTF-8, can make it draw outside a cell or panic (CP-22).
2. Few dependencies, and every licence recorded.
3. Deterministic pixels, so that a test can compare a screendump exactly.
4. Text renderers in privileged code have been a source of exploits, so rendering stays in the client (DM-081).

## Considered options

### Option A: FreeType and fontconfig in each client (Linux)
- **Description:** Outline fonts from the filesystem, rasterised with hinting and anti-aliasing in the client.
- **Evidence for:** Any font, any size, good quality.
- **Evidence against:** A C library of hostile-input parsing, and a font search path; anti-aliasing makes pixel comparison depend on the rasteriser's version.
- **Cost:** *(estimate)* A C library port, which needs stage 6's toolchain.
- **Precedent:** Linux desktops.

### Option B: A TTF rasteriser crate (`fontdue`) with an embedded font
- **Description:** A pure-Rust `no_std` rasteriser and an embedded outline font, for example DejaVu Sans Mono.
- **Evidence for:** Real fonts and sizes, in Rust, now.
- **Evidence against:** An external dependency with a font parser of its own to review, an embedded font's licence (DejaVu's is the Bitstream Vera licence `[outside evidence]`), anti-aliased pixels to compare (driver 3).
- **Cost:** *(estimate)* A dependency review and a few hundred KB of font.
- **Precedent:** Embedded Rust GUIs `[outside evidence]`.

### Option C: Zoraya's own bitmap font
- **Description:** A 5 × 9 dot design for printable ASCII, drawn at twice its size in a 12 × 20 pixel cell, written for Zoraya under the repository's licence. Every other character draws a box.
- **Evidence for:** Drivers 1 to 4. Every pixel is either the text colour or the background, so a screendump can be read back by exact glyph match. No dependency and no licence question.
- **Evidence against:** ASCII only, one size, and coarse to look at.
- **Cost:** *(estimate)* 95 glyphs and 80 lines of renderer.
- **Precedent:** Text consoles of most systems.

### Option D: Bitmap fonts with subfonts per Unicode range (Plan 9)
- **Description:** A font is a list of subfont files, each covering a range of runes, loaded on demand; Plan 9 used UTF-8 throughout (GR-015, `alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** Unicode coverage with simple bitmaps.
- **Evidence against:** Font files from the filesystem and their parser; more than stage 5 needs.
- **Cost:** *(estimate)* A loader and font files with their licences.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option C** now, with Option D's subfont idea as the path to Unicode. It is a recommendation, not a decision.

- **Crate** `zoraya-font`, `no_std`, `#![forbid(unsafe_code)]`, no dependencies. Glyphs as rows of bits in the source.
- **Cell.** 12 × 20 pixels; the glyph's dots are 2 × 2 pixels from (1, 1). Rows 0 to 6 hold capitals and the body, rows 7 and 8 descenders.
- **Coverage.** U+0020 to U+007E. Any other `char`, including controls the terminal passes on, draws a hollow box. Every printable glyph differs from every other and from the box, so a screen can be read back as text.
- **Rendering** writes into a caller's pixel slice with a stride, clipped to the slice: a cell partly outside is drawn partly, never past the end.
- **Licence.** The repository's BSD-2-Clause; `docs/dependencies.md` records that the font is in-house.

**Tensions with accepted ADRs.** None.

### Effect of the vision
None for any target at this stage. Target 3 needs Unicode and proportional text, which is Option B or D added beside this crate; the terminal's cell grid does not change for a monospace font, so the later cost is the new crate and its review, not a terminal rewrite.

### Deferred to stage 6
Nothing here depends on POSIX. Programs that print non-ASCII UTF-8 see boxes until a larger font exists.

## Consequences
- **Positive.** Exact, testable pixels; no dependency.
- **Negative.** ASCII only, and one size.
- **Follow-up.** Subfonts for Unicode ranges.

## Invariants
- **INV-0095-1.** Rendering any `char` at any position writes only inside its cell and inside the target slice. *Check:* host test over every `char` class and positions at and past every edge, against a guard border.
- **INV-0095-2.** Every printable ASCII glyph is distinct from every other and from the box. *Check:* host test comparing all pairs.

## Open questions
- Legibility for a person: not measured.

## Revisit triggers
- A program that needs non-ASCII text on screen.

## Architect's decision
