# Stage 7 mockups

One direction remains: `clear-field`, the "Clear boundaries" concept in its Field variant with dock B, which the
architect chose on 2026-11-03 after looking at three directions, a second version of one of them, and three variants
of the concept. The earlier directions (`mist`, `mist-v2`, `quiet`, `night`, `clear-line`, `clear-band`) were deleted
from the tree in the commit named under "Earlier directions" below, and the tool can still render them from that
history. No ADR is accepted by this choice: ADR-0160 to ADR-0172 stay proposed and describe the chosen direction.

`clear-field/` holds sixteen screens, each in the light set (`<screen>.png`) and the dark set (`<screen>-dark.png`) at
scale 1 (1024 × 768), and the desktop and the four file manager screens also at scale 2 (`desktop@2x.png`, `files@2x.png` and the others, 2048 × 1536; the
token `screens.2x` names the screens rendered at scale 2). Every image is rendered by `tools/mockups` from
`clear-field/tokens.txt` (the screens are in `clear.rs`, the shared drawing in `scene.rs`), with the tool's own
TrueType rasteriser and PNG encoder (no crate), so what is shown is what the tokens say: ADR-0161 proposes the same
token vocabulary for `zoraya-tokens`, and ADR-0162 the same coverage model (8-bit coverage, no hinting, glyph origins
on whole pixels; the tool computes it in `f32`, the system's crate will in fixed point). The mockups are
illustrations, not the implementation: the dock's icons (which do not follow ADR-0161's 16-pixel icon rule), the
thumbnails' sources and the dialogs' contents are drawn by hand.

| Screen | What it shows |
|---|---|
| `desktop` | Windows of two rooms: `project` (a terminal and the editor) and `agent-1`, an untrusted room with one program; the bar; the dock with the room chips. |
| `grants` | The editor's window with its grants in the title bar and, on hover, the full record in the trusted material, with the line that made it. One room has windows, so there is no field: the title bar's tint carries the room. |
| `drop` | A file dragged from `files` (the `home` room) onto the editor (`project`): the target's rim of light and the card with the grant's text. Two rooms have windows, so the project's field is drawn; `home` has none. |
| `chooser` | The chooser as a sheet in the trusted material under the editor's title bar (ADR-0167), with the grant's line. |
| `rooms` | The rooms overview (System+R): one card per room with its windows, its grants and `Enter`; a `New room` form with its `zosh` line. |
| `switcher` | Alt+Tab, grouped by room, with the windows' own pixels shrunk as thumbnails. |
| `settings` | The settings window, Appearance page, with only its own parameters and its `settings` line. |
| `terminal` | `zosh` in the project room: `room`, `grants`, a build, `room new ... --untrusted`, a refused path with its hint. |
| `commands` | The desktop in show-commands mode (System+?): a tag with the `zosh` line beside every gesture, and the dock grown with the line under every icon and chip. |
| `dock` | The desktop with the editor's icon hovered: its name and line in a tooltip over the dock. |
| `dock-compare` | The dock at rest, hovered and in show-commands mode, one under the other. |
| `trust` | A focused trusted window beside a focused untrusted one: the untrusted edge, the seam between the fields, and what else carries trust (below). |
| `files` | The file manager redesign (2026-11-10, `files.rs`), in the `project` room: the sidebar of places it holds with what each allows, the toolbar (back, forward, up, the path, search), the list sorted by name with one file selected, the status line with free space, its grants on the frame. |
| `files-select` | The list sorted by modified, newest first (the header clicked); a Shift range and one more row picked with Command; the keyboard's cursor ring. |
| `files-rename` | Rename in place (Enter or F2): a field over the name, the stem selected. |
| `files-delete` | Delete asks first: the application's own card on its paper (the glow is the compositor's alone), Delete in the error colour. |

To regenerate: `tools/mockups/fetch-fonts.sh` once (needs `curl`, `shasum`, `unzip` and `tar` on the host;
downloads Inter 4.1, JetBrains Mono 2.304 and DejaVu 2.37 into `build/fonts/`, checking their SHA-256), then
`cargo run --release -p zoraya-mockups -- --fonts build/fonts --out docs/research/desktop/mockups`. The fonts and
their licences are recorded in `../survey.md` (OFL 1.1 for Inter and JetBrains Mono, the Bitstream Vera licence for
DejaVu); none is committed, since the renderer and its tests do not need them. Rendering the twenty-six images takes
about three seconds on an M4 (each glowing surface blurs the pixels under it; the desktop at scale 2 over a million of
them).

## Clear boundaries

The architect's concept for stage 7, after mist-v2 was judged "a correct modern interface in general, without an idea":
the design grows out of Zoraya's architecture. The interface shows, for the first time, what other systems hide: who
has access to what. Four rules give the whole visual language, and ADR-0172 gives them their object, the *room*: a named
set of grants (ADR-0153's words) in which programs run, a job with a table, the same thing in `zosh` and on the
desktop, drawn by the compositor from the real grants and never from the application.

1. **Colour means a room.** Saturated colour is used for rooms only (`room.1` to `room.4`, `room.untrusted`). Everything
   else is neutral: a calm palette, typography as the main expressive means, windows as sheets of paper, no decoration
   without a function. Application icons are monochrome; the only hue in the dock is a room's (the dots under the
   running applications, the chips). The untrusted hue is reserved: no trusted room colour lies within
   `room.hue-distance` degrees of it (60; the tool's test refuses a file that breaks this, INV-0161-6), and the tool
   refuses a saturated colour outside the room, button and terminal tokens (INV-0161-5).
2. **Glow means trust.** One material, light through blurred glass: a blur of what is under the surface, a tint, a band
   of light from the top edge, a bright rim and a halo (`glow.alpha`, `glow.halo`, `glow.blur`). Only surfaces the
   compositor draws use it: the chooser's sheet, the handing out of grants (the hover card, the drop card), the
   switcher, the rooms overview, the system bar and dock, the dock's tooltip, the show-commands tags. An application
   cannot draw it (ADR-0160, ADR-0167; INV-0160-7, INV-0167-4, INV-0172-1).
3. **Everything has its text.** Every element and gesture has a form in `zosh`. The grants on a window are written in
   ADR-0153's syntax (`+w ~/project`, `+w notes.txt`), a drop shows what it gives in the same words
   (`+r ~/Документи/звіт-2026.txt edit`), the chooser's sheet shows the line its Open would be, the settings page shows
   its `settings` line, a chip's tooltip and the show-commands mode (System+?) give every gesture its line. Focusing
   a window has no `zosh` form (`fg` is the shell's foreground job, not window focus), so the switcher carries none.
4. **Calm by default.** Restraint is the ground; colour and light work because they are rare. The wallpaper is deep
   (four blobs, a fold, a vignette, `wallpaper = deep`) and still muted; time of day may change the wallpaper alone,
   as a setting, never the interface.

### The Field variant

The room's colour tints the window's ground (`room.style = ground`: the body at 0.10, the title bar at 0.18), the
glow is strong (`glow.alpha 150`, `glow.halo 110`), and the rooms are *fields* on the wallpaper
(`wallpaper = rooms`). A field is not a region of the screen: the compositor draws it around a room's windows
(ADR-0172 point 5), and the rules are these, each with an invariant a screendump can check:

- **Only between rooms.** A field is drawn only when two or more rooms have windows on screen (`home` counts). With one
  room there is no field and the title bar's tint carries the room, an untrusted window its edge too (`grants`,
  `chooser`, `terminal`, `settings` show this; INV-0172-6).
- **The shape.** A room's field is the union of its windows' rectangles each grown by `field.padding` (24), with
  corners of `radius.window` plus the padding, clipped to the work area between the bar and the dock's strip
  (`field.gap` from each and from the screen's edges). The strip is the dock's current height: in show-commands
  mode, where the dock grows, the fields are recomputed for the taller strip (`commands`), so no field lies under
  the dock in any mode. Every tinted pixel is within the padding of a window of that
  room, and none lies in the bar or in the dock's strip (INV-0172-7). The field's header (the room's name, the word
  `untrusted`, the room's grants) sits on a small neutral ground at the top of the field, above its top-most window.
- **The minimum gap.** Where two rooms' padded shapes come within `field.gap` (12) of each other, each field is cut
  back by the other's shape grown by the gap, so that two fields never share a pixel and a neutral seam of at least
  the gap runs between them (`trust` shows the seam; INV-0172-8).
- **Dragging and maximising.** The field is recomputed from the windows' rectangles at every commit of a move or a
  resize, so a window dragged out of its room's field takes its field with it (the shape splits or joins; there is no
  edge to cross), and after any move, resize or maximise the fields equal those computed from the new rectangles
  (INV-0172-9). A maximised window fills the work area and contributes no shape: its room shows on its title bar
  only, and the other rooms' fields are drawn under it as usual.

### The dock

The architect's dock B, in `clear.rs`: every application that has a window has an icon (the agent included), on a
tile of neutral material (light glass in the light theme, graphite in the dark one: a vertical gradient from
`bg.control` towards `glow` to `bg.control` towards `border` or `shadow`, a one-pixel inner light along the top, a
one-pixel inner shadow along the bottom, a `border` edge, `radius.control × 2` corners), with the glyph embossed (its
relief one pixel below it in `glow`, or in the tile's shadow in the dark theme). The settings icon is a gear. Under a
running application there is one dot per room it has windows in, in that room's colour (`home` grey): the terminal
shows two, `project` and a hidden window in `home`. An application running in an untrusted room is ringed in
`room.untrusted`. After a divider the rooms are chips: `home`, `project`, `agent-1 untrusted` (a dot in the room's
colour, the name and the word in `fg.text`) and `+` for a new room; the current room's chip is filled with its colour
at a low alpha and outlined in it. A press on a chip enters its room (`room NAME`); `+` opens the overview's form
(`room new`). The bar then has no room badge: it keeps the system menu at the left, the trusted mark in the middle
while a sheet or the show-commands mode is open (the emblem and the heading), and the keyboard layout and the clock
at the right. The room is shown in the dock and on every frame.

The icon cells are equal (the chips are as wide as their text) and the dock never changes its height or its cells'
widths for a hover: the hovered icon's name
and line stand in a small tooltip of the trusted material over the dock (`dock`). In show-commands mode every icon
and chip has its line under it (`term`, `files ~`, `edit FILE`, `agent TASK`, `settings`, `room home`, `room project`,
`room agent-1`, `room new`); a line may be wider than its cell, and only in that mode does the dock grow, by the label
zone (30 pixels) and the widened cells (`commands`, `dock-compare`). The dock's strip is reserved: fields end above
it. The compositor draws all of it from the panel's launch records and ADR-0172's room table (which application has
windows in which room, which room is current, which rooms exist); an application cannot add a dot, a ring or a chip.

### What carries trust between two windows

`trust` puts a focused window of `project` beside a focused window of `agent-1`, light and dark. The glow is not
what tells them apart: by rule 2 no window has it, trusted or not; the glow tells the compositor's surfaces from every
application's. Between the two windows the difference is the hue (teal against the reserved orange) on the title
bar, the body and the field, the word `untrusted` in the badge, in the field's header and in the chip, and the ring
around the agent's icon in the dock. At scale 1 in the light theme the two grounds are told apart at a glance; in the
dark theme the tints are close to each other (the body's tenth of orange and of teal on `#26282d` differ by a few
values) and the field and the word did the work. So that the window itself carries trust without reading, the
architect adopted (2026-11-03, the decisions of ADR-0161 and ADR-0172) the measure proposed here, reserved for
untrusted rooms: an edge of `room.untrusted.edge` pixels (2) in `room.untrusted` around every untrusted window, inside
its border and in addition to the ground tint, drawn by the compositor whether or not a field is drawn (one room on
screen, a maximised window) and never for a trusted room, so that no trusted room colour produces it (INV-0172-10).
It is a token, not a new material, so rule 2 holds. Every screen with an untrusted window shows it (`trust`,
`desktop`, `dock`, `commands`, and the agent's thumbnail in `switcher` and `rooms`). The tool's
`untrusted_windows_carry_the_edge` test renders its `trust` scene, the untrusted window alone and the same window
maximised, and reads the edge's pixels at mid-height: `room.untrusted` two deep on the untrusted window every time,
and no room colour on the trusted window's edge.

### Contrast

INV-0161-3's 4.5 to 1, checked by the tool (`cargo test -p zoraya-mockups -- --nocapture clear_field_text_contrast`
prints this table) for every text the concept adds, on the background the compositor composes for it, at the darkest
and the lightest pixel of the wallpaper (the worst of the two is shown), in both sets. Secondary text is used on a
tinted title bar only where it keeps the ratio there; otherwise the text colour is used (`Painter::contrasting`), as
the rows say. The light set's `fg.text.secondary` is `#5b5f67` for the dock's lines on the material. The badge's name and
the overview's and switcher's room names are in `fg.text`, since the room colours themselves do not all reach 4.5
as text on a title bar they tint (the second table, which the test prints without asserting it); the chips' word
`untrusted` is in `fg.text` for the same reason on the chip, and the tooltip's line is in `fg.text` because
`fg.text.secondary` falls to 3.1 under the tooltip's band of light in the dark set.

| Text on background | Light | Dark |
|---|---|---|
| field header name, `fg.text` on the header of `room.1` | 12.6 | 10.5 |
| field header grants, `fg.text.secondary` on the header of `room.1` | 4.9 | 4.8 |
| field header name, `fg.text` on the header of `room.2` | 12.5 | 10.6 |
| field header grants, `fg.text.secondary` on the header of `room.2` | 4.9 | 4.8 |
| field header name, `fg.text` on the header of `room.3` | 12.6 | 10.5 |
| field header grants, `fg.text.secondary` on the header of `room.3` | 4.9 | 4.8 |
| field header name, `fg.text` on the header of `room.4` | 12.6 | 10.5 |
| field header grants, `fg.text.secondary` on the header of `room.4` | 4.9 | 4.8 |
| field header name, `fg.text` on the header of `room.untrusted` | 12.6 | 10.5 |
| field header grants, `fg.text.secondary` on the header of `room.untrusted` | 4.9 | 4.8 |
| bar, `fg.text` on the material | 12.3 | 10.6 |
| show-commands name under an icon, `fg.text` on the material | 12.3 | 10.6 |
| show-commands line under an icon or chip, `fg.text.secondary` on the material | 4.8 | 4.9 |
| tooltip name and line, `fg.text` in the tooltip | 12.9 | 6.8 |
| chip of another room, `fg.text` on `bg.control` over the material | 14.3 | 9.3 |
| current chip, `fg.text` on `room.1` at 46 over the material | 10.3 | 6.6 |
| current chip, `fg.text` on `room.2` at 46 over the material | 10.0 | 6.8 |
| current chip, `fg.text` on `room.3` at 46 over the material | 10.3 | 6.4 |
| current chip, `fg.text` on `room.4` at 46 over the material | 10.0 | 6.7 |
| current chip, `fg.text` on `room.untrusted` at 46 over the material | 10.4 | 6.7 |
| title and badge on the focused title bar tinted by `room.1`, `fg.text` | 12.4 | 7.8 |
| grant pill on the focused title bar tinted by `room.1`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 11.0 | 6.5 |
| title and badge on the other title bar tinted by `room.1`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 4.6 | 8.3 |
| grant pill on the other title bar tinted by `room.1`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.4 | 7.0 |
| body text, `fg.text` on `bg.window` tinted by `room.1` | 13.7 | 10.1 |
| title and badge on the focused title bar tinted by `room.2`, `fg.text` | 12.1 | 8.1 |
| grant pill on the focused title bar tinted by `room.2`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.7 | 6.8 |
| title and badge on the other title bar tinted by `room.2`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 11.6 | 8.6 |
| grant pill on the other title bar tinted by `room.2`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.2 | 7.2 |
| body text, `fg.text` on `bg.window` tinted by `room.2` | 13.5 | 10.3 |
| title and badge on the focused title bar tinted by `room.3`, `fg.text` | 12.4 | 7.5 |
| grant pill on the focused title bar tinted by `room.3`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 11.0 | 6.3 |
| title and badge on the other title bar tinted by `room.3`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 4.6 | 8.0 |
| grant pill on the other title bar tinted by `room.3`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.4 | 6.8 |
| body text, `fg.text` on `bg.window` tinted by `room.3` | 13.7 | 10.0 |
| title and badge on the focused title bar tinted by `room.4`, `fg.text` | 12.1 | 7.9 |
| grant pill on the focused title bar tinted by `room.4`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.8 | 6.7 |
| title and badge on the other title bar tinted by `room.4`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 11.6 | 8.4 |
| grant pill on the other title bar tinted by `room.4`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.2 | 7.1 |
| body text, `fg.text` on `bg.window` tinted by `room.4` | 13.6 | 10.2 |
| title and badge on the focused title bar tinted by `room.untrusted`, `fg.text` | 12.5 | 7.9 |
| grant pill on the focused title bar tinted by `room.untrusted`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 11.0 | 6.6 |
| title and badge on the other title bar tinted by `room.untrusted`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 4.6 | 8.4 |
| grant pill on the other title bar tinted by `room.untrusted`, `fg.text.secondary` where it keeps 4.5, else `fg.text` | 10.6 | 7.1 |
| body text, `fg.text` on `bg.window` tinted by `room.untrusted` | 13.8 | 10.2 |

| Not used: a room's colour as text on its tinted title bar | Light | Dark |
|---|---|---|
| `room.1` on the focused title bar | 3.5 | 4.0 |
| `room.1` on the other title bar | 3.3 | 4.2 |
| `room.2` on the focused title bar | 4.3 | 3.5 |
| `room.2` on the other title bar | 4.1 | 3.7 |
| `room.3` on the focused title bar | 3.6 | 4.4 |
| `room.3` on the other title bar | 3.4 | 4.7 |
| `room.4` on the focused title bar | 4.0 | 3.8 |
| `room.4` on the other title bar | 3.8 | 4.0 |
| `room.untrusted` on the focused title bar | 3.1 | 4.0 |
| `room.untrusted` on the other title bar | 3.0 | 4.2 |

### Cost

The blur under the system surfaces is of what is really under them: the tool blurs the pixels it has drawn (windows
included) when it draws a glowing surface, as the compositor would when the surface opens (ADR-0160). What the compositor
would pay, measured in the tool on the architect's M4 (`cargo test -p zoraya-mockups --release -- --ignored --nocapture
glow_cost`, one thread, release build, 2026-11-03), for the switcher's surface of 640 × 372 logical pixels:

| Scale | Pixels blurred (surface plus margin) | Blur | Halo | Inner light |
|---|---|---|---|---|
| 1 | 364 thousand | 11.5 ms | 2.0 ms | 0.3 ms |
| 2 | 1.46 million | 40 ms | 9 ms | 1.2 ms |

The blur is paid when the surface opens and when something under it commits, not per frame; the halo and the rim are
cached per size (ADR-0161 records the estimate, with the once-per-change wallpaper blur: 32 ms at scale 1, 185 ms at
scale 2). A field is one pass over its bounding box with a signed distance per pixel, repeated when a window of the
room moves; it is drawn into the wallpaper's copy, not per frame.

### Tokens behind the direction

`clear-field/tokens.txt`: Inter and JetBrains Mono; `text.body 13`, `text.title 20`; `radius.window 10`,
`radius.shape smooth`, `frame.height 36`, `padding.window 20`, `padding.terminal 14`, `shadow.window.blur 22` in two
layers; `bar.height 28`, `dock.icon 40`, `dock.radius 16`, `dock.padding 10`, `blur.panel 36`, `panel.alpha 190`;
`room.style ground`, `room.untrusted.edge 2`, `glow.alpha 150`, `glow.halo 110`, `glow.blur 30`;
`field.padding 24`, `field.gap 12`, `room.hue-distance 60`; `screens.2x desktop`; light `wallpaper.a #b9c6d8`,
`wallpaper.b #e4dcd6`,
`wallpaper.c #c9d9e3`, `wallpaper.d #d5c9cf`, `bg.window #f7f7f8`, `accent #3b4f6b` (a neutral blue-grey: controls,
not rooms), `fg.text.secondary #5b5f67`, `room.1 #2f7f8f`, `room.2 #6b58a8`, `room.3 #4e7f36`, `room.4 #9a4f8a`,
`room.untrusted #c2641c`, `glow #ffffff`; dark `wallpaper.a #161c28`, `bg.window #26282d`, `accent #8ea3c2`,
`fg.text.on-accent #141a24`, `room.1 #5fb3c4`, `room.2 #a08ee0`, `room.3 #8fc06a`, `room.4 #d48ac4`,
`room.untrusted #e8924a`, `glow #e8ecf2`.

## Earlier directions

`mist` (light, close to macOS), `quiet` (minimal, a classic BSD feel), `night` (dark), `mist-v2` (mist after the
architect's breakdown: the top bar and the dock, no coloured border, the procedural wallpaper, two-layer shadows,
continuous corners) and the two other variants of the concept, `clear-line` (the room as a thin edge, a quiet glow)
and `clear-band` (the room as a tinted title bar, a clear glow), are not in the tree. They were deleted in the commit
named below; the tool keeps the classic screens (`scene.rs`, `concept = classic`), so the four classic directions
render again as they were from that commit's parent. `clear-line` and `clear-band` render too, but with the code of
this commit (dock B, the fields around windows, the neutral header), not as the architect saw them; for that, build
the tool from the same parent. A checked-out second direction makes the tool's test of the direction count fail until
it is removed again:

```
git checkout edf4b4b^ -- docs/research/desktop/mockups/<name>
cargo run --release -p zoraya-mockups -- --fonts build/fonts --out docs/research/desktop/mockups --direction <name>
```

The deletion commit is `edf4b4b` on the branch `adr/stage-7-design` ("keep only clear-field"); the files are in its
parent, `edf4b4b^`. The branch will be squash-merged into `main`, so afterwards they exist only in that branch's
history on `origin`, not in `main`'s.

## What the tool checks

`cargo test -p zoraya-mockups` runs without fonts or network: the rasteriser's coverage (a square, a half-pixel
edge, out-of-range segments), the PNG encoder (deflate round trip through a small inflate, chunk CRCs, known CRC-32
and Adler-32 values), the TrueType reader on a synthetic one-glyph font (metrics, `cmap`, the outline, every
truncation and 2,000 random mutations without a panic, the triangle's rasterised area), the token parser, the
continuous corner (inside its rectangle, touching every edge, between a circle of the radius and one of its span),
the image blur (a flat image unchanged, deterministic), the wallpaper (deterministic, within its colours), and the
direction file: the required token names in both sets, a text contrast of at least 4.5 to 1 on each background
(ADR-0161's INV-0161-2 and INV-0161-3 applied to the mockups), the rule "colour means a room" (no saturated colour
outside the room, button and terminal tokens, INV-0161-5), the reserved untrusted hue (INV-0161-6), the untrusted edge
on every untrusted window and on no trusted one (INV-0172-10, over the tool's own render without fonts), every room's
grants as `+r` or `+w` words, and the contrast table above over the composed backgrounds. `cargo test -p
zoraya-mockups --release -- --ignored --nocapture blur_cost` prints the time of one blur at both scales, the
measurement ADR-0161 cites, and `glow_cost` the same for the trusted material.
