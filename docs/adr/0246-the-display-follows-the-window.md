# ADR-0246: The display follows the window

- **Status:** accepted
- **Date:** 2026-11-09
- **Depends on:** ADR-0090, ADR-0093, ADR-0097, ADR-0160, ADR-0164, ADR-0169
- **Evidence:** desktop/survey.md#display-scaling

## Context
This ADR proposes to supersede one point of ADR-0164, the display's size fixed at boot; its scale, protocol and
invariants stand.

ADR-0164 and ADR-0097 fix the display's size at boot: `tools/run-qemu.sh` asks virtio-gpu for 1024 by 768 (or 2048 by
1536 with `--gui-scale 2`), `gpud` reads the size once (`GET_DISPLAY_INFO`), and the compositor attaches one
framebuffer of that size. Since `tools/run-qemu.sh` gives the macOS window `zoom-to-fit=on` (TASK-24), the person can
resize QEMU's window. What happens then was read in QEMU's source, not measured: with zoom-to-fit the cocoa window is
resizable, and each `windowDidResize:` calls `updateUIInfo`, which hands the window's size in pixels (the frame times
the backing scale) to the console with `qemu_console_set_ui_info`. virtio-gpu turns that into its display event
(`VIRTIO_GPU_EVENT_DISPLAY` in the configuration's `events_read`, with a configuration-change interrupt), after which
`GET_DISPLAY_INFO` gives the new size. Today `gpud` never looks: the guest keeps its boot size and QEMU scales it to
the window.

The person asked (TASK-24, 7g) for the display to follow the window: `gpud` handles the event and recreates the
scanout, the compositor lays out again (bar and dock follow, windows stay on screen, terminals hear `Configure`) and
picks scale 1 or 2 from the width, which `gui.scale` (ADR-0169) can fix.

Measured: nothing yet. QEMU's QMP has no command that changes a console's size (`qapi/ui.json`: `display-update` and
`display-reload` concern VNC only), so no automated test can resize the window; tests can boot at different sizes and
can hand the compositor a size change through a fake driver.

## Decision drivers
1. ADR-0164's driver 1: nothing is ever bitmap-stretched. A guest scaled by the host to the window breaks it whenever
   the window is not the boot size.
2. One code path for a new size: the compositor already re-attaches a restarted driver at whatever size it reports
   (`attach_display`: the scale, the wallpaper, every window's `Configure`).
3. The driver treats every message as hostile and the compositor treats the driver as hostile (ADR-0090, ADR-0093):
   a size change is one more message to check, not a new trust relation.
4. Small, and testable without a resizable QEMU.

## Considered options

### Option A: Fixed at boot (ADR-0164 as it is)
- **Description:** Keep the boot size; QEMU scales the picture to the window.
- **Evidence for:** Nothing to build.
- **Evidence against:** Driver 1: the picture is stretched and blurred at every window size but one.
- **Cost:** None.
- **Precedent:** Firmware framebuffers (a mode set once).

### Option B: Restart the driver on a size change
- **Description:** `gpud` exits when the device reports a display event; the manager restarts it (ADR-0093) and the
  compositor attaches the new driver at the new size through the path it has.
- **Evidence for:** Driver 2 with no new message.
- **Evidence against:** A device reset and a new driver for each step of a window drag (cocoa reports every step);
  the hang counter and the restart's back-off (ADR-0093) count a person's resize as failures.
- **Cost:** *(estimate)* 20 lines in `gpud`.
- **Precedent:** None that does this on purpose.

### Option C: The driver reports the new size; the compositor attaches a new framebuffer (Linux DRM's hotplug)
- **Description:** Between requests, every 100 ms (one register read; the configuration interrupt is not used),
  `gpud` reads `events_read`, clears the display event, asks `GET_DISPLAY_INFO` and, when the size differs from the
  attached one, sends the compositor an event `Resized { width, height }` (a new ordinal of ADR-0090's
  protocol, from driver to compositor, the first message the driver sends unasked). The compositor answers with `Info`
  and a new `Attach` of a framebuffer of that size; `gpud` makes a second resource, backs it, points scanout 0 at it
  and unreferences the old one, so the old framebuffer stays shown until the new one is complete. The compositor's
  attach path does the rest: scale from `gui.scale` or the width, wallpaper, bar and dock, every toplevel kept at its
  logical size as far as the work area holds it and moved back into it, legacy windows moved back, popups closed
  (sheets too at a new scale), and a `Configure` to every toplevel. A burst of events is coalesced: the compositor acts
  on the latest size once its in-flight flush is answered. The compositor's `Info` and `Attach` take the next message
  on the channel as their reply, so the driver sends one `Resized` before each `Info` and none between an `Info` and
  the request after it, and its `Attach` uses the size that `Info` gave; a size back at the attached one is not
  attached again (the whole-screen flush that follows ends the driver's wait).
- **Evidence for:** Drivers 1 to 4. It is what Linux's virtio-gpu DRM driver does (a hotplug event, then the
  compositor reads the new mode and makes new framebuffers); Windows broadcasts `WM_DISPLAYCHANGE` and every top-level
  window lays out again (desktop/survey.md#display-scaling records the per-monitor relayout).
- **Evidence against:** The display protocol gains its first unsolicited message, and `Attach` may be repeated.
- **Cost:** *(estimate)* 150 lines in `gpud` and `zoraya-virtio-gpu`, 100 in the compositor, one message.
- **Precedent:** Linux DRM with Wayland compositors; Plan 9's `rio`, where a resized window's program re-reads its
  window and redraws.

### Option D: The compositor polls `Info`
- **Description:** The compositor asks the driver for the size every second and re-attaches when it changed.
- **Evidence for:** No unsolicited message.
- **Evidence against:** A second of a wrongly sized picture after each change, and a request a second for nothing.
- **Cost:** *(estimate)* As C, less the event.
- **Precedent:** None in a current desktop.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Scale.** At each new size the compositor picks the scale as at attach (ADR-0164): `gui.scale` 1 or 2 fixes it,
  `auto` gives 2 from `SCALE_TWO_FROM` (1920) pixels wide.
- **Sizes.** Any size the driver accepts, 1 to 4096 pixels a side (ADR-0090's limit); a window larger than the new
  work area is made smaller, and every window's frame is moved back into it.
- **The settings window** shows `gui.scale` (`auto`, `1`, `2`); a change applies at once, by the same re-layout.
- **Tests.** `make test-gui` boots at two more sizes (1600 by 1000, scale 1; 1920 by 1200, scale 2 under `auto`) beside
  1024 by 768 and 2048 by 1536.
  A host test of the compositor changes a fake display's size and checks the scale, the framebuffer, windows in
  the work area and the `Configure`s. A host test of `zoraya-virtio-gpu` feeds the display event and checks the re-created
  scanout. No GUI test resizes a running guest (no QMP command does).

**Tensions with accepted ADRs.** *ADR-0164*'s "The scale ... from the display size" holds at each new size, and its
`--gui-scale` stays; this supersedes only the size being fixed at boot. *ADR-0093*: `Attach` "attached already" now
means "at this size": a second `Attach` after `Resized` is allowed. *ADR-0097* fixes 1024 by 768 for the test runs;
that stays the first size.

## Consequences
- **Positive.** The picture is never scaled by the host; a large window gives a larger desktop or, past 1920 pixels,
  scale 2.
- **Negative.** Every window is configured again at each size step of a drag; clients that are slow to redraw lag.
- **Follow-up.** The configuration-change interrupt in `gpud`; `Resized`; the re-layout; the boot sizes in `make
  test-gui`.

## Invariants
- **INV-0246-1.** After a display size change the compositor composes only into a framebuffer of the new size, and
  every toplevel's frame lies in the work area when the work area holds at least a border, a title bar and one body
  row, and every legacy window's frame lies in it when it fits there (a legacy window is never resized; one larger
  than the work area keeps its top-left corner in it). *Check:* host test of the compositor with a fake display.
- **INV-0246-2.** `gpud` sends `Resized` only once something is attached, only for a size (1 to 4096 a side) that
  differs from the attached one, one before each `Info`, and none between an `Info` and the request after it.
  *Check:* host test of `zoraya-virtio-gpu`'s `Announce`, which `gpud` asks before each `Resized`.

## Open questions
- Whether cocoa reports the window size in pixels or points on a Retina display (the source multiplies by the backing
  scale): to be seen by hand with `make run-gui`.
- Whether a drag's many events need a delay before the re-attach, or coalescing behind the flush is enough.

## Revisit triggers
- A second display (ADR-0164's trigger).
- A QEMU that can resize a console from QMP: a GUI test of the resize itself.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request.

Option C. Its two open questions wait for the architect's check by hand.
