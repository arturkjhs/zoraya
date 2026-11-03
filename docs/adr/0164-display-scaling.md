# ADR-0164: Display scaling

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0051, ADR-0090, ADR-0093, ADR-0097, ADR-0160, ADR-0161, ADR-0162, ADR-0169
- **Evidence:** CP-21, DM-083, desktop/survey.md#display-scaling, desktop/survey.md#hinting-and-antialiasing

## Context
The architect works on a Mac whose display is of high density `[assumption: a Retina display, on which QEMU's 1024 by 768 window and stage 5's 12 by 20 pixel cells are small; not measured]`. Stage 7 needs a scale factor so that text and controls are legible on dense displays, and a rule for how clients render for it. The survey (`desktop/survey.md#display-scaling`) records the three models: Wayland's integer buffer scale ("you have to attach a buffer that is larger (by a factor of scale in each dimension)"), its later fractional protocol (a numerator over 120 and a viewport), macOS's points with four times the pixels and "most apps won't need to be aware", and Windows' history of bitmap-stretched, blurry, DPI-unaware applications and the per-monitor awareness that made every application relayout on `WM_DPICHANGED`. Hinting matters less at high density (`desktop/survey.md#hinting-and-antialiasing`). Measured: nothing on Zoraya; the virtio-gpu display is whatever `tools/run-qemu.sh` asks for (ADR-0097: 1024 by 768). Assumed: QEMU can be asked for a 2048 by 1536 display and shows it in a window the Mac scales as it scales any window.

## Decision drivers
1. Nothing is ever bitmap-stretched: every pixel on screen was drawn at that density (Windows' lesson, `desktop/survey.md#display-scaling`).
2. One number, known to every client before it draws, and the same tokens (ADR-0161) at every density (CP-21: no implicit global state).
3. The screendump tests keep their exactness at every scale (DM-083's "every frame is perfect" applied to tests).
4. Small: no viewport protocol, no per-window awareness modes.

## Considered options

### Option A: Awareness modes per application, stretching for the rest (Windows)
- **Description:** A client declares whether it understands density; the compositor stretches the buffers of those that do not.
- **Evidence for:** Old clients keep working at the new density.
- **Evidence against:** Driver 1: the stretched ones are blurry, and `desktop/survey.md#display-scaling` records a decade of migration; Zoraya has no old clients to protect.
- **Cost:** *(estimate)* A scaler in the compositor and two code paths in every toolkit.
- **Precedent:** Windows.

### Option B: An integer scale factor for the whole display; clients render at that factor (Wayland core, macOS)
- **Description:** The compositor holds one scale, 1 or 2, chosen from a setting (ADR-0169) or from the display size; it tells every client in `Display` and in every `Configure` (ADR-0160); logical sizes (tokens, window sizes, pointer coordinates) are in logical pixels and a client's buffer is `scale` times larger in each dimension; text is rasterised at `size × scale` (ADR-0162). This is Wayland's `set_buffer_scale` and macOS's points (`desktop/survey.md#display-scaling`).
- **Evidence for:** Drivers 1 to 4. One multiplication in each client; the compositor copies buffers as it does today; a screendump at scale 2 is a function of the same scene at scale 1.
- **Evidence against:** No 1.5: on a display where 2 is too large and 1 too small, the person chooses one of them; a scale change at run time means every client re-creates its surfaces.
- **Cost:** *(estimate)* 100 lines in the compositor, a field in two messages, one multiplication in each client's layout.
- **Precedent:** Wayland core, macOS (2x).

### Option C: Fractional scale with a viewport (Wayland's `fractional-scale-v1`)
- **Description:** The scale is a fraction in 120ths; a client renders at that fraction into a larger buffer and the compositor maps it through a viewport with rounding rules (`desktop/survey.md#display-scaling`).
- **Evidence for:** 1.25 and 1.5 for displays between the two densities.
- **Evidence against:** Fractional glyph sizes and control sizes break pixel alignment (icons on a 16-pixel grid land on half pixels), rounding rules for sub-surfaces are left undefined by the protocol itself, and the compositor needs a viewport; driver 3's exactness goes with the fractional positions.
- **Cost:** *(estimate)* Option B plus a viewport and rounding rules everywhere.
- **Precedent:** wayland-protocols staging; GNOME and KDE.

### Option D: The compositor scales everything (one logical display, resampled)
- **Description:** Clients always render at scale 1; the compositor resamples the whole composition to the display.
- **Evidence for:** Clients know nothing of scale.
- **Evidence against:** Driver 1 outright: everything is blurry at 2.
- **Cost:** *(estimate)* A resampler.
- **Precedent:** Low-resolution modes on any system.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **The scale** is an integer, 1 or 2 in stage 7 (3 and 4 would need nothing new), from the setting `gui.scale` (ADR-0169); `auto` picks 2 when the display is at least 1920 pixels wide `[assumption]`. It is one value for the one display (ADR-0090).
- **Protocol.** `Display` (ADR-0093) replies width and height in logical pixels and the scale; ADR-0160's `Configure` carries the scale in its `scale` field (defined there, not here); a client creates its memory object at `width × scale` by `height × scale` pixels (ADR-0051) and commits damage in buffer pixels. Pointer coordinates to clients are logical; the compositor divides.
- **Change at run time.** When the setting changes, the compositor sends every window a `Configure` with the new scale and the same logical size; the client re-creates its surface (a new `CreateToplevel` with a new object; the compositor keeps the old pixels, scaled by whole-pixel replication only as a transient `[assumption: acceptable for the seconds a re-create takes]`, until the first commit at the new scale). The switch is rare; the transient is the one place pixels are replicated and it never appears in a screendump test, which sets the scale before boot.
- **Tokens and text.** Every token is logical (ADR-0161); the toolkit (ADR-0165) multiplies once when it draws; text is rasterised at the physical size, never scaled (ADR-0162); icons are rasterised from paths at 16 or 32 logical pixels times the scale, so their strokes stay on the physical grid.
- **QEMU.** `tools/run-qemu.sh --gui` keeps 1024 by 768; `--gui-scale 2` asks for 2048 by 1536 (`xres` and `yres` of `virtio-gpu-device`); `make run-gui` passes the architect's choice; `make test-gui` runs the criterion script at both scales, reading the screen back with ADR-0162's reader at the matching size.

**Tensions with accepted ADRs.** *ADR-0093* "width and height" of `Display` and the surface's size are physical today; they become logical plus a scale, a new ordinal for `Display` (ADR-0052); INV-0093-3's "a surface larger than the display" is then checked in buffer pixels against the display's buffer size, and its test gains the scale-2 case. *ADR-0090* has width and height "from 1 to 4096", which 2048 by 1536 satisfies. *ADR-0097* fixes 1024 by 768; this adds a second size for the same script.

### Effect of the vision
None on the choice. For target 2 the scale is a number a client receives; nothing to grant. Target 3 wants a second display with its own scale, which is a scale per output in `Configure` and clients re-creating surfaces when a window moves between them, at Option B's run-time-change cost per move; fractional scaling stays available as Option C later, since logical coordinates are already separate from buffer pixels. Networking and threads: none.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** Crisp at 1 and 2 with one multiplication; the tests run at both.
- **Negative.** No fractional scale; a scale change re-creates every surface.
- **Follow-up.** The setting; the QEMU option; the second GUI test run.

## Invariants
- **INV-0164-1.** The compositor never resamples a client's buffer: every client pixel reaches the display unchanged (replication during a scale change excepted, and only until the client's next commit). *Check:* host test composing a client buffer with a known pattern at scale 2 and reading it back from the composition; review of the copy path.
- **INV-0164-2.** At scale 2 every window frame, control and glyph is drawn with twice the logical dimensions and at the physical size, so that the GUI screendump at scale 2 shows the terminal's cells at twice their scale-1 size and reads back as the same text. *Check:* `make test-gui` at both scales with the same script.
- **INV-0164-3.** A client learns the scale before it creates any surface, and every size the compositor sends is logical. *Check:* host test of the protocol flow asserting `Display` carries the scale and that a surface created at the wrong physical size is refused with `BadBuffer`.

## Open questions
- Whether QEMU's display under HVF on the Mac shows a 2048 by 1536 guest at a usable size: to be tried by the first stage 7 task.
- The `auto` threshold.

## Revisit triggers
- A display for which neither 1 nor 2 is right: Option C.
- A second display.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
