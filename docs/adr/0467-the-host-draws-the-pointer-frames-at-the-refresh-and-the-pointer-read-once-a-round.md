# ADR-0467: The host draws the pointer, frames come at the display's refresh, and a device's pointer messages are read once a round

- **Status:** proposed
- **Date:** 2026-11-20
- **Depends on:** ADR-0090, ADR-0092, ADR-0093, ADR-0094, ADR-0160, ADR-0246
- **Evidence:** alternatives-postmortem.md#genode

## Context

The architect's hand report (2026-11-20): moving the pointer and dragging windows on the desktop is not smooth under HVF on the
Mac mini. `docs/tasks/TASK-31-gui-smooth-report.md` records the measurements (`compd`'s bounded measurement, `make test-gui`'s
`pointer` script, scales 1 and 2). What they show, before this ADR's changes:

- **The pointer was a frame.** Every pointer message made a round, a frame of the pointer's old and new boxes (one bounding box,
  so a fast move damaged up to 50 000 pixels at scale 1 and 11 784 on average), a flush of that box through two device commands, and
  the answer, a round trip through `gpud` each time. 170 messages a second were 177 frames, 8 MiB of transfer a second at scale 1
  and 36 MiB at scale 2, for a 8 by 11 pointer.
- **One message a round.** The compositor read one message of each input device per round and composed after each; ten positions
  waiting were ten frames.
- **Frames as fast as input.** A dragged window composed once per message: at scale 2, 12 ms a frame and 12 ms to flush its
  1.9 million pixels, so the guest ran at 40 to 80 frames a second and its input queue overflowed (the device gave 53 to 89
  messages a second of the 170 sent).
- ADR-0090 left the cursor queue unset-up ("the cursor queue if the software pointer costs too much") and ADR-0093 has the
  compositor draw its own pointer. The follow-up is now due.

**Known:** virtio-gpu has a cursor queue (queue 1) on which `UPDATE_CURSOR` and `MOVE_CURSOR` need no response and the host draws
the image over the scanout (virtio 1.2, 5.7.6.10); QEMU's device needs a 64 by 64 resource in `B8G8R8A8`. **Assumed:** the host's
own display (Cocoa, SDL, none) shows a guest cursor image when the device gives one; the screendumps of the GUI tests do not
include it (to be confirmed by the architect's own look).

## Decision drivers

1. **The pointer is the one thing the person moves without a client's help**: it must follow at the input's rate, with no frame.
2. **The compositor is the trusted drawer** (INV-0160-1, ADR-0093): no client's pixels reach the pointer, and the pointer's look
   stays one of the tokens.
3. **A driver treats every message as hostile** (ADR-0090): a position, a hot spot, a length from the compositor is checked.
4. **Never wait for the display driver** (ADR-0093): a pointer message that finds the driver's queue full is dropped and told
   again, never waited for.
5. **Small and reversible**: the software pointer remains as the fallback.

## Considered options

### Option A: leave the pointer to the compositor (the baseline)
- **Description:** what exists. **Cost:** none. **Evidence against:** the measurements above.
- **Precedent:** Linux's fbdev compositors without a cursor plane; every system draws its pointer in software until it has a plane.

### Option B: the compositor gives `gpud` the pointer's image and its place; the host draws it (recommended)
- **Description:** `gpud` sets up the cursor queue. Two display messages: `SetCursor` (ordinal 5, a reply `Done`) carries a 64 by 64
  image of `0xAARRGGBB` words, its hot spot and the place; `MoveCursor` (ordinal 6, no reply) carries a place. The image is built
  by the compositor from the pointer's dots and the palette (INV-0467-1); a new image goes at a new scale or appearance. A
  driver that refuses leaves the software pointer in force. Pointer moves cost one small message and no frame.
- **Evidence for:** the measurements; virtio's own design; `gpud` owns no policy.
- **Evidence against:** one more pair of messages in ADR-0090's protocol; the host decides how the cursor looks beside the guest's
  pixels (a screendump does not hold it).
- **Precedent:** Genode's nitpicker draws the pointer itself and the GPU driver's cursor plane shows it (alternatives-postmortem.md#genode);
  every DRM driver with a cursor plane.

### Option C: a client (or the browser) gives the image
- **Description:** cursor shapes from applications. **Evidence against:** a client's pixels over the whole screen can imitate the
  trusted UI (ADR-0160's rule that the person can tell the system's drawing from a client's). Not taken here; a future ADR may allow
  a few named shapes the compositor draws.

### Option D: keep a software pointer and only make frames cheaper
- **Description:** damage in several rectangles, a faster compose. **Evidence against:** every pointer move stays a frame and
  a flush; the cost per move falls but does not go.

## Recommendation

Option B, with the two smaller changes that the measurements asked for:

1. **The host draws the pointer** (Option B). `SetCursor` once per image (scale or appearance), `MoveCursor` at most once a round and
   only when the place changed. The frame has no pointer once the driver holds the image.
2. **A device's messages are read together.** Up to 32 messages of each input device a round, and the pointer told to the window
   under it once, at the end, with the last place; a message that changes a button or turns a wheel is told at its end as before.
3. **One frame per refresh.** At most one frame per 1/60 s (`compd` sets 16 666 667 ns; the compositor library's default is none,
   so the host tests see every frame). What changed waits in the damage and the loop's wait ends at the due time. A client's commit is
   answered in its own round: a commit is never held (a client with a commit unanswered is not read, and would keep the loop awake).
4. **The measurement** (not a decision of the product): `compd` prints one console line per second of pointer activity, at most 64
   a boot, only when `/config/gui-measure` exists.

Not decided here: moving a dragged window's last drawn pixels instead of composing them again (see the report: the cost is
the transfer, and a frame's compose is what the blit would save); the report says what was and was not done.

## Consequences

**Positive:** a pointer move is 0 frames and 0 flushed bytes; ten waiting positions are one frame; a drag is at most 60 frames a
second; the driver never blocks on the cursor (no completion interrupt asked for).

**Negative:** the host draws the pointer, so a screendump of the guest has none (the GUI tests that read pixels under the pointer
read the wallpaper); a frame held for up to 16.7 ms adds that to a keystroke's echo when another change was within the interval;
two messages added to a protocol the driver treats as hostile.

**Follow-up:** a drag by moving the last drawn window; a rectangle list instead of one bounding box where two changes are far
apart; the host's own refresh read from the device instead of 60 Hz fixed.

## Invariants

- **INV-0467-1.** The pointer's image is the compositor's own: built from the pointer's dots and the palette, never from a client's
  memory or message. *Check:* `cargo test -p zoraya-compositor` (`a_hardware_cursor_moves_with_no_frame`: the image's words are
  the palette's colours at the dots, clear elsewhere).
- **INV-0467-2.** The driver checks what the compositor sends the cursor queue: an image of other than 64 by 64 pixels or a hot spot
  outside it reaches no device, and a position is clamped to the display. *Check:* `cargo test -p zoraya-virtio-gpu` (`a_cursor_image_of_the_wrong_size_or_a_hot_spot_outside_it_reaches_no_device`); the clamp is in `gpud` and is checked by reading.
- **INV-0467-3.** The compositor never waits for the cursor: `MoveCursor` is a write that is dropped when the queue is full and told
  again with the next place; a refused or missing image leaves the software pointer. *Check:* `cargo test -p zoraya-compositor`
  (`the_pointer_is_drawn_by_the_compositor_until_the_driver_holds_an_image`).
- **INV-0467-4.** With a frame interval set, at most one frame is composed per interval unless a client's commit waits for it, and
  a commit is answered in its round. *Check:* `cargo test -p zoraya-compositor` (`frames_are_at_most_one_per_interval_when_one_is_set`,
  `a_commit_is_answered_in_its_round_even_when_a_frame_is_held`).

## Open questions

- Whether Cocoa shows the guest's cursor image over its own: the architect's look at `make run-gui` (scale 2) settles it; if it
  does not, the option to turn the hardware cursor off is `SetCursor`'s refusal, which a setting could ask for.
- Whether the display's refresh should be read from the host (QEMU gives none to the guest): 60 Hz is assumed.
- The two-rectangle damage and the blit of a dragged window: measured, not built (see the report).

## Revisit triggers

- A client needing a cursor shape of its own (text caret, resize arrows): a superseding ADR for named shapes.
- A display that is not 60 Hz, or a host that shows the guest's cursor wrongly.

## Architect's decision

(Left empty for the human.)
