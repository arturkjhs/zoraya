# ADR-0326: NetSurf's text through `zoraya-text`: the system's fonts from `/system/fonts`, at the window's scale

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0162, ADR-0164, ADR-0311, ADR-0317, ADR-0318
- **Evidence:** CP-27, desktop/survey.md#candidate-fonts-and-their-licences, desktop/survey.md#the-ofl-next-to-bsd-code

## Context
ADR-0318 gave NetSurf 3.11 its internal bitmap font (`font_internal.c`, Latin-1, 8 by 16 cells at one or two times),
and named a page that shows boxes where text is expected as its revisit trigger. A page in Russian or Ukrainian shows
NetSurf's placeholders (four hexadecimal digits a character) today, while the rest of the desktop draws the same letters
with `zoraya-text` (ADR-0162). At scale 2 (ADR-0164) the window's buffer has twice the pixels a side, and NetSurf lays
the page out in those pixels at its default 90 dpi, so its text and boxes are half the size of the desktop's.

The architect decided on 2026-11-14 (TASK-27, part 9f-text) that NetSurf's fonts go through `zoraya-text`, ADR-0318's
Option C, with three conditions: the system's fonts are read only from `/system/fonts` (no other path), Unicode with
Cyrillic coverage, sized for the window's scale. This ADR records that decision and how it is built. It supersedes
ADR-0318 in its fonts item only; the source set, the build and everything else of ADR-0318 stand.

Read in this session (NetSurf 3.11's `frontends/framebuffer`): the front end calls the font code through
`fb_font_init`, `fb_font_finalise`, the layout table's `width`, `position` and `split`, and the text plotter in
`framebuffer.c`; `fbtk/text.c` sizes its text fields from the font height. `font_freetype.c` is the model for a font
library: it selects a face by family and weight, takes the size in points from `plot_font_style` (22.10 fixed
point) and NetSurf's dpi, and plots each glyph's 8-bit coverage with `libnsfb`'s `nsfb_plot_glyph8`, which clips to
the plotter's clip rectangle. The system's fonts are Inter 4.1 Regular and Medium and JetBrains Mono 2.304 Regular and
Bold (`fonts/`, `docs/dependencies.md`); Inter maps 248 of the 256 code points of U+0400 to U+04FF and every required
Ukrainian, Russian and Czech letter (`desktop/survey.md#candidate-fonts-and-their-licences`). They are under the SIL
OFL 1.1 (`desktop/survey.md#the-ofl-next-to-bsd-code`). The system image has no `/system/fonts` yet: the programs
`init` starts embed the fonts (ADR-0240).

## Decision drivers
1. The architect's three conditions above.
2. One renderer for the whole desktop: the same bytes give the same pixels in NetSurf as in the terminal and the
   panel (INV-0162-2), with ADR-0162's total parser and its bounds (INV-0162-1).
3. A POSIX program reads only what its line grants (INV-0318-2); a new grant is visible to the person, as the roots
   are with the network (ADR-0311) and the window's grants are as pills (ADR-0317).
4. Scope held by decision (CP-27): no shaping, no italic face, no font setting.

## Considered options

### Option A: FreeType and fontconfig, as on Linux
- **Description:** port FreeType and fontconfig, build NetSurf with `NETSURF_FB_FONTLIB=freetype`.
- **Evidence for:** NetSurf's tested path; hinting; any font format.
- **Evidence against:** two large C ports of hostile-input parsing in a program that reads web pages, a second
  renderer whose pixels differ from the desktop's (driver 2), fontconfig's search of configuration paths against
  driver 3.
- **Cost:** *(estimate)* weeks.
- **Precedent:** every Linux build of NetSurf's framebuffer front end.

### Option B: keep the internal bitmap font (ADR-0318 as accepted)
- **Description:** no change.
- **Evidence for:** no dependency, no I/O, deterministic screens.
- **Evidence against:** Cyrillic as placeholders, which the architect rejected; one cell width for every character.
- **Cost:** none.
- **Precedent:** NetSurf's fbcon builds.

### Option C: `zoraya-text` behind a small C interface in `libc.a`
- **Description:** a `no_std` crate over `zoraya-text` does what the front end needs (faces by family and weight,
  width, position, split, glyphs, line metrics); `libposix` exports it as a few C functions, as it does the window
  (ADR-0317 item 7: one Rust static library, one allocator); a NetSurf font file of our own calls them.
- **Evidence for:** drivers 1 to 3; the same `Text` cache and shaper as the desktop; a plotter that draws
  `zoraya-text`'s coverage through `nsfb_plot_glyph8` keeps `libnsfb`'s clipping.
- **Evidence against:** names added to `libc.a`; a measured cost per page that is not known before it is built
  (ADR-0318's revisit trigger names 5%); the fonts must be in the program's namespace.
- **Cost:** *(estimate)* days with tests.
- **Precedent:** Redox's own toolkit draws with its own Rust renderer rather than FreeType `[outside evidence: the
  orbtk and orbfont repositories]`.

## Recommendation
**Option C**, as the architect decided. In detail, as built in part 9f-text:

1. **The library.** `crates/zoraya-fontset` (`no_std`, `#![forbid(unsafe_code)]`, over `zoraya-text` only): four
   faces, each a `zoraya-text` font list in fallback order: sans (Inter Regular, then JetBrains Mono Regular), sans
   bold (Inter Medium, then Inter Regular, then JetBrains Mono Bold), monospace (JetBrains Mono Regular, then Inter
   Regular) and monospace bold (JetBrains Mono Bold, then JetBrains Mono Regular, then Inter Medium). NetSurf's
   serif, cursive and fantasy families take sans; weight 600 and above takes the bold face; italic and oblique are
   drawn upright (the system has no italic face). A character no font of the list maps is `zoraya-text`'s box, and
   the zero-width characters U+200B to U+200F draw nothing, as in the internal font. Width, position and split keep
   NetSurf's documented meaning (`font.h`, `font_freetype.c`): position gives the character boundary nearest to `x`;
   split breaks at the last space whose start fits, else the first space after, else the end; offset 0 is never a
   split. Bytes that are not UTF-8 count one U+FFFD per invalid sequence and keep their offsets.
2. **The names `libc.a` adds** (`crates/zoraya-posix/src/ffi/text.rs`, the window's `ffi/window.rs`), declared in
   `ports/netsurf/zoraya_text.h` and `zoraya_window.h`; they are not a boundary (the font bytes are the system's, and
   any C code can read the same files through its grant):

   ```posix-surface-added
   zoraya_text_glyphs
   zoraya_text_init
   zoraya_text_metrics
   zoraya_text_position
   zoraya_text_split
   zoraya_text_width
   zoraya_window_scale
   ```
3. **Only `/system/fonts`.** The library names its four files and the directory; the loader asks for
   `/system/fonts/NAME` for those names and nothing else: no environment variable, no option, no configuration file,
   no search. A file that is missing or does not parse leaves its place in the lists empty (the next font draws); with
   none, every character is the box and the text is still measured.
4. **The grant.** A POSIX program of the image whose description has `window` is given `/system/fonts` read only,
   "with the window", as one with the network is given `/system/tls` (ADR-0311); `grants` shows it and the window shows
   it as a pill (INV-0317-1). Nothing else gives a POSIX program that directory.
5. **The image.** `tools/run-disk.sh` (the disk of `make run`, `run-net`, `run-gui` and the hand run) and the root of
   `make test-netsurf` copy the files of `fonts/`, unmodified with their licence texts, into the volume's
   `/system/fonts`; `rootfs/` holds no second copy, and the disk's key includes them, so a changed font makes a new
   disk. The programs `init` starts still embed their fonts (ADR-0240).
6. **The scale.** `zoraya_window_scale` gives the window's scale; at font start NetSurf's dpi becomes 90 times the
   scale (NetSurf's default 90 at scale 1), so the page's CSS lengths and its text are both at the window's scale, and
   a glyph's size is `points × dpi / 72` pixels, rounded, within `zoraya-text`'s 1 to 512.
7. **Licence.** The fonts are data under the OFL, unmodified, read at run time and not part of the `netsurf` binary;
   whether the licence rule of `CLAUDE.md` names OFL-1.1 for such data files is ADR-0327's question (proposed).

## Consequences
- **Positive.** Russian, Ukrainian and Czech pages are readable in NetSurf; its text matches the desktop's; the page
  is at the window's scale.
- **Negative.** `libc.a` gains seven names; NetSurf's window shows one more pill; pages with no Cyrillic look
  different from 9d's screens (proportional type in place of 8-pixel cells); no italic; no kerning beyond `kern` 0
  (ADR-0162's limit); a measured cost per page is still to be taken.
- **Follow-up.** An italic face, if the architect wants one; the native GUI programs move from embedded fonts to
  `/system/fonts` (ADR-0240's follow-up) now that the image has the directory.

## Invariants
- **INV-0326-1.** The font library reads no path but `/system/fonts/NAME` for its own file names, and nothing gives a POSIX program `/system/fonts` but a `window` line, read only. *Check:* `cargo test -p zoraya-fontset` (a recording source) and `cargo test -p zoraya-shell` (the grant with the window and without it).
- **INV-0326-2.** Every Russian and Ukrainian letter (U+0410 to U+044F, Ё ё, Ґ ґ, Є є, І і, Ї ї) maps to a glyph of the sans faces that is not the box, and draws a non-blank glyph at the sizes of scale 1 and scale 2. *Check:* `cargo test -p zoraya-fontset`, and `make test-netsurf` (a Cyrillic page at scale 1 and at scale 2: ink where its words are, and a line of і narrower than a line of as many ж).

## Open questions
- The cost of measuring text per page load against ADR-0318's 5% trigger: not measured in 9f-text.

## Revisit triggers
- A page the architect names whose letters show the box; a measured text cost above 5% of a page load; an italic
  face in the system's fonts.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

NetSurf's fonts go through `zoraya-text` (ADR-0318's Option C): the system's fonts are read only from `/system/fonts`
(no other path), Unicode with Cyrillic coverage, sized for the window's scale. Supersedes ADR-0318 in the fonts item
only.
