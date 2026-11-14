# ADR-0416: Fonts and text for the engine: FreeType and HarfBuzz as ports, fontconfig with one fixed configuration over `/system/fonts`

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0162, ADR-0163, ADR-0320, ADR-0326, ADR-0327, ADR-0410, ADR-0411, ADR-0421
- **Evidence:** CP-21, CP-22, DM-097, X-15, browser-engine/survey.md#ladybird, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#measurements, browser-engine/survey.md#the-window-text-and-the-network, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE requires FreeType 2.9 and HarfBuzz 2.7.4 with ICU; it renders with Skia (browser-engine/survey.md#wpe-webkit-in-detail).
Its font lookup goes through fontconfig on Linux, which the Bubblewrap launcher gives the Web process as its
configuration and font directories. Ladybird shapes with HarfBuzz unconditionally and matches fonts with fontconfig on
Linux, with a `SystemFontMatcherNone` beside it (browser-engine/survey.md#ladybird). Skia can work without fontconfig
through its directory, embedded and empty font managers (browser-engine/survey.md#libraries-and-interfaces).

Zoraya's text is `zoraya-text`, its own rasteriser without hinting or shaping, with four OFL font files, Latin and
Cyrillic (ADR-0162, ADR-0327; browser-engine/survey.md#the-window-text-and-the-network). ADR-0326 put NetSurf's text
through it and rejected FreeType and fontconfig for NetSurf: two large C ports parsing hostile input, a second renderer,
search paths. Fontconfig reads `/etc/fonts`, `$XDG_CONFIG_HOME` and environment variables, and writes a cache under
`$XDG_CACHE_HOME` (browser-engine/survey.md#libraries-and-interfaces). **Measured**: FreeType 2.15.0 is 257,493 lines,
HarfBuzz 15.1.0 238,050, fontconfig 2.18.3 31,886 (browser-engine/survey.md#measurements).

## Decision drivers

1. **The engine shapes text** with HarfBuzz and draws glyphs with Skia over FreeType faces; it has no hook for a
   renderer like `zoraya-text` below its font cache.
2. **No implicit configuration** (CP-21, X-15): nothing a font library reads from the environment or a home
   directory may change what a program sees (DM-097's hidden global state).
3. **Fonts are data the image fixes** (ADR-0327): unmodified OFL files in `/system/fonts`.
4. **Hostile input parsed in the content process only**: web fonts come from pages.
5. **One encoding** (CP-22): UTF-8 names and paths.

## Considered options

### Option A: FreeType, HarfBuzz and fontconfig as Linux distributions configure them

- **Description:** The libraries with `/etc/fonts/fonts.conf`, per-user configuration and a writable cache.
- **Evidence for:** No change to the engine or the libraries.
- **Evidence against:** Driver 2: `FONTCONFIG_FILE`, `FONTCONFIG_PATH`, `$XDG_*` and the user files change what fonts a
  program gets; a writable cache is state outside the program's own directory.
- **Cost:** *(estimate)* Three ports.
- **Precedent:** Every Linux distribution; WebKit's Bubblewrap launcher binds the user's fontconfig into the sandbox.

### Option B: FreeType and HarfBuzz as ports; fontconfig built with one compiled-in configuration, no environment, no cache file

- **Description:** fontconfig is built with its configuration compiled in (a single `<dir>/system/fonts</dir>`, fixed
  family aliases), its environment variables and home lookups removed by a patch, and its cache kept in memory.
- **Evidence for:** Drivers 1 to 5; the engine's lookup runs unchanged; fontconfig is MIT-style and 32 thousand lines.
- **Evidence against:** A second renderer beside `zoraya-text` on the desktop (ADR-0326 driver 2); fontconfig's XML
  parser (expat or libxml2) runs on trusted files only.
- **Cost:** *(estimate)* Three ports, a fontconfig patch of about 100 lines.
- **Precedent:** Fuchsia, whose Skia font manager (`SkFontMgr_fuchsia` in Skia's ports directory) asks a font
  provider for a fixed set; Android's fixed `fonts.xml` list (its parser is in the same directory; the format was not
  read).

### Option C: No fontconfig: WebKit's font cache over Skia's directory font manager

- **Description:** Patch WebKit's font lookup to use `SkFontMgr_New_Custom_Directory("/system/fonts")` with a fixed
  family table, as Ladybird's `SystemFontMatcherNone` does for itself.
- **Evidence for:** One port fewer; no XML parser.
- **Evidence against:** A patch inside WebKit's font cache to carry at each upgrade; family matching and fallback by
  character must be rewritten.
- **Cost:** *(estimate)* 600 lines of WebKit patch.
- **Precedent:** Skia's custom font managers; Ladybird's `SystemFontMatcherNone`.

### Option D: `zoraya-text` behind the engine's font interface (ADR-0326's route)

- **Description:** Give WebKit glyphs and advances from `zoraya-text`.
- **Evidence for:** One renderer on the desktop (ADR-0326 driver 2).
- **Evidence against:** Driver 1: the engine shapes with HarfBuzz and rasterises with Skia; `zoraya-text` has no
  shaping, no italic, no GPOS kerning.
- **Cost:** Not feasible without replacing Skia's text path.
- **Precedent:** NetSurf on Zoraya (ADR-0326), whose layout engine asks only for widths.

## Recommendation

**Recommendation (not a decision): Option B.** It does not change ADR-0326, which stays NetSurf's; it accepts a second
renderer inside the browser's processes, against ADR-0326 driver 2 ("one renderer for the whole desktop"), which is a
tension for the architect to decide.

1. **FreeType** (FTL, its BSD-style licence) and **HarfBuzz** (Old MIT) as static ports built by the CMake toolchain
   file (ADR-0367), FreeType with zlib, libpng and brotli (WOFF2) and without bzip2; HarfBuzz with FreeType and ICU.
2. **fontconfig** with `-Dxml-backend` on libxml2 (already a WPE dependency) or expat if libxml2 is not accepted; a
   patch that compiles in one configuration (`/system/fonts` only; `sans-serif` → Inter, `monospace` → JetBrains Mono,
   `serif` → Inter), removes every environment variable and home path, and keeps the cache in memory only.
3. **Fonts.** The four files of ADR-0327 and nothing else at first. Web fonts are page data, decoded in the Web
   process (WOFF2 through `woff2` and brotli), never written to `/system/fonts` and not files of the image; ADR-0327
   governs the image's files only. More scripts (CJK, Arabic, Devanagari, emoji) are a later choice of files under
   ADR-0327, not of code.
4. **Where they run.** The Web process opens `/system/fonts` read-only (ADR-0411 item 2); the UI process draws its own
   chrome with `zoraya-text` (ADR-0419), so the desktop's look stays one renderer outside the page.

### Effect of the vision

For target 2 a hostile font from a page is parsed by FreeType inside the content process, which holds nothing
(ADR-0411); no environment variable of an agent can redirect font lookup. Target 3: pages render with shaping, kerning
and italics. Networking: web fonts arrive over the network process. Threads: Skia's worker threads share the faces.

### Examples

A Ukrainian article in Inter renders with kerning and italic emphasis (synthesised from Inter Regular until an italic
file is added); a Japanese page shows boxes for its CJK text until a CJK font file is added, as `zoraya-text` does today.

## Consequences

- **Positive.** The engine's text path as upstream has it; shaping for every script whose font exists.
- **Negative.** Half a million lines of font code parsing hostile data in the Web process; two renderers on the desktop.
- **Follow-up.** Three ports and the fontconfig patch; the first engine part's text tests (scale 1 and 2, Cyrillic,
  web fonts).

## Invariants

- **INV-0416-1.** fontconfig in the image reads no environment variable and no file outside `/system/fonts` and its
  compiled-in configuration, and writes no file. *Check:* a test step that runs `fc-match` (built for the test only)
  with `FONTCONFIG_FILE`, `HOME` and `XDG_*` set to directories it may not read, under a table that refuses and
  reports any other path; the run shows no refusal and the same answer.
- **INV-0416-2.** The Web process's table has `/system/fonts` read-only and no other font directory. *Check:*
  INV-0411-2's test.

## Open questions

- Whether WPE 2.54's Skia text path uses fontconfig through `FontCache` or Skia's fontconfig font manager or both; read
  in the source by the fonts part.
- Whether fontconfig's in-memory cache costs noticeable start-up time per process with four fonts (not likely).

## Revisit triggers

- A second program needing shaping (an editor, a PDF viewer): whether `zoraya-text` gains HarfBuzz instead (ADR-0162's
  extension point).
- WebKit gains a font interface below its cache that a platform can implement.

## Architect's decision
