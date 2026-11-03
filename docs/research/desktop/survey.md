# Desktop survey for TASK-19

The research behind proposed ADR-0160 to ADR-0169 (stage 7, the desktop). Every source in [Sources](#sources) was
fetched on 2026-11-03 from the architect's Mac, and the note there says what was read. Text is paraphrased; short
quotations are marked. Where a page could not be fetched, the claim rests on a catalog entry (`DM-`, `GR-`, `CP-` in
`../design-mistakes.md`, `../held-up-well.md` and `../candidate-principles.md`) or is left out. Apple's current
guidelines render only with JavaScript; their text was read from the JSON data behind each page (the URLs in
[Sources](#sources) are the data endpoints). Three hosts refused this session: `www.hpl.hp.com` (the Polaris
technical report, replaced by the authors' SOUPS 2005 poster), `www.combex.com` (CapDesk's own pages, replaced by
Miller, Tulloh and Shapiro's paper, which describes CapDesk) and `cacm.acm.org` (403). Nothing below rests on a page
that was not fetched.

Numbers measured in this session (font coverage and sizes) are marked *measured*; everything else is what the source
says.

## Desktops and their design systems

### macOS and Apple's Human Interface Guidelines

Apple's guidelines are organised as Foundations (typography, colour, layout, motion, ...), Patterns and Components.
What they say that matters here:

- **Typography** [D1]. macOS's default text size is 13 pt and its minimum 10 pt. "Minimize the number of typefaces
  you use", "avoid light font weights" (prefer Regular to Bold at small sizes). Two families: San Francisco (SF Pro,
  SF Compact, SF Mono and script variants) and New York (serif), shipped as variable fonts with optical sizes. A
  *text style* "specifies a combination of font weight, point size, and leading values"; the macOS built-in styles
  are Large Title 26/32 pt, Title 1 22/26, Title 2 17/22, Title 3 15/20, Headline 13/16 bold, Body 13/16, Callout
  12/15, Subheadline 11/14, Footnote 10/13, Caption 1 10/13, Caption 2 10/13 medium (size/line height; weight
  Regular unless stated). "Consider using the built-in text styles."
- **Colour** [D2]. System colours "vary subtly depending on the system appearance" and have light, dark and
  increased-contrast variants; "when possible, use system colors"; a custom colour needs light and dark variants;
  "avoid hard-coding system color values"; dynamic colours are "semantically defined by its purpose, rather than its
  appearance". Never rely on colour alone.
- **Windows** [D3]. A macOS window "consists of a frame and a body area"; the frame holds the window controls and
  the toolbar and is what people drag. Three states: *main* (the app's frontmost window), *key* ("accepts people's
  input", one at a time) and *inactive*, each with a different appearance: "the key window uses color in the title
  bar options for closing, minimizing, and zooming; inactive windows and main windows that aren't key use gray".
  "Avoid creating custom window UI" and "don't try to replicate the system-provided appearance" (that the system
  draws the frame is this survey's inference from that advice; the page does not say who draws it).
- **Motion** [D4]. "Add motion purposefully", "make motion optional" (never the only way to convey information),
  "aim for brevity and precision", "avoid adding motion to UI interactions that occur frequently", "let people
  cancel motion". No durations are given.
- **Layout** [D5]. Order by importance in reading order; align and indent to show hierarchy; group with space,
  containers or separators; progressive disclosure; adapt to text-size changes.

What makes it coherent: one type family in a fixed scale of styles, semantic colours that adapt to appearance, and
frames the system draws. The guidelines are prescriptive about what the system owns (windows, colours, text styles)
and permissive inside the body area.

### GNOME's HIG and libadwaita

- **Principles** [D6]: design for people, make it simple ("the best apps do one thing and do it well"), reduce user
  effort, be considerate.
- **Typography** [D7]. The default font is Adwaita Sans, "a custom built variant of Inter". Styles: body, heading,
  caption, caption-heading, large title, title 1 to 4. "Minimize the number of font sizes and weights"; avoid
  italics; "do not use all caps"; use the standard styles, never hard-coded sizes. Typographic conventions as Unicode
  characters: curly quotes, the ellipsis U+2026, the en dash for ranges, U+00D7 for dimensions ("1024×768").
- **Styling** [D8]. Light is the default; dark "is primarily recommended for apps which display rich visual content";
  an app that offers a choice offers "light, dark, and follow system preference"; test with high contrast; reuse
  the named colours and style classes; never colour as the only cue.
- **Named colours** (libadwaita's stylesheet, [D9]): light `window_bg #fafafb`, `view_bg #ffffff`,
  `headerbar_bg #ffffff`, `card_bg #ffffff`, `dialog_bg #fafafb`, foreground `rgb(0 0 6 / 80%)`; dark
  `window_bg #222226`, `view_bg #1d1d20`, `headerbar_bg #2e2e32`, `dialog_bg` and `popover_bg #36363a`, `card_bg`
  white at 8 %, foreground white; borders at 15 % opacity. Destructive is Red 3 (Red 4 in dark), success Green 4
  (Green 5 in dark), warning Yellow 5 in light and a hard-coded `#cd9309` in dark; the default accent is not
  defined in this file (`accent_bg_color` is referred to and set elsewhere). The palette's values are [D10] and
  [D11]. The palette [D11] is five steps of each hue, for example Blue 1 to 5
  `#99c1f1 #62a0ea #3584e4 #1c71d8 #1a5fb4`, Light 1 to 5 `#ffffff #f6f5f4 #deddda #c0bfbc #9a9996`, Dark 1 to 5
  `#77767b #5e5c64 #3d3846 #241f31 #000000`.
- **Header bars** [D12] run along the top of a window, "allow windows to be dragged", hold a few actions at the
  start, the title in the centre and menus at the end; "should only contain a small number of controls"; always leave
  blank space to drag. Toolbars keep "6px margins and spacing between widgets"; a frame is a 1 px border [D13].
- **Icons** [D14]. UI icons are *symbolic*: monochrome 16 × 16 px SVGs, "2px strokes for main features", "align all
  shapes to the pixel grid", recoloured programmatically, used at 16, 32, 64 or 128 px ("avoid other sizes to
  prevent fuzzy rendering"); a control has a label or an icon, not both; use an icon only if people will recognise
  it.
- **Client-side decorations** [D15]. GNOME's CSD initiative argues that title bars are "largely empty bars" that
  cannot hold other UI, that header bars "allow for better integration between application and window chrome", and
  aims at "functional, not visual consistency": no title bar, native-looking window buttons, respect for the
  settings that hide or reposition them.

What makes it coherent: one named-colour vocabulary and one set of style classes that every app is told to reuse,
and conventions written down to the character. What does not: the decoration is drawn by each app, so consistency is
by convention and by the toolkit, and a non-GTK app on GNOME must draw its own frame [D15].

### KDE's HIG

The landing page [D16] states the aim: an app should feel "at home when run in KDE Plasma, and works well outside of
it"; the two pillars are "simple by default" and "powerful when needed"; "under-promise and over-deliver, never the
reverse"; the document is "more of a what and a why, not a how". Its colour, typography and spacing rules live in
sub-pages that were not read. KDE's compositor, KWin, draws server-side decorations and its author drafted the
decoration negotiation protocol [D17] (below).

### Haiku

Haiku inherits BeOS's "one team writes everything" coherence (`../alternatives-postmortem.md`, Haiku / BeOS). Its
draft HIG [D18] gives concrete rules: "a titled window should never have an empty title bar"; controls "never
overlap"; program windows open centred and later documents offset; "do not arbitrarily restrict resizing"; "control
labels should always be in the system plain font and size"; limit the number of font sizes; icons flat or isometric.
The window *look* and *feel* are properties of the window object, drawn by the system: document, titled, floating,
modal, bordered and borderless looks; normal, modal and floating feels [D19]. The user guide [D20] describes the
result: the yellow tab shows the name and is dragged; double-clicking it minimises; right-clicking the tab or border
sends the window back; the zoom button expands; left-dragging the border moves and right-dragging resizes; holding
OPT while dragging stacks windows (tabs join) or tiles them (borders fuse). Preferences are separate small programs
(Appearance, Keymap, Screen, Shortcuts, ...) [D21]; a user keymap lives in `~/config/settings/Keymap/` and can be
dumped and edited as text with `keymap -d` and loaded with `keymap -s` [D22].

### SerenityOS

"A love letter to '90s user interfaces with a custom Unix-like core"; "a system by us, for us"; everything "built
from-scratch by us" under a 2-clause BSD licence; "unified style and design philosophy, flexible theming system,
custom (bitmap and vector) fonts" [D23]. Its window server's sources include `WindowFrame`, `Button`, `Menu`,
`MenuManager`, `Menubar`, `WindowSwitcher`, `KeymapSwitcher` and `Compositor` [D24]: frames, menus, the switcher and
the keymap switch are the server's. Its text guideline fixes two capitalisation styles and when a label ends in an
ellipsis ("Save As...", only when more input is needed) [D25].

### Plan 9's rio

`rio` [D26] manages windows with mouse button 3 and a menu of New, Resize, Move, Delete and Hide; "one window is
current, and is indicated with a dark border and text"; clicking in a window makes it current and raises it;
dragging a border with button 1 or 2 moves that edge; each window is a terminal whose `/dev/cons` receives the
keyboard only while current; deleting a window sends a hangup note to the window's process group. There are no
title bars, only borders whose colour carries the state.

### What makes a desktop coherent

Across the five: (1) one type scale with named styles that code refers to by name (Apple, GNOME, Haiku's "system
plain font"); (2) one named-colour vocabulary with light and dark values, and semantic names (Apple, libadwaita);
(3) the frame and its controls drawn by one party (macOS, Haiku, SerenityOS, rio draw them in the system; GNOME in
the app, with a written contract); (4) rules written down to the character (GNOME's Unicode conventions,
SerenityOS's capitalisation); (5) a small set of components everyone reuses. Where a desktop feels incoherent the
cause is a toolkit boundary: GNOME apps outside GTK, or GTK apps on KWin, each draw a frame that the other does not
recognise [D15][D17].

## Window management

### Server-side and client-side decorations

The Wayland core leaves decoration to the client; the `xdg-decoration` protocol [D27] lets a compositor announce
server-side decorations and a client ask for a mode, with two modes, `client_side` and `server_side`, and "the
compositor can decide not to use the client's mode and enforce a different mode instead". Its author wrote that
"GNOME Shell and Weston require CSD, but KDE Plasma and Sway do not", and that client-side decoration is "a lot of
work" for toolkits [D17]. GNOME's case for CSD is integration: the frame can hold controls [D15]. The case for SSD is
consistency (one drawing of the frame), a frame that stays usable when the client hangs, and the security argument
below. Haiku, SerenityOS, rio and macOS draw the frame in the server ([D19][D24][D26][D3]).

### Spoofing and the trusted path

Shapiro et al.'s EROS trusted window system [D28] is the capability-system precedent (DM-079 cites it). Its
"entire implementation ... is less than 4,500 lines, which is a factor of ten smaller than previous trusted window
systems such as Trusted X". Design principles: "no operation performed on one client session should be able to
affect or observe state associated with other sessions", "the display server should provide no shared mutable state
to clients", minimise resource types. Clients render into shared memory and the server's bit-block transfer "is the
only routine in the display server that reads the client memory region", wrapped in a fault handler that ends the
session on a bad reference. On decorations: the title bar's functions "are directives rather than requests, and in
consequence ... the display server must handle these functions"; "title bar and border rendering are performed by the
display server"; "if applications are permitted to set the title bar font, they are in a position to alter the
information displayed", so the server uses "a fixed, compiled-in font" for titles. On labels: "there is no label
that the display server can apply on a window border that cannot be visually forged by a client", and dimming
unfocused windows is not enough because an app can dim its own window the same way; EWS "prominently featur[es] the
border of the focus window using a bright color while dimming non-focus windows" and reserves "a band at the bottom
of the display" for labels. On paste: "in EWS, no events are transmitted to the destination until the user performs
a 'paste' action"; a key table says which keystrokes authorise cut and paste; mouse-driven paste goes through
server-defined regions with a distinguished cursor that must have been visible for a minimum time. On dialogs:
"there is no need for a trusted path to support a trusted SaveAs agent", because "only the SaveAs agent holds a
capability to the user's file system. An application might forge the appearance of a SaveAs dialog, but cannot
forge possession of the necessary file system capability"; the remaining trusted-path cases are password prompts,
key pass phrases and login.

Polaris [D29], on Windows, "adjusted [window title bars] to add an indication of the confinement domain". The EWS
quotations above are from the paper's sections 1 (size), 2.1 (principles R1 to R3), 5 (client memory), 6 and 6.1
(cut and paste), 7.1 (title bar), 7.2 (labelling) and 7.3 (pass phrases and the SaveAs agent); the paper's PDF is
RC4-encrypted with an empty password and its text was extracted in this session with a small decoder, which keeps
the words but not the page numbers.

### Tiling and floating

i3 [D30] keeps outputs, workspaces and windows "in a tree"; containers split horizontally or vertically, or stack or
tab their children; floating windows "are always on top of tiling windows"; focus follows the mouse by default and
can be turned off; workspaces group windows and are created on first use; navigation is keyboard-first. Haiku's
Stack & Tile [D20] is the floating-first hybrid: windows stay floating until the person joins them by dragging with a
modifier held. macOS is floating with system-managed states (full screen, windowed) [D3].

### Focus and stacking

`xdg_toplevel` [D31] has states maximised, fullscreen, resizing and activated; "activated" means decorations should
look focused but "do not assume this means that the window actually has keyboard or pointer focus"; interactive move
and resize are requests the client makes with the serial of the user's button press, and "the server may ignore move
requests depending on the state of the surface"; `set_minimized` has no readback ("there is no way to know if the
surface is currently minimized"); size changes are a `configure` the client acknowledges, and min and max sizes are
hints. macOS's key window is one at a time and changes when people click [D3]. rio raises on click and keeps the
current window marked by its border [D26]. ADR-0094 already fixed click to focus and one reserved key; GR-016 and
CP-24 are the catalog entries.

## Text

### Rasterisers in Rust without `std`

| Crate | Version | Licence | `no_std` | Size | What it does | Notes |
|---|---|---|---|---|---|---|
| `ttf-parser` [D32] | 0.25.1 | MIT or Apache-2.0 | yes, "zero heap allocations" | large (every table) | parses TrueType, OpenType and AAT: outlines (`glyf` and CFF), `cmap`, metrics, variations | "the library forbids unsafe code", "must not panic"; in maintenance mode, "bug fixes only", recommends fontations for new work |
| `fontdue` [D33] | 0.9.5 | MIT or Apache-2.0 or Zlib | yes, needs `alloc` | 1,972 lines over `ttf-parser` (*measured*) | rasterises a glyph to a coverage bitmap at a pixel size; `rasterize_subpixel` gives horizontal subpixel coverage; a layout helper | no hinting anywhere in its source (*measured*: no match for "hint" outside a comment on wrapping); `FontSettings.scale` default 40 px; no shaping, by design |
| `ab_glyph` [D34] | 0.2.32 | Apache-2.0 | `#![cfg_attr(not(feature = "std"), no_std)]`, a `libm` feature for floats | thin over `owned_ttf_parser` 0.25 and `ab_glyph_rasterizer` | loads, scales, positions and rasterises glyphs; `draw(|x, y, coverage| ...)` | no hinting |
| `ab_glyph_rasterizer` [D34] | 0.1.10 | Apache-2.0 | yes | 555 lines (*measured*) | "zero dependency coverage rasterization for lines, quadratic & cubic beziers" | the whole scan-line rasteriser is one small file |
| `rustybuzz` [D35] | archived | MIT | not stated | about 17 KLOC without tables | a port of HarfBuzz's shaping, "matches harfbuzz v10.1.0", 2221 of 2252 tests | "not developed further, unmaintained"; recommends HarfRust; 1.5 to 2 times slower than HarfBuzz |

The pattern every crate follows: parse the outline (quadratic Béziers from `glyf`, cubic from CFF), flatten to line
segments at the target size, accumulate signed area and coverage per pixel on a scan line, output 8-bit coverage.
`ab_glyph_rasterizer` does that in 555 lines; the parsing of `glyf`, `cmap`, `hmtx`, `head` and `hhea` that a
`glyf`-only font needs is a few hundred lines more (`ttf-parser` is large because it parses everything). An
in-house rasteriser for `glyf` fonts is therefore of the same order as `zoraya-font` plus a parser of ADR-0012's
kind; CFF outlines, variations and shaping are what the crates add beyond that.

### Hinting and antialiasing

FreeType's authors [D36]: hinting snaps outlines to the pixel grid and trades shape fidelity for sharpness; "slight"
hinting snaps vertically only and "is a compromise between design fidelity and sharpness" that keeps spacing; full
native hinting makes many fonts look worse. Grayscale antialiasing is a coverage bitmap; on low-density screens "the
pixels are too large for most of the details of a glyph". Subpixel (ClearType-style) rendering gains horizontal
resolution at the cost of "color artifacts, also called color fringes", which only linear-light blending really
minimises. Gamma-correct blending thins text, which stem darkening offsets. On high-density screens these problems
matter less. For a system that reads its own screen back in tests (ADR-0095's driver 3), grayscale coverage without
hinting is the deterministic choice: the same outline, size and position give the same bytes, with no font-program
interpreter in the path.

### Shaping, and what Latin and Cyrillic need

HarfBuzz defines shaping as "the process of translating a string of character codes" into positioned glyphs, driven
by "the input string, the active font, the script" and language, with rules "incorporat[ed] as lookups in its GSUB
and GPOS tables" [D37]. Its explainer [D38] gives the cases: Arabic letters take a form by position ("text shaping
chooses the correct form of the letter by its position"); Tamil consonant plus vowel sign "must be replaced by the
single glyph"; a combining mark such as a breve over Cyrillic zhe must be positioned; the Latin "fi" ligature is
optional. For Latin, Cyrillic including Ukrainian and Czech with diacritics, every letter the keyboard layouts below
produce is a precomposed code point (U+011B ě, U+0159 ř, U+016F ů, U+0457 ї, U+0454 є, U+0456 і, U+0491 ґ, from the
keysym table [D39]), so one glyph per character from `cmap` plus the advance from `hmtx` renders them correctly;
kerning, ligatures and mark positioning are quality, not correctness, for these scripts. Right-to-left and the
joining and reordering scripts need a shaper. UTF-8 is settled (GR-015, CP-22).

### Font fallback

Fallback is a client's walk over an ordered list of fonts, first that has a glyph for the code point wins, with a
visible "missing" glyph at the end; ADR-0095 draws a box, Plan 9 loaded subfonts per range (ADR-0095 Option D). None
of the crates above does fallback; `egui` asks the app to install a font for non-Latin text [D40].

### Candidate fonts and their licences

All sizes and coverages below are *measured* in this session on the files named, with a `cmap` reader written for
the purpose; "required letters" are the Ukrainian ґ є і ї (and the apostrophe U+02BC), the Czech ě š č ř ž ý á í é ú
ů ď ť ň ó and the Russian ы ъ э ё, in both cases.

| Font | Version, file | Licence | Reserved font name | Bytes | Code points mapped | Cyrillic U+0400–04FF | Latin Extended-A | Required letters |
|---|---|---|---|---|---|---|---|---|
| Inter | 4.1, `extras/ttf/Inter-Regular.ttf` [D41] | SIL OFL 1.1, "Copyright (c) 2016 The Inter Project Authors" | none declared | 411,640 | 2,852 | 248 of 256 | 127 of 128 | all |
| JetBrains Mono | 2.304, `fonts/ttf/JetBrainsMono-Regular.ttf` [D42] | SIL OFL 1.1, "Copyright 2020 The JetBrains Mono Project Authors" | none declared | 273,900 | 1,363 | 122 of 256 | 126 of 128 | all |
| DejaVu Sans | 2.37, `ttf/DejaVuSans.ttf` [D43] | Bitstream Vera licence; "DejaVu changes are in public domain" | the words "Bitstream" and "Vera" may not name a modified font | 757,076 | 5,918 | 256 of 256 | 128 of 128 | all |
| DejaVu Sans Mono | 2.37, `ttf/DejaVuSansMono.ttf` [D43] | as above | as above | 340,712 | 3,322 | 180 of 256 | 128 of 128 | all |
| Noto Sans | variable `NotoSans[wdth,wght].ttf` from Google Fonts [D44] | SIL OFL 1.1, "Copyright 2022 The Noto Project Authors" | none declared | 2,049,096 | 3,094 | 256 of 256 | 128 of 128 | all (the arrow U+2192 is absent) |

Every file is TrueType with `glyf` outlines (the first four bytes are `00 01 00 00`), so a `glyf`-only rasteriser
reads them; Inter also ships `Inter.ttc` (13 MB) and variable fonts, which need `fvar`/`gvar` for anything but the
default instance.

### The OFL next to BSD code

The OFL FAQ [D45]: "fonts licensed under the OFL can be freely included alongside other software under FLOSS", and
"only the portions based on the Font Software are required to be released under the OFL", so the software's licence
does not change; the fonts may not be sold by themselves ("the intent is to keep people from making money by simply
redistributing the fonts"); embedding in documents is free; a modified font, including a subset, is a Modified
Version that must drop any Reserved Font Name and stay under the OFL. None of Inter, JetBrains Mono and Noto Sans
declares a reserved name (*measured*: the only occurrence of the phrase in each licence file is its definition). The
Bitstream Vera licence [D43] grants reproduction, distribution and sale "as part of a larger software package but no
copy ... may be sold by itself", and requires a modified font to be renamed away from "Bitstream" and "Vera". Both
are compatible with a BSD-2-Clause repository that ships the files unmodified with their licence texts; subsetting a
font to save space is a modification under both and needs a rename.

## Widget toolkits

### Own `no_std` widgets

The precedent is ADR-0095 and `zoraya-term`: a crate that owns a pixel buffer, draws from a token table and has host
tests over the pixels. The cost is every widget written by hand; the gain is one design system with no translation
layer, total control of input handling (ADR-0094's rules), and no licence or dependency question.

### LVGL

LVGL [D46] is "a fully portable C (C++ compatible) library with no external dependencies" under MIT, with 30 or more
widgets, a style system of "100+ style properties", flex and grid layouts, antialiased 2D rendering, and UTF-8 text
including complex scripts. It needs "at a bare minimum ... only 32kB RAM and 128kB Flash" plus a render buffer of a
tenth of the screen, about 100 kB RAM and 200 to 300 kB flash for a typical UI. Porting is a display with render
buffers and a flush callback that copies pixels out and calls `lv_display_flush_ready`, an input device with a read
callback giving a point and a pressed state, a millisecond tick and `lv_timer_handler` in the main loop. On Zoraya
it would be a C library in a native process (needs ADR-0109's toolchain) or behind `libposix`; its look is its own
theme system, not Zoraya's tokens, unless a theme is written for it.

### Slint

Slint's repository [D47] offers three licences: a Royalty-free licence for "proprietary desktop, mobile, and web
applications" that excludes embedded systems ("use in embedded systems is excluded") and requires disclosure ("you
must disclose that you use Slint", through the `AboutSlint` widget or a badge); GPLv3; or a paid commercial licence
for everything including embedded. Whether a hobby operating system under QEMU counts as a desktop or an embedded
system is a question the licence does not answer, and the disclosure requirement is a condition on the product's
UI. Neither GPLv3 nor a disclosure condition fits CLAUDE.md's rule of permissive `no_std` crates. Whether Slint's
renderer runs without `std` on Zoraya was not checked, since the licence decides first.

### egui and iced

`egui` [D40] is "an easy-to-use immediate mode GUI in Rust", MIT or Apache-2.0, "a library you call into", with a
custom integration that feeds input each frame and paints the triangle meshes it produces; it bundles the fonts Hack,
Ubuntu Light and Noto Emoji, needs an installed font for other scripts, says "if you want a GUI that looks native,
egui is not for you", lays out everything every frame (large scroll areas cost CPU), theming "is not yet as powerful
as say CSS", and "new releases will have breaking changes". `iced` [D48] is an Elm-architecture toolkit under MIT with
`wgpu` and `tiny-skia` renderers, "currently experimental software". Both are `std` crates in practice: their shells
are `winit` and their renderers `wgpu` or software, and they would need a Rust `std` for Zoraya and a compositor
backend written for them.

## Capability-oriented desktops: the powerbox

### CapDesk and E

Miller, Tulloh and Shapiro [D49] state the pattern from the command line up: with `cp foo.txt bar.txt` the program
gets strings and "must already have the authority to use your namespace", whereas with `cat < foo.txt > bar.txt`
"your shell passes direct access to the files to cat, as open file descriptors", so cat's least authority is two
files. "The key to usable POLA is to bundle authority with designation": "to use Excel to edit killer.xls, Doug must
somehow designate this file ... by double clicking on the file, by selecting it in an open file dialog box, or by
drag-and-drop", and that act grants the file and nothing else. CapDesk "is a capability-secure distributed desktop
written in E, for running caplets"; "CapDesk is part of Doug's TCB; but the programs launched by CapDesk are not";
drag-and-drop was supported by CapDesk; the DarpaBrowser was a caplet "able to use a potentially malicious plug-in as a
renderer", reviewed by Wagner. This is the "powerbox": a trusted chooser that turns the person's choice into a
granted handle.

### Polaris

Polaris [D29] ran unmodified Windows XP applications each confined in its own account with "very limited access to the
disk". "Security by designation": "when the user double-clicks a file to open it in its associated application, that
double-click can be interpreted as an indication that the launched application should be granted access to that
one file"; a Save command that names a file grants write to "just that one file". Polaris "replaces the
application's Open and Save dialog boxes with dialog boxes from Polaris" and then copies and synchronises the chosen
file into the confined account; when there is no act of designation (images beside a web page) it compromises with
read-only access to the directory. Title bars show the confinement domain. Miller, Tulloh and Shapiro add that
"Polaris users regularly run with macros enabled, since they no longer live in fear of their documents" [D49].

### EROS's trusted window system

See [Spoofing and the trusted path](#spoofing-and-the-trusted-path): the SaveAs agent holds the file-system
capability, so a forged dialog cannot forge the authority; paste and drag are traced to a user action before any data
moves [D28].

### macOS's sandboxed open panel

Apple's documentation [D50]: a sandboxed app has its own container under `~/Library/Containers` with full access
there and nowhere else; to reach other files it presents a standard open or save panel, and "the operating system
displays open and save panels in a separate process, and extends your app's sandbox to include the selected URLs".
Access is "security-scoped"; to keep a file across launches the app stores a security-scoped bookmark and must call
`startAccessingSecurityScopedResource` when it resolves it; files dragged onto the Dock icon are granted the same
way; writing programs into user-selected locations needs a further entitlement. The user-selected entitlements come
in read-only and read-write forms.

### xdg-desktop-portal

The FileChooser portal [D51] "allows sandboxed applications to ask the user for access to files outside the
sandbox"; the portal shows the dialog, and "the selected files will be made accessible to the application (which
may involve adding it to the Documents portal)"; files granted that way "stay accessible across sessions". Methods:
`OpenFile` (one or more files, or a directory), `SaveFile` (one location), `SaveFiles` (a folder for several
names), with options for filters, choices, an accept label and modality.

## Porting Rust's `std`

### What a target needs

The standard library's platform code lives in `library/std/src/sys` [D52], split by concern: `alloc`, `args`,
`env`, `fd`, `fs`, `io`, `net`, `os_str`, `path`, `paths`, `pipe`, `process`, `random`, `stdio`, `sync`, `thread`,
`thread_local`, `time`, plus `backtrace`, `exit` and `personality`. Each concern selects its implementation with
`cfg_select!`: `fs` has branches for Unix (with WASI), Windows, Hermit, Motor, SOLID, UEFI, VEXos and a fallback to
`unsupported` [D53]; `thread` has twelve branches and each platform re-exports from `unsupported` what it lacks
(Hermit takes `current_os_id` and `set_name` from it; UEFI has no threads at all and takes `Thread` itself from it)
[D54]. Under them, `pal` ("the platform abstraction layer contains platform-specific abstractions for implementing
the features in the other submodules, such as e.g. bindings") has sixteen platforms: hermit, itron, motor, sgx,
solid, teeos, trusty, uefi, unix, unsupported, vexos, wasi, wasm, windows, xous and zkvm [D55]. The Hermit module's
own comment describes the starting state of every port: "super highly experimental and not actually intended for
wide/production use yet", "all functions here are basically stubs that immediately return errors" [D56]. A new target
also needs a target specification JSON added to `rustc_target`'s `supported_targets` table and a file with its
`target` function, an entry in bootstrap's `STAGE0_MISSING_TARGETS`, usually a patched `libc` crate, and a build of
`compiler/rustc library/std` with `./x` [D57]. The target tier policy [D58] admits a tier 3 target with a named
maintainer, a conventional name, no licence incompatibility, no breakage of other targets and documentation on how
to build; "Rust provides no guarantees about tier 3 targets"; tier 2 adds two maintainers, CI builds, and that no
"significant portions of `core` or the standard library" stay stubbed.

### What Redox and Fuchsia did

Redox's targets are `x86_64-unknown-redox` (tier 2) and `aarch64-unknown-redox` (tier 3) among others; "std is fully
supported"; building needs "a copy of relibc" in the linker search path, so `std` sits on a C library whose platform
layer is Redox's [D59]. Fuchsia's `aarch64-unknown-fuchsia` and `x86_64-unknown-fuchsia` are tier 2, "support
`std`", and are cross-compiled with "a recent copy of the Fuchsia SDK, which provides the tools, libraries, and
binaries required to build and link programs for Fuchsia": a sysroot, `libfdio.so` and `ld.so.1` [D60]. Both are the
"over a libc" shape: `std`'s `unix` pal is reused and the work goes into the libc and its platform layer.

### Hermit and Xous: `std` without a libc

Hermit is a unikernel "built from scratch" in Rust; its four targets are tier 3, "the targets do support std", and
"the targets do not yet support C code and Rust code at the same time" [D61]; `hermit-abi` "builds the interface
between library operating system and the application" [D62], so `std`'s `hermit` pal calls Rust functions, not a
libc. Xous is "a microkernel operating system written in pure Rust" with "std support in stable Rust (tier-3 Rust
target)" [D63][D64]; its pal is one of the sixteen [D55]. These are the "natively" shape: a pal that calls the
kernel's own system calls, with the concerns the kernel does not offer left on `unsupported`.

### The cost

In either shape the port is the sum of the concerns: files, directories and paths; environment and arguments;
processes (spawn, wait, exit); time; threads and synchronisation (mutex, condition variable, thread-local storage,
parking); stdio; pipes; networking (or `unsupported`); random; allocation; backtraces (or none). Hermit's and Xous's
pals exist because each concern can start as a stub (`unsupported`) and be filled in; the policy requires only that
a tier 3 target build. Zoraya has today: files and directories through a protocol (ADR-0055), processes (ADR-0003),
time (ADR-0023), allocation over memory objects (ADR-0051), stdio over a channel or a byte stream (ADR-0096,
ADR-0101), and no threads (ADR-0005 keeps them out; ADR-0030 is one thread per process). The missing pieces for a
pal are mostly threads and synchronisation, which `std` needs at start-up for `thread_local` and for `std::sync`.
The library-over-libc shape gets them from `libposix` if and when `libposix` has them; the native shape writes them
against `zoraya-rt`. Nothing measured; the line counts of other pals are the only guide, and they are not cited
here because the files were not read in full.

## Keyboard layouts

xkeyboard-config's layouts are the reference used by every Linux desktop [D65][D66][D67][D68]:

- **Czech** (`cz`, "basic", QWERTZ): the number row gives `+ ě š č ř ž ý á í é` unshifted and the digits with Shift;
  `=` is `dead_acute` (Shift: `dead_caron`), `ú` is on `[`, `ů` on `;`, `)` on `]` with `(` shifted; `y` and `z` are
  swapped against US; the third level (AltGr) gives `\ | € [ ] { } @ # &` and the fourth more dead keys. Variants:
  `qwerty`, `bksl`, `winkeys`, `qwerty-mac`, `ucw`, `prog`, `coder` and others [D65] (the upstream file; the
  copy libxkbcommon keeps as test data lacks `winkeys`).
- **Ukrainian** (`ua`, "unicode", the default): ЙЦУКЕН with `ї` on `]`, `і` on `s`, `є` on `'`, `ґ` on `\` and on
  AltGr with `г`; the apostrophe on the key left of `1` and U+02BC on its Shift; third-level quotes `«»„“` and the
  hryvnia sign. Variants: `macOS`, `legacy`, `winkeys`, `typewriter`, `phonetic`, `homophonic` [D66].
- **Russian** (`ru`, "winkeys" is the default, built on "common"): ЙЦУКЕН with `ё` left of `1`, `ы ъ э` on `s ] '`,
  and the Windows arrangement of the number-row punctuation (`№ ; % : ? *`) [D67].
- **US** (`us`, "basic"): the plain QWERTY table [D68].

Every keysym maps to one Unicode code point in the header libxkbcommon ships [D39]: `ecaron` is U+011B, `rcaron`
U+0159, `uring` U+016F, `Ukrainian_yi` U+0457, `Ukrainian_ie` U+0454, `Ukrainian_i` U+0456,
`Ukrainian_ghe_with_upturn` U+0491, `Cyrillic_yeru` U+044B; the dead keys are keysyms of their own (`dead_acute`
0xfe51, `dead_caron` 0xfe5a, `dead_abovering` 0xfe58, `dead_diaeresis` 0xfe57).

How the pieces fit in XKB [D69]: a keymap has `keycodes` (hardware codes to names), `types` (modifier combinations
to shift *levels*), `compat` (what modifiers do) and `symbols` (names to keysyms per level); "layouts are called
groups in XKB", ordered and switched at run time by SetGroup, LatchGroup and LockGroup actions; level 1 is the plain
key, level 2 Shift, level 3 AltGr. Dead keys and compose are separate from the keymap [D70]: "nothing initially
happens" on `dead_acute`; the next keysym completes a sequence such as `<dead_acute> <a> : "á" aacute` from a Compose
file, or cancels it (swallow, pass through or replay the cancelling key); the state machine runs in the client after
keysym translation. Under Wayland the compositor sends each client the whole keymap as a file descriptor
(`wl_keyboard.keymap`), raw key codes with each event, and a `modifiers` event that "updates the modifiers and
group" [D71]. macOS switches input sources from the Input menu in the menu bar, with Control-Space for the last used
source and Control-Option-Space for the next, or with the Fn or Globe key, or optionally with Caps Lock between a
Latin and a non-Latin source [D72]. Haiku's Keymap preferences show the layout as a keyboard, mark dead keys in blue,
let a key's output be edited by drag and drop, and keep user maps as files [D22].

## Display scaling

Wayland's core [D71]: `wl_surface.set_buffer_scale` is an integer; "it is intended that you pick the same buffer
scale as the scale of the output", and "if the scale is larger than 1, then you have to attach a buffer that is
larger (by a factor of scale in each dimension)"; `wl_output.scale` exists "for very high resolution displays where
applications rendering at the native resolution would be too small to be legible". `fractional-scale-v1` [D73]
later added fractions: "the sent scale is the numerator of a fraction with a denominator of 120" (180 is 1.5), the
client renders at surface size times scale into a buffer with scale 1 and maps it with a viewport, and toplevel sizes
round half away from zero. Apple [D74]: "the two defining characteristics of high resolution for OS X are high
pixel density and the virtual screen"; "there are four times as many pixels for a given area on a high-resolution
display"; "OS X refers to screen size in points, not pixels"; apps draw in points and the frameworks map to pixels,
so most "won't need to be aware" of the display. Windows [D75]: the base is 96 DPI; a "DPI unaware" app is
bitmap-stretched and "results in the application appearing blurry"; "system DPI aware" apps are crisp at one scale
only; per-monitor v2 apps get `WM_DPICHANGED` and must relayout, while Windows scales the non-client area (caption,
scroll bars) and dialogs for them; the common pitfalls are cached DPI values and APIs with no DPI context.

## Design tokens and motion

The Design Tokens Community Group's format [D76] is a JSON file (`.tokens.json`) where "a token is information
associated with a human readable name, at minimum a name/value pair": an object with `$value` is a token, one
without is a group; `$type` is set on a token or inherited from its group and "tools MUST NOT attempt to guess the
type"; references are `{group.token}` and "MUST NOT be circular"; simple types are color, dimension, fontFamily,
fontWeight, duration, cubicBezier and number, composite types strokeStyle, border, transition, shadow, gradient and
typography. It is a draft ("do not attempt to implement this version"), useful as a vocabulary rather than a
dependency. Material's motion tokens [D77] are the one published set with numbers: durations short 50, 100, 150,
200 ms, medium 250 to 400, long 450 to 600, extra long 700 to 1000; easings standard `cubic-bezier(0.2, 0, 0, 1)`,
standard-accelerate `(0.3, 0, 1, 1)`, standard-decelerate `(0, 0, 0, 1)`, emphasized-decelerate
`(0.05, 0.7, 0.1, 1)`, emphasized-accelerate `(0.3, 0, 0.8, 0.15)`, linear. Apple gives rules, not numbers [D4].

## Settings

GSettings [D78]: "a convenient API for storing and retrieving application settings"; every instance needs "a schema
that describes the keys in your settings and their types and default values", compiled from XML; dconf is the usual
backend; `changed` signals report a key that "has potentially changed"; vendors adjust defaults with
`.gschema.override` files; and "you should only ever modify GSettings keys in response to explicit user action".
FreeBSD's `rc.conf(5)` [D79] holds the host's name, interfaces and services as `sh` assignments, "included by the
various generic startup scripts"; `/etc/defaults/rc.conf` lists every default and `/etc/rc.conf` only overrides;
`/etc/rc.conf.d/` holds per-service files; `sysrc(8)` [D80] reads and sets variables "in a safe and effective
manner" (`sysrc name`, `name=value`, `+=`, `-=`, `-x` removes, `-a` lists non-defaults, `-c` checks), appending a new
variable to the first file and replacing only the last occurrence. Haiku keeps preferences as separate panels and
settings as files under `~/config/settings` [D21][D22]. The catalog's lessons: a kernel-resident shared database
(DM-086) and per-program text syntax with no schema (DM-087) are the two failure modes; typed, owned, schema-checked
configuration is CP-23, and no implicit process-global configuration is CP-21.

## What this suggests for stage 7

Not decisions; inputs to ADR-0160 to ADR-0169.

1. Every coherent desktop has one type scale, one colour vocabulary and one party drawing the frame. The frame drawn
   by the compositor is also the only known defence against a client spoofing another's title or dialog (EWS), and
   it is what Zoraya's compositor does today for its one-pixel border.
2. Latin, Cyrillic and Czech need a `cmap` lookup and advances, not a shaper; a `glyf` rasteriser is 555 lines in
   the smallest crate; hinting is a quality trade that costs determinism. The fonts that cover the required letters
   under a licence compatible with BSD code are Inter, JetBrains Mono, DejaVu and Noto Sans.
3. The powerbox is the same pattern five times over (CapDesk, Polaris, EWS, macOS, portals): the chooser is the
   trusted party, the person's choice is the grant, and the application never names a file it was not given. It fits
   ADR-0002 and ADR-0053 without a new kernel object.
4. A Rust `std` is a set of per-concern modules; the two shapes are a pal over `libposix` (Redox, Fuchsia) or over
   `zoraya-rt` (Hermit, Xous). Threads are the concern Zoraya lacks for either.
5. Keyboard layouts are tables of key to code point per level, with dead keys as a small state machine after the
   table; four layouts are a few hundred table rows.
6. Scaling is an integer factor at which clients render larger buffers (Wayland core, macOS's two times); fractional
   factors came later and cost a viewport protocol.

## Sources

Format: `[D<n>] what, URL, type, checked 2026-11-03, what it was read for`.

- [D1] Apple, Human Interface Guidelines, Typography (data endpoint), https://developer.apple.com/tutorials/data/design/human-interface-guidelines/typography.json, primary, checked 2026-11-03. Default and minimum sizes per platform, weights, the two families, text styles and the macOS table.
- [D2] Apple, HIG, Color (data endpoint), https://developer.apple.com/tutorials/data/design/human-interface-guidelines/color.json, primary, checked 2026-11-03. System and dynamic colours, light, dark and increased contrast, no hard-coded values.
- [D3] Apple, HIG, Windows (data endpoint), https://developer.apple.com/tutorials/data/design/human-interface-guidelines/windows.json, primary, checked 2026-11-03. Frame and body, main, key and inactive windows, no custom window UI.
- [D4] Apple, HIG, Motion (data endpoint), https://developer.apple.com/tutorials/data/design/human-interface-guidelines/motion.json, primary, checked 2026-11-03. Purposeful, optional, brief, cancellable motion.
- [D5] Apple, HIG, Layout (data endpoint), https://developer.apple.com/tutorials/data/design/human-interface-guidelines/layout.json, primary, checked 2026-11-03. Reading order, alignment, grouping, progressive disclosure.
- [D6] GNOME, Human Interface Guidelines, Design principles, https://developer.gnome.org/hig/principles.html, primary, checked 2026-11-03. The four principles.
- [D7] GNOME HIG, Typography, https://developer.gnome.org/hig/guidelines/typography.html, primary, checked 2026-11-03. Adwaita Sans, text styles, conventions.
- [D8] GNOME HIG, UI styling, https://developer.gnome.org/hig/guidelines/ui-styling.html, primary, checked 2026-11-03. Light default, dark, high contrast, reuse of named colours.
- [D9] libadwaita, `src/stylesheet/_colors.scss` (GitLab API raw), https://gitlab.gnome.org/api/v4/projects/GNOME%2Flibadwaita/repository/files/src%2Fstylesheet%2F_colors.scss/raw?ref=main, primary, checked 2026-11-03. Named colours for light and dark.
- [D10] libadwaita, `src/stylesheet/_palette.scss` (GitLab API raw), https://gitlab.gnome.org/api/v4/projects/GNOME%2Flibadwaita/repository/files/src%2Fstylesheet%2F_palette.scss/raw?ref=main, primary, checked 2026-11-03. The palette variables the named colours refer to.
- [D11] GNOME HIG, Palette, https://developer.gnome.org/hig/reference/palette.html, primary, checked 2026-11-03. The 45 colours with hex values.
- [D12] GNOME HIG, Header bars, https://developer.gnome.org/hig/patterns/containers/header-bars.html, primary, checked 2026-11-03. Contents and rules of a header bar.
- [D13] libadwaita, Style classes, https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/style-classes.html, primary, checked 2026-11-03. Toolbar 6 px margins, 1 px frame.
- [D14] GNOME HIG, UI icons, https://developer.gnome.org/hig/guidelines/ui-icons.html, primary, checked 2026-11-03. Symbolic icon rules.
- [D15] Tobias Bernard, "Introducing the CSD Initiative", GNOME blogs, 2018, https://blogs.gnome.org/tbernard/2018/01/26/csd-initiative/, primary (the author's position), checked 2026-11-03. The case for client-side decorations.
- [D16] KDE, Human Interface Guidelines (landing page), https://develop.kde.org/hig/, primary, checked 2026-11-03. Aims and structure; sub-pages not read.
- [D17] Martin Flöser (Gräßlin), "Server-side decorations and Wayland", 2018, https://blog.martin-graesslin.com/blog/2018/01/server-side-decorations-and-wayland/, primary (the protocol's author), checked 2026-11-03. Which compositors require CSD; the protocol's history.
- [D18] Haiku, Human Interface Guidelines (draft, 2006), https://www.haiku-os.org/docs/HIG/index.xml, primary, checked 2026-11-03, read to chapter 12. Window, layout, font and icon rules.
- [D19] Haiku, Be Book, `BWindow`, https://www.haiku-os.org/legacy-docs/bebook/BWindow.html, primary, checked 2026-11-03. Window looks and feels.
- [D20] Haiku, User guide, GUI, https://www.haiku-os.org/docs/userguide/en/gui.html, primary, checked 2026-11-03. Tabs, buttons, Stack & Tile, Deskbar.
- [D21] Haiku, User guide, Preferences, https://www.haiku-os.org/docs/userguide/en/preferences.html, primary, checked 2026-11-03. The preference panels.
- [D22] Haiku, User guide, Keymap, https://www.haiku-os.org/docs/userguide/en/preferences/keymap.html, primary, checked 2026-11-03. Layouts, dead keys, keymap files.
- [D23] SerenityOS, `README.md`, https://github.com/SerenityOS/serenity/blob/master/README.md, primary, checked 2026-11-03. Philosophy, licence, window server.
- [D24] SerenityOS, `Userland/Services/WindowServer` (directory listing), https://github.com/SerenityOS/serenity/tree/master/Userland/Services/WindowServer, primary, checked 2026-11-03. Which parts the server owns, from file names only.
- [D25] SerenityOS, HIG, Text, https://github.com/SerenityOS/serenity/blob/master/Documentation/HumanInterfaceGuidelines/Text.md, primary, checked 2026-11-03. Capitalisation and ellipsis rules.
- [D26] Plan 9, `rio(1)`, https://9p.io/magic/man2html/1/rio, primary, checked 2026-11-03. Window operations, the current window, `/dev/cons`.
- [D27] wayland-protocols, `xdg-decoration-unstable-v1`, https://wayland.app/protocols/xdg-decoration-unstable-v1, primary, checked 2026-11-03. Modes and who decides.
- [D28] J. S. Shapiro, J. Vanderburgh, E. Northup, D. Chizmadia, "Design of the EROS Trusted Window System", USENIX Security 2004, https://www.usenix.org/legacy/event/sec04/tech/full_papers/shapiro/shapiro.pdf (text extracted locally; abstract at https://www.usenix.org/legacy/event/sec04/tech/shapiro.html), primary, checked 2026-11-03. Size, principles, decorations, labels, paste, trusted path.
- [D29] K.-P. Yee, M. Stiegler, A. Karp, T. Close, M. Miller, "Polaris: Usable Virus Protection for Windows", SOUPS 2005 poster, https://cups.cs.cmu.edu/soups/2005/2005posters/9-yee.pdf, primary, checked 2026-11-03. Security by designation, replaced dialogs, title bars.
- [D30] i3, User's Guide, https://i3wm.org/docs/userguide.html, primary, checked 2026-11-03, first 100,000 characters. Tree, layouts, floating, focus, workspaces.
- [D31] wayland-protocols, `xdg-shell`, https://wayland.app/protocols/xdg-shell, primary, checked 2026-11-03. Toplevel states, move, resize, minimise, configure.
- [D32] `ttf-parser` repository, https://github.com/harfbuzz/ttf-parser, primary, checked 2026-11-03; version 0.25.1 from crates.io. Scope, safety, maintenance status.
- [D33] `fontdue` repository and crate, https://github.com/mooman219/fontdue and https://docs.rs/fontdue/latest/fontdue/, primary, checked 2026-11-03; source of 0.9.5 read from crates.io. Licences, `no_std`, API, no hinting, no shaping.
- [D34] `ab_glyph` repository and crates, https://github.com/alexheretic/ab-glyph and https://docs.rs/ab_glyph/latest/ab_glyph/, primary, checked 2026-11-03; sources of `ab_glyph` 0.2.32 and `ab_glyph_rasterizer` 0.1.10 read from crates.io. Features, licence, size.
- [D35] `rustybuzz` repository, https://github.com/harfbuzz/rustybuzz, primary, checked 2026-11-03. Status, size, test coverage.
- [D36] FreeType, "Text rendering: general", https://freetype.org/freetype2/docs/hinting/text-rendering-general.html, primary, checked 2026-11-03. Hinting, antialiasing, subpixel, gamma.
- [D37] HarfBuzz, "What is text shaping?", https://harfbuzz.github.io/what-is-harfbuzz.html, primary, checked 2026-11-03.
- [D38] HarfBuzz, "Why do I need a shaping engine?", https://harfbuzz.github.io/why-do-i-need-a-shaping-engine.html, primary, checked 2026-11-03. Arabic, Tamil, marks, ligatures.
- [D39] libxkbcommon, `xkbcommon-keysyms.h`, https://raw.githubusercontent.com/xkbcommon/libxkbcommon/master/include/xkbcommon/xkbcommon-keysyms.h, primary, checked 2026-11-03. Keysym to Unicode.
- [D40] `egui` repository, https://github.com/emilk/egui, primary, checked 2026-11-03. Model, licence, fonts, integration, limits.
- [D41] Inter 4.1 release, https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip (SHA-256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`), primary, checked 2026-11-03. `LICENSE.txt` and the font files.
- [D42] JetBrains Mono 2.304 release, https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip (SHA-256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`), primary, checked 2026-11-03. `OFL.txt` and the font files.
- [D43] DejaVu fonts 2.37 release, https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2 (SHA-256 `fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7`), primary, checked 2026-11-03. `LICENSE` and the font files.
- [D44] Noto Sans (variable) from the Google Fonts repository, https://raw.githubusercontent.com/google/fonts/main/ofl/notosans/NotoSans%5Bwdth,wght%5D.ttf (SHA-256 `bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d`) and its `OFL.txt`, and the project's `OFL.txt` at https://github.com/notofonts/latin-greek-cyrillic/blob/main/OFL.txt, primary, checked 2026-11-03.
- [D45] SIL, OFL FAQ, https://openfontlicense.org/ofl-faq/, primary, checked 2026-11-03. Bundling, selling, reserved names, subsetting.
- [D46] LVGL repository, https://github.com/lvgl/lvgl, primary, checked 2026-11-03. Licence, requirements, porting model.
- [D47] Slint, `LICENSE.md`, https://github.com/slint-ui/slint/blob/master/LICENSE.md, primary, checked 2026-11-03. The three licences and their conditions.
- [D48] `iced` repository, https://github.com/iced-rs/iced, primary, checked 2026-11-03. Model, licence, renderers, maturity.
- [D49] M. S. Miller, B. Tulloh, J. S. Shapiro, "The Structure of Authority: Why Security Is not a Separable Concern", http://www.erights.org/talks/no-sep/secnotsep.pdf (text extracted locally; the page http://www.erights.org/talks/no-sep/ says the paper is superseded by "Robust Composition"), primary, checked 2026-11-03. `cp` versus `cat`, designation as authorisation, CapDesk, Polaris.
- [D50] Apple, "Accessing files from the macOS App Sandbox" (Markdown version), https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox.md, primary, checked 2026-11-03. Containers, open and save panels in a separate process, security-scoped URLs and bookmarks.
- [D51] xdg-desktop-portal, `org.freedesktop.portal.FileChooser`, https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html, primary, checked 2026-11-03.
- [D52] Rust, `library/std/src/sys` (directory listing), https://github.com/rust-lang/rust/tree/master/library/std/src/sys, primary, checked 2026-11-03.
- [D53] Rust, `library/std/src/sys/fs/mod.rs`, https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/mod.rs, primary, checked 2026-11-03. Platform branches of the filesystem concern.
- [D54] Rust, `library/std/src/sys/thread/mod.rs`, https://github.com/rust-lang/rust/blob/master/library/std/src/sys/thread/mod.rs, primary, checked 2026-11-03. Platform branches of the thread concern and the `unsupported` fallbacks.
- [D55] Rust, `library/std/src/sys/pal/mod.rs` and the `pal` directory, https://github.com/rust-lang/rust/blob/master/library/std/src/sys/pal/mod.rs and https://github.com/rust-lang/rust/tree/master/library/std/src/sys/pal, primary, checked 2026-11-03. The pal's purpose and its sixteen platforms.
- [D56] Rust, `library/std/src/sys/pal/hermit/mod.rs`, https://github.com/rust-lang/rust/blob/master/library/std/src/sys/pal/hermit/mod.rs, primary, checked 2026-11-03. The module comment on stubs.
- [D57] rustc dev guide, "Adding a new target", https://rustc-dev-guide.rust-lang.org/building/new-target.html, primary, checked 2026-11-03.
- [D58] Rust, Target tier policy, https://doc.rust-lang.org/rustc/target-tier-policy.html, primary, checked 2026-11-03.
- [D59] Rust platform support, Redox, https://doc.rust-lang.org/rustc/platform-support/redox.html, primary, checked 2026-11-03.
- [D60] Rust platform support, Fuchsia, https://doc.rust-lang.org/rustc/platform-support/fuchsia.html, primary, checked 2026-11-03.
- [D61] Rust platform support, Hermit, https://doc.rust-lang.org/rustc/platform-support/hermit.html, primary, checked 2026-11-03.
- [D62] hermit-rs, `README.md`, https://github.com/hermit-os/hermit-rs/blob/main/README.md, primary, checked 2026-11-03. `hermit-abi`'s role.
- [D63] Rust platform support, `riscv32imac-unknown-xous-elf`, https://doc.rust-lang.org/rustc/platform-support/riscv32imac-unknown-xous-elf.html, primary, checked 2026-11-03.
- [D64] Xous, `README.md`, https://github.com/betrusted-io/xous-core/blob/main/README.md, primary, checked 2026-11-03. Pure Rust, `std` in stable Rust.
- [D65] xkeyboard-config, `symbols/cz`, https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/raw/master/symbols/cz, primary, checked 2026-11-03.
- [D66] xkeyboard-config, `symbols/ua`, https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/raw/master/symbols/ua, primary, checked 2026-11-03.
- [D67] xkeyboard-config, `symbols/ru`, https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/raw/master/symbols/ru, primary, checked 2026-11-03.
- [D68] xkeyboard-config, `symbols/us`, https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/raw/master/symbols/us, primary, checked 2026-11-03.
- [D69] libxkbcommon, "The XKB keymap text format", https://xkbcommon.org/doc/current/keymap-text-format-v1-v2.html, primary, checked 2026-11-03, first 100,000 characters. Sections, groups, levels.
- [D70] libxkbcommon, Compose and dead keys, https://xkbcommon.org/doc/current/group__compose.html, primary, checked 2026-11-03.
- [D71] Wayland, `protocol/wayland.xml`, https://gitlab.freedesktop.org/wayland/wayland/-/raw/main/protocol/wayland.xml, primary, checked 2026-11-03. `wl_surface.set_buffer_scale`, `wl_output.scale`, `wl_keyboard.keymap`, `wl_keyboard.modifiers`.
- [D72] Apple, macOS User Guide, "Use input sources to type in other languages", https://support.apple.com/guide/mac-help/use-input-sources-to-type-in-other-languages-mchlp1406/mac, primary, checked 2026-11-03.
- [D73] wayland-protocols, `fractional-scale-v1`, https://wayland.app/protocols/fractional-scale-v1, primary, checked 2026-11-03.
- [D74] Apple, High Resolution Guidelines for OS X, "High Resolution Explained", https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Explained/Explained.html, primary (archived), checked 2026-11-03.
- [D75] Microsoft, "High DPI Desktop Application Development on Windows", https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows, primary, checked 2026-11-03.
- [D76] Design Tokens Community Group, Design Tokens Format Module (draft), https://www.designtokens.org/TR/drafts/format/, primary, checked 2026-11-03, first 100,000 characters.
- [D77] Material Web, `tokens/versions/v0_192/_md-sys-motion.scss`, https://raw.githubusercontent.com/material-components/material-web/main/tokens/versions/v0_192/_md-sys-motion.scss, primary, checked 2026-11-03. Duration and easing values.
- [D78] GLib, `GSettings` class documentation, https://docs.gtk.org/gio/class.Settings.html, primary, checked 2026-11-03.
- [D79] FreeBSD, `rc.conf(5)`, https://man.freebsd.org/cgi/man.cgi?query=rc.conf&sektion=5, primary, checked 2026-11-03, DESCRIPTION.
- [D80] FreeBSD, `sysrc(8)`, https://man.freebsd.org/cgi/man.cgi?query=sysrc&sektion=8, primary, checked 2026-11-03.
