# ADR-0322: A wheel event in the compositor protocol: one message in 120ths of a notch, to the body under the pointer

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0052, ADR-0092, ADR-0093, ADR-0094, ADR-0160, ADR-0164, ADR-0245, ADR-0248, ADR-0317
- **Evidence:** CP-24, desktop/survey.md#focus-and-stacking

## Context
The compositor protocol (`zoraya-proto-gui`) has keys, pointer positions with buttons, focus and window events, and
no wheel. ADR-0317's architect's decision removed "wheel to mouse 4 and 5" from NetSurf's surface for that reason and
named a wheel event as a follow-up (`docs/backlog.md`). Read in this session: the input protocol from `inputd` to the
compositor (`zoraya-proto-input`, ADR-0092) has two record kinds, keys and absolute axes, and `inputd`'s driver
(`zoraya-virtio-input`) drops and counts every `EV_REL` event; QEMU's `virtio-tablet` device, which `tools/run-qemu.sh`
gives the guest, reports a wheel as `EV_REL`/`REL_WHEEL` events of plus or minus one per notch, which QMP's
`input-send-event` makes from the buttons `wheel-up` and `wheel-down` (measured in this part by `make test-netsurf`'s
`wheel` step, see the task report). NetSurf's framebuffer front end (`frontends/framebuffer/gui.c` in the unpacked 3.11
tree) scrolls the page or frame under the pointer by 100 pixels on a press of `NSFB_KEY_MOUSE_4` (up) or
`NSFB_KEY_MOUSE_5` (down) and has no horizontal button. The terminal has 500 lines of scrollback reached with Shift and
Page Up. Not in `docs/research/`: the conventions of other systems for a wheel's unit (Linux's `REL_WHEEL_HI_RES` and
Wayland's `axis_value120` count 120 a notch, as Windows' `WHEEL_DELTA` does; X11's core protocol reports buttons 4 to 7);
they are stated here from those systems' documentation and are labelled as such.

## Decision drivers
1. Pointer events go to the window under the pointer, keys to the focused one; a press focuses (ADR-0094,
   `desktop/survey.md#focus-and-stacking`, CP-24). A wheel is a pointer event and must not move focus.
2. A changed message is a new ordinal (ADR-0052); every message decodes totally and bounded.
3. Nothing a client receives carries pixels or input of another client (INV-0093-2); the modifiers go only to the
   focused client (ADR-0248, review 7h).
4. A high-resolution wheel or touchpad must not be rounded to notches on the way, and a notch device must not need a
   second message.
5. The least change to the C surface and to NetSurf: no patch of NetSurf's own code.

## Considered options

### Option A: The Linux/Wayland way, several axis events in a frame
- **Description:** `wl_pointer.axis` (a fixed-point value per axis), `axis_discrete` or `axis_value120`, `axis_source`
  (wheel, finger, continuous) and `axis_stop`, grouped by `wl_pointer.frame`.
- **Evidence for:** Carries kinetic scrolling and the source; what toolkits on Linux read.
- **Evidence against:** Five messages and a frame for what one carries here; the frame grouping is a state machine in
  every client; no device of Zoraya's has a finger source.
- **Cost:** *(estimate)* five ordinals, a frame state in `zoraya-surface` and `zoraya-app`.
- **Precedent:** Wayland (`axis_value120` since `wl_pointer` version 8).

### Option B: Buttons 4 and 5, as X11's core protocol
- **Description:** A notch is a press and a release of a fourth or fifth button in `WindowPointerMods`.
- **Evidence for:** No new message; NetSurf's front end reads exactly this.
- **Evidence against:** `buttons` is `Flags<7>`: two more bits change the layout of an existing ordinal (ADR-0052)
  and every client's button logic sees presses that are not presses (drag serials, ADR-0170, count left presses only,
  but a client's own click counting would see them); no unit below a notch.
- **Cost:** *(estimate)* small in the compositor, a new pointer ordinal anyway.
- **Precedent:** X11 core; NetSurf's SDL and X front ends.

### Option C: One message, `Wheel`, in 120ths of a notch, to the body under the pointer
- **Description:** A new client event `Wheel = 46` `{dropped, window, x, y, dx, dy, modifiers}`: `(x, y)` the pointer in
  logical pixels of the body, `dx` and `dy` `i32` in 120ths of a notch (`WHEEL_STEP`), `dy` above 0 toward the person
  (the view moves down), `dx` above 0 to the right, each at most `WHEEL_MAX` (64 notches) either way. The input
  protocol gains a third record kind `WHEEL` (code 0 vertical, 1 horizontal, the value an `i32` of at most 16 notches
  either way, not 0), which `inputd` makes from `REL_WHEEL`/`REL_HWHEEL` times 120 or from their `HI_RES` axes when the
  device declares them (then the notches of that axis are skipped, so a turn is not taken twice). The compositor sums
  a frame's records and sends one `Wheel` after the frame's pointer event.
- **Evidence for:** Drivers 1 to 5: one message, signed and bounded, decodable totally; the unit carries parts of a
  notch; the C surface turns whole notches into NetSurf's button presses itself.
- **Evidence against:** No source (wheel or finger) and no kinetic stop; `zoraya-protocol` gains `i32` as a field type.
- **Cost:** *(estimate)* 300 lines with tests across the input driver, two protocols, the compositor, the surface,
  the terminal and the C surface.
- **Precedent:** Windows' `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` (`WHEEL_DELTA` 120); Haiku's `B_MOUSE_WHEEL_CHANGED` (one
  message with both deltas).

### Option D: Option C delivered to the focused client
- **Description:** As C, but the turn goes to the focused window wherever the pointer is.
- **Evidence for:** What `docs/backlog.md`'s item said; simpler (no hit test).
- **Evidence against:** Every desktop of the survey scrolls what is under the pointer; a person scrolling a window
  beside the one they type in would scroll the wrong one; it contradicts driver 1.
- **Cost:** none beyond C.
- **Precedent:** Windows before 10 (focused window); changed in Windows 10 to the window under the pointer.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. As built in part 9e:

1. **Units and sign.** 120 a notch, `dy` above 0 toward the person, `dx` above 0 to the right; `inputd` turns evdev's
   sign of `REL_WHEEL` (positive away from the person) around. A frame's sum is clamped to `WHEEL_MAX` (7680).
2. **Delivery.** The rules of `WindowPointerMods`: to the body under the pointer (a toplevel or popup; a legacy
   surface gets none, its protocol has no window id); nothing over a frame, the bar, the dock or the wallpaper; nothing
   during a move or resize, an entry's drag (ADR-0170) or the switcher; nothing to a window that has a sheet over it,
   nor to a sheet in its first 500 ms (ADR-0245's guard); focus does not move; the modifiers only to the focused client.
3. **Clients.** `zoraya-surface` gives `Event::Wheel { x, y, dx, dy }` in buffer pixels, after a `Move` there when the
   pointer was elsewhere; `zoraya_window_poll` gives it as kind 8 (`ZW_WHEEL`, `c` and `d` the turns as `int32_t`);
   NetSurf's surface gathers 120ths and sends a press and release of `NSFB_KEY_MOUSE_5` per notch toward the person
   (`MOUSE_4` away), at most 64 owed; the terminal moves its view three lines a notch (`zoraya_term::WheelLines`).
   `zoraya-app` ignores the event for now (the file manager, the editor, settings: `docs/backlog.md`).
4. **Tests.** `make test-netsurf` turns the wheel over a tall page through the runner's `wheel X Y up|down [N]` step
   (QMP's `wheel-up`/`wheel-down` buttons) and reads the page's lower part.

## Consequences
- **Positive.** The wheel works in NetSurf and the terminal; a high-resolution device needs no protocol change.
- **Negative.** A new field type in `zoraya-protocol` (`i32`); `zoraya-proto-input`'s `kind` widens from `Below<2>` to
  `Below<3>` (a driver and compositor of one build; the input protocol is not a client boundary).
- **Follow-up.** The wheel in `zoraya-app` and its widgets; a touchpad's source and kinetic stop if one is measured.

## Invariants
- **INV-0322-1.** A `Wheel` goes only to the client of the window whose body is under the pointer, never during a move,
  a drag or the switcher, never to a window with a sheet over it, and carries the modifiers only to the focused client;
  it moves no focus. *Check:* `cargo test -p zoraya-compositor` (`tests_wheel.rs`).
- **INV-0322-2.** Every wheel record and message decodes totally and stays within its bound: `inputd` keeps only
  records the input protocol's `is_valid` takes, the compositor sends at most `WHEEL_MAX` either way. *Check:*
  `cargo test -p zoraya-virtio-input -p zoraya-proto-input -p zoraya-proto-gui -p zoraya-compositor`, and the fuzz
  target `compositor_protocol` (input frames are part of its input).

## Open questions
- A research note on wheel and touchpad conventions (Wayland's `axis_source`, macOS's phases and momentum, Haiku's
  float deltas) belongs in `docs/research/desktop/survey.md`; this ADR states them from the systems' documentation.
- Natural (reversed) scrolling as a setting (`gui.*`): not offered; it would be a sign the compositor applies.

## Revisit triggers
- A touchpad or a device with kinetic scrolling on QEMU or real hardware; a client that needs the source.

## Architect's decision
