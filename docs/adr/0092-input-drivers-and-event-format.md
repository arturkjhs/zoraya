# ADR-0092: Input drivers and the event format

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0006, ADR-0050, ADR-0052, ADR-0054, ADR-0091, ADR-0093, ADR-0094
- **Evidence:** DM-079, DM-072, DM-069, GR-016, GR-021, CP-24, CP-14, alternatives-postmortem.md#plan-9--inferno, stage-4/sources.md#virtio-and-qemu

## Context
QEMU offers `virtio-keyboard-device` and `virtio-tablet-device` on virtio-mmio (`stage-4/sources.md#virtio-and-qemu`). Both are virtio-input, device ID 18: the device writes 8-byte events, `{type, code, value}` with Linux evdev numbering, into buffers the driver offers on queue 0, and a configuration window describes the device by name, event types and axis ranges `[outside evidence: virtio 1.2, section 5.8]`. A keyboard sends key codes, not characters. Something must turn codes into text under a layout. Measured: nothing on Zoraya.

## Decision drivers
1. Input goes only to the compositor, which routes it (CP-24, GR-016, DM-079).
2. The device is untrusted (GR-021): every event is checked.
3. A slow compositor must not stall the driver, and a burst must not grow memory (CP-14).
4. One place for the layout, so that a client cannot be told a different one.

## Considered options

### Option A: evdev in the kernel, keymap in the compositor or client library (Linux)
- **Description:** A kernel input core exposes `/dev/input/event*`; the compositor reads raw codes and libxkbcommon turns them into keysyms, in the compositor and again in each client from a keymap the compositor sends.
- **Evidence for:** A complete layout system, used by every Wayland compositor (GR-016).
- **Evidence against:** The input path is kernel code, against ADR-0054. Clients translate too, so a layout lives in two places.
- **Cost:** *(estimate)* A kernel input core; a layout compiler.
- **Precedent:** Linux.

### Option B: Raw codes from the driver; the keymap in the compositor
- **Description:** `inputd` forwards checked evdev codes and absolute coordinates. The compositor holds the layout and gives a focused client both the code and the text it produces.
- **Evidence for:** Drivers 1 to 4. A client gets text without a layout of its own, and a game can still use codes.
- **Evidence against:** A layout change needs the compositor; per-client layouts are not possible.
- **Cost:** *(estimate)* A 200-line driver crate and a 150-line keymap crate.
- **Precedent:** Wayland's compositor-side keymap, without the client side (GR-016).

### Option C: Characters from the driver
- **Description:** The keyboard driver holds the layout and sends characters.
- **Evidence for:** The simplest client.
- **Evidence against:** Modifiers and keys without text (arrows, function keys) need a second encoding anyway, a reserved key for the compositor (ADR-0094) must be recognised after translation, and a second keyboard driver repeats the layout. DM-069 records what a translation layer far from its consumer became.
- **Cost:** *(estimate)* Small now, repeated per driver.
- **Precedent:** A PC BIOS keyboard handler `[outside evidence]`.

### Option D: Runes from the kernel (Plan 9)
- **Description:** Plan 9's kernel turns scan codes into Unicode runes through a loadable table, and `rio` gives each window its own `/dev/cons` (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** Text is UTF-8 from the start, and every window gets only its own input.
- **Evidence against:** Translation in the kernel, against ADR-0054 and ADR-0060's "no terminal policy in the kernel"; codes are lost, so a key without a rune has no portable name.
- **Cost:** *(estimate)* A kernel table and a device.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **One driver, `inputd`,** for both devices, one process per device: slot 5 the device (ADR-0091), slot 6 the channel to the compositor, the range as argument. It finds what it drives from the configuration: a device that reports `EV_ABS` with `ABS_X` and `ABS_Y` is a pointer, one that reports `EV_KEY` alone is a keyboard. It reads each axis's `min` and `max`, and refuses a range with `max <= min`.
- **Queue.** Queue 0 with 64 buffers of 8 bytes `[assumption]`, all offered at start and offered again when read. Queue 1 (status, for LEDs) is not set up.
- **Checks.** A used entry must name an offered buffer, with length 8. An event is kept only if it is `EV_SYN`, `EV_KEY` with a code below `0x300` and a value of 0, 1 or 2, or `EV_ABS` for `ABS_X` or `ABS_Y` with a value inside the range the device declared. Anything else is dropped and counted, and the driver goes on: a strange event is not a protocol break. A broken used ring is, and the driver resets the device and exits, as `blkd` does.
- **Event format** (`zoraya-proto-input`). One message per `SYN_REPORT`, holding up to 64 records `{kind, code, value}`, plus the count of events the driver dropped since its last message. Kinds: `KEY` (code, 0 released, 1 pressed, 2 repeated) and `AXIS` (0 for x, 1 for y; value scaled to `0..=65535` from the device's range). Pointer buttons are `KEY` codes (`BTN_LEFT` is `0x110`), as evdev has them.
- **Never blocks.** If the compositor's queue is full the driver drops that frame, counts its events and goes on reading the device, so the device's queue never fills (driver 3). The count reaches the compositor with the next frame that fits.
- **The layout** lives in the compositor, in a host-tested `zoraya-keymap` crate: US, with Shift, Caps Lock, Ctrl and Alt. A focused client gets `{code, pressed, modifiers, text}`, where `text` is the character or nothing, and Ctrl with a letter gives its control character. A client never sees the driver's protocol.

**Tensions with accepted ADRs.** None beyond ADR-0091's placement.

### Effect of the vision
None on the choice. For target 2, an agent that drives a GUI would need synthetic input, which DM-072 and CP-24 say must be a capability granted by the compositor, never a path into the driver protocol: the driver's channel is held only by the compositor, so nothing here precludes it. Target 3 needs more layouts and input methods, which change the keymap crate and its table, not the wire. Networking and threads are untouched.

### Deferred to stage 6
Nothing here depends on POSIX. A POSIX terminal gets bytes from the terminal client (ADR-0096), not codes.

## Consequences
- **Positive.** One small driver for both devices. Text comes from one table.
- **Negative.** One layout for the whole system. Keys repeat at QEMU's rate, not the user's choice.
- **Follow-up.** A layout setting; the status queue for LEDs.

## Invariants
- **INV-0092-1.** The driver forwards only events of the kinds, codes and ranges listed above, and a value outside the declared axis range never reaches the compositor. *Check:* host tests against a fake device that writes every kind of bad event and a broken used ring.
- **INV-0092-2.** The driver never waits for the compositor: a frame that does not fit is dropped and counted, and the count reaches the compositor in the next message that fits. *Check:* host test of the driver logic with a client whose queue is full.
- **INV-0092-3.** Translation is total: every code, state and modifier combination gives a key, with or without text. *Check:* exhaustive host test over codes below `0x300` and all modifier states.

## Open questions
- Whether 64 buffers suffice for a fast typist under TCG: the dropped count is reported, and the GUI test types at QMP's speed.

## Revisit triggers
- A second layout.
- A client that needs synthetic input (accessibility, automation).

## Architect's decision
