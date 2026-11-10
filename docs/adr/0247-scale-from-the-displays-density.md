# ADR-0247: The scale from the display's density, read from its EDID

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0090, ADR-0093, ADR-0164, ADR-0169, ADR-0246
- **Evidence:** desktop/survey.md#display-scaling

## Context
The architect tried the desktop on a Retina Mac (2026-11-10): the text is too small. ADR-0164's `auto` gives scale 2
from 1920 pixels wide, a rule for monitors of about 100 pixels per inch. QEMU's cocoa window, read in QEMU 11.1.2's
source (`ui/cocoa.m`, `updateUIInfoLocked`), gives the guest its size in pixels, the frame in points times the
window's backing scale: on a Retina screen a window 900 points wide is 1800 pixels, `auto` gives scale 1, and every
glyph is half the size it is on a monitor. This also answers ADR-0246's first open question (pixels, not points).

The same function puts the window's physical size in the console's `width_mm` and `height_mm`, from the screen's
size (`CGDisplayScreenSize`) in proportion to the window. virtio-gpu passes them on in the EDID it generates
(`hw/display/virtio-gpu-base.c`, `virtio_gpu_base_generate_edid`; `hw/display/edid-generate.c` writes them in the first
detailed timing descriptor and, in centimetres, in the base block), which a driver reads with `GET_EDID` once it has
negotiated `VIRTIO_GPU_F_EDID` (virtio 1.2, 5.7.3). Without a window (`-display none`, every test) the size is 0 and
QEMU writes the size of a 100 ppi display, so tests keep ADR-0164's rule. ADR-0090's INV-0090-1 lets `gpud`
negotiate `VERSION_1` and no other feature.

Measured: nothing on a Mac yet; host tests with a fake device and a fake driver. The architect also asked that
`make run-gui` on macOS boot at 2048 by 1536 with scale 2 (`GUI_SCALE=1` stays), which is a Makefile default, not
part of this decision.

## Decision drivers
1. Text of the same physical size on a Retina window as on a monitor (ADR-0164's aim, the architect's check).
2. Nothing the host tells the guest outside the devices it already has; no new device, no kernel change.
3. Tests unchanged: without a window the rule is ADR-0164's.
4. `gui.scale` 1 or 2 still fixes the scale (ADR-0169).

## Considered options

### Option A: `F_EDID` in `gpud`, the physical size in `InfoReply`, `auto` counts the density (built)
- **Description:** `gpud` negotiates `VIRTIO_GPU_F_EDID` when offered and, after each `GET_DISPLAY_INFO`, asks
  `GET_EDID` for scanout 0 and keeps the physical size its first detailed timing (else its base block) states; a
  refused `GET_EDID` leaves it unknown. `InfoReply` gains `width_mm` and `height_mm` (0: unknown). `auto` gives scale
  2 from 1920 pixels wide, or from 1280 (640 logical pixels) on a display of at least 150 pixels per inch.
- **Evidence for:** Drivers 1 to 4; the density is what QEMU computes for the window, on the screen it is on.
- **Evidence against:** One more feature and command in a driver that treats the device as hostile (the EDID is
  parsed into two numbers, bounds-checked by the format: at most 4095 mm a side); `InfoReply` changes layout (the
  compositor and `gpud` are built together).
- **Cost:** Built: about 120 lines with tests.
- **Precedent:** Linux's DRM reads the EDID's physical size, and desktops pick their scale from it (`desktop/survey.md#display-scaling`; GNOME's
  automatic scale from 192 dpi for 2).

### Option B: The host tells the scale (a QEMU option, a boot argument)
- **Description:** `tools/run-qemu.sh` detects a Retina screen and passes the scale on the kernel's command line or
  through `fw_cfg`.
- **Evidence against:** A new channel from the host into `init` and the compositor (driver 2); wrong as soon as the
  window moves to another screen.
- **Cost:** *(estimate)* 200 lines across the kernel, `init` and the compositor.
- **Precedent:** Xorg's `-dpi`.

### Option C: Keep ADR-0164's rule; `make run-gui` boots at scale 2 on macOS
- **Description:** Only the Makefile's default changes; a window made smaller than 1920 pixels drops to scale 1.
- **Evidence against:** Driver 1 fails after any resize below 1920 pixels.
- **Cost:** One line (done anyway).

### Option D: What Linux does (the person sets the scale)
- **Description:** `gui.scale = 2` by hand.
- **Evidence against:** Works, but `auto` is then wrong on every Retina window, the common case on this host.

## Recommendation
Recommended: **Option A**, as built. It is a recommendation, not a decision. It changes ADR-0090's INV-0090-1 from
"`VERSION_1` and no other feature" to "`VERSION_1` and `F_EDID`, no other", and ADR-0164's `auto` rule by the dense
case; nothing else of either.

## Consequences
- **Positive.** A Retina window at any size of 1280 pixels or more gets scale 2; a monitor and every test keep scale 1
  below 1920.
- **Negative.** A window between 1280 and 1919 pixels on a Retina screen shows 640 to 959 logical pixels: windows are
  made smaller to fit (ADR-0246).
- **Follow-up.** None.

## Invariants
- **INV-0247-1.** `gpud` negotiates `VERSION_1` and `VIRTIO_GPU_F_EDID` and no other feature, whatever the device
  offers. *Check:* host test of `zoraya-virtio-gpu` with a fake device that offers every bit.
- **INV-0247-2.** Under `auto` the scale is 2 exactly when the display is at least 1920 pixels wide, or at least 1280
  pixels wide with a width in millimetres that gives 150 pixels per inch or more; an unknown width in millimetres
  counts as no density. *Check:* host tests of the compositor's `scale_for` and of a resize with a fake driver.

## Open questions
- The threshold of 150 pixels per inch and the 1280 pixels: chosen, not measured on a person's eyes; to be seen by
  hand with `make run-gui`.

- A change of density alone (the window moved to a screen of another density at the same pixel size) is not
  followed: `gpud` tells a new size only when the pixels change, and the compositor takes the physical size with it
  (review of the fix branch, 9).

## Revisit triggers
- A second display, or a display whose EDID lies about its size.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request.

Option A (INV-0090-1's change included).
