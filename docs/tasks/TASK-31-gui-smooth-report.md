# TASK-31 part `gui-smooth`: the pointer and dragging windows, measured and changed

Branch `work/gui-smooth` (from `origin/main` bd72467c). The architect's hand report: moving the pointer and dragging windows is
not smooth on the desktop under HVF on the Mac mini. Governing ADRs: ADR-0090 (gpud), ADR-0092 (input), ADR-0093 (compositor),
ADR-0094, ADR-0160, ADR-0246; the new proposed ADR-0467 (status `proposed`, number from the task). Nothing here changes an
accepted ADR's text.

## How it was measured

`compd` has a bounded measurement (`crates/zoraya-compositor/src/measure.rs`): on only when `/config/gui-measure` exists, one console
line per second of pointer activity, at most 64 a boot. The test `pointer` (`tests/gui/pointer.script`, image layer
`tests/gui/pointer.root/config/gui-measure`; new runner steps `glide` and `sweep`) plays, over QMP's tablet, 500 pointer positions
over the wallpaper in two 1-second glides and then two 1-second drags of the terminal's window by its title bar, each position
4 ms after the last (the tablet delivers about 170 messages a second of the 250 sent). Run by `tools/test-runner` under
`--accel auto` (HVF) on the M4 Mac mini, QEMU 11.1.2, `--mem 512M`, one vCPU, `-display none` (so QEMU's own display work is not in
the numbers; the host window of `make run-gui` adds to them), at `--gui-scale 1` (1024 x 768) and 2 (2048 x 1536).

What the numbers are. *Delay* is from the first pointer event a flush carried to the display driver's answer to that flush:
the host's scan-out after it is not seen from the guest. *Flush KiB* is the area of the flushed rectangle times 4 (what
`TRANSFER_TO_HOST_2D` copies). Lines of a second of activity; a line whose second holds the end of a sweep (the 800 ms settle) is
diluted, so the tables use the lines of a full second. Logs: `target/test-logs/gs-*.log` of the author's machine (not kept).

## Before (bd72467c plus the measurement only)

| | scale 1, pointer only | scale 1, window drag | scale 2, pointer only | scale 2, window drag |
|---|---|---|---|---|
| Input messages a second the compositor read | 162 to 168 | 112 | 158 to 168 | 52 to 88 (of about 170 sent) |
| Rounds a second | 323 to 349 | 200 | 316 to 350 | 52 to 88 |
| Frames composed a second | 161 to 177 | 104 | 157 to 176 | 48 to 84 |
| Compose a frame (mean) | 7 to 97 us | 3.7 ms | 10 to 398 us | 10.3 to 12.9 ms |
| Damage a frame (mean) | 111 to 772 px (first: 11 784) | 486 000 px | 443 px (first: 52 632) | 1.48 to 1.90 million px |
| Flush a frame | 0 to 46 KiB | 1 897 KiB | 1 to 205 KiB | 5 742 to 7 413 KiB |
| Flushed a second | 2.9 MiB | 187 MiB | 12 MiB | 295 to 619 MiB |
| Delay, event to the driver's answer (mean, longest) | 0.2 ms, 2.2 ms | 4.4 ms, 34.8 ms | 0.2 ms, 2.3 ms | 10.8 to 13.9 ms, 58.9 ms |

Findings, in the order they drive the changes:

1. **The pointer was a frame.** One frame and one flush per pointer message (177 frames for 168 messages), each a bounding box of the
   pointer's old and new boxes, for a pointer of 8 by 11 dots. That is 160 to 175 flush round trips through `gpud`
   a second for nothing a person sees but the pointer.
2. **A frame per message.** The compositor read one message of a device per round and composed after each (rounds were twice the
   messages): ten waiting positions were ten frames.
3. **A dragged window composed in full per message.** 3.7 ms at scale 1, 10 to 13 ms at scale 2 for 1.5 to 1.9 million pixels, and the
   flush of the same box took 10 to 13 ms more at scale 2 (7.4 MiB a frame, 600 MiB a second). At scale 2 the guest kept up with 52
   to 88 of the 170 messages a second: the rest was lost in QEMU's input queue while the guest composed (visible as a pointer that
   jumps and a window that lags).
4. Not a cause: `FLUSH`'s answer time at scale 1 (21 to 900 us) and the frame's physical-page layout (a one-line experiment with a
   physically contiguous framebuffer changed the flush time by nothing: 10 to 12 ms at scale 2, so it was dropped).

## What was changed (ADR-0467, proposed)

1. **The host draws the pointer.** `gpud` brings up virtio-gpu's cursor queue; two new display messages `SetCursor` and
   `MoveCursor`; the compositor gives the 64 x 64 image it builds from the pointer's dots and the palette (never a client's
   pixels), and tells the place. A refusal leaves the software pointer.
2. **A device's messages are read together** (up to 32 a round), the pointer told once with the last place; a message that
   changes a button or turns a wheel is told at its end as before.
3. **At most one frame per 1/60 s** (`compd`), the held frame due at a deadline of the loop's wait; a commit is answered in its round.
4. **A dragged window is copied within the frame** when nothing else changed (see `docs/design.md`), the rest composed as ever.
5. The measurement itself, and the runner's `glide` and `sweep` steps.

## After (head of the branch; same script, same machine)

| | scale 1, pointer only | scale 1, window drag | scale 2, pointer only | scale 2, window drag |
|---|---|---|---|---|
| Input messages a second the compositor read | 164 to 166 | 182 | 159 to 164 | 182 (first second), 56 to 111 (the others) |
| Rounds a second | 166 | 286 | 164 | 249 |
| Frames composed a second | **0** | 58 | **0** | 58 (gap between frames 17.4 to 18 ms) |
| Compose a frame (mean) | none | 1.8 ms | none | 4.5 to 6.7 ms |
| Flush a frame | **0 KiB** | 1 905 KiB | **0 KiB** | 7 310 to 7 434 KiB |
| Flushed a second | **0** | 110 MiB | **0** | 420 MiB (first second) |
| `MoveCursor` messages a second | 165 | 172 | 163 | 166 |
| Delay, event to the driver's answer (mean, longest) | no frame | 15.3 ms, 18.3 ms | no frame | 17.0 to 17.2 ms, 18.6 to 21.8 ms |

What the numbers say, and do not:

- **Pointer only: no frame, no flush, no transfer** (it was 161 to 177 frames, 2.9 MiB a second at scale 1 and 12 MiB at
  scale 2). The pointer follows at the input's rate with one small message a move. Whether the host *shows* the guest's
  cursor image as the person expects (Cocoa) cannot be seen from the guest; the architect's look at `make run-gui` settles
  it (an open question of ADR-0467).
- **A drag: compose 2 to 3 times cheaper** (3.7 to 1.8 ms at scale 1; 10.3 to 12.9 to 4.5 to 6.7 ms at scale 2), the frame rate held
  at the refresh (58 a second, one every 17.4 to 18 ms) and the guest read 182 messages in the first second of the drag at scale 2
  (it read 52 to 88 a second before, of about 170 sent). The guest is no longer the bottleneck at scale 2.
- **Worse: the delay of a drag at scale 1** rose from 4.4 ms to 15.3 ms mean, because a frame now waits for the next 1/60 s slot
  (104 frames a second became 58). At scale 2 the delay is 17 ms against 10.8 to 13.9 ms. This is the price of "at most one frame per
  refresh": if the architect finds drags feel behind the hand, the first knob is `compd`'s `set_frame_interval` (a shorter interval,
  or pacing by the flush's answer instead of a clock).
- **Not changed: the bytes a drag sends.** A moved window's whole new place is transferred (1.9 MiB a frame at scale 1, 7.4 MiB at
  scale 2, 110 MiB a second and 420 MiB a second at 58 frames). Only a transfer of fewer pixels would lower that; the host's
  copy of the window's pixels cannot be moved by a command that virtio-gpu 2D has.

## What was not done, and why

- **Damage as a list of rectangles** ("transfer and flush only damaged rectangles"): the flush is still the bounding box of a
  frame's damage. After the hardware cursor, a frame's damage is one region (a window's move, a commit); the case a list would
  help (two changes far apart in one frame) was not in any measured line. Left as a follow-up in ADR-0467 and design.md.
- **The hardware cursor on the host's own window** was not looked at (no display in the test runs).
- **Cursor shapes** (resize arrows, a text caret) and a client's cursor: a client-chosen image is a trust question (ADR-0467,
  Option C) and was not taken.
- **The display's real refresh** is assumed 60 Hz.
- A full `make test-par`/`make test` (the architect's, not an agent session's).

## Review fixes, pacing and a second measurement (head of the branch after the review)

The independent review (`docs/tasks/TASK-31-review-gui-smooth.md`, R1 to R14, each with its disposition) found that `SetCursor` was a
blocking call; it is now a request answered later (as `Flush` is), with a test of a `Resized` queued before its answer. The ADR-0467
text and invariant rows were corrected (the copy of a dragged window is in the Recommendation, INV-0467-5).

**Pacing.** The rule asked for by the architect is the one the code already had: `frame_held` holds a frame only when the last
frame *began* less than one interval (1/60 s) ago; with the last frame older, a change is composed in its own round. A new host
test pins it (`an_event_after_an_idle_interval_is_drawn_at_once_and_one_right_after_a_frame_waits`: idle, then a move, a frame with
no wait; a move 2 ms after that frame is held and due within one interval). No code changed for it. The drag delay of about 15 ms
is therefore not a wait after idle: it is the cost of back-to-back frames. During a drag a message arrives every 5 to 6 ms, so
every frame follows another by less than an interval and waits for the slot (59 frames a second, gap 17.6 to 18.3 ms). The script's
glides and drags are all continuous motion; it has no slow-motion step, so the "delay after idle" is seen only in the first line of
each run (the first second of the first glide: mean 2.4 ms at scale 1, 1.9 ms at scale 2, with 7 and 9 frames). Lowering the
drag's delay would need a different rule (pacing by the flush's answer, or an interval shorter than the refresh), which is a
decision for the architect: the first knob stays `set_frame_interval`.

**Measured again** (`make test-gui GUI_RUNS="pointer:auto:1 pointer:auto:2"`, HVF, the same script and machine; lines of a full
second; logs `target/test-logs/gui-pointer-auto-{1,2}.log`, not kept):

| | scale 1, pointer only | scale 1, window drag | scale 2, pointer only | scale 2, window drag |
|---|---|---|---|---|
| Input messages a second | 191, 122 (a shorter line) | 191, 116 | 189, 124 | 191, 116 |
| Frames a second | 0 | 59, 35 | 0 | 59, 35 |
| Compose a frame (mean) | none | 1.0 to 1.4 ms | none | 3.4 to 5.2 ms |
| Flushed a second | 0 | 110 MiB (59 frames) | 0 | 428 MiB (59 frames) |
| `MoveCursor` a second | 190, 120 | 180, 107 | 188, 122 | 180, 104 |
| Delay (mean, longest) | no frame | 15.1 to 15.8 ms, 17.4 to 20.2 ms | no frame | 16.5 to 17.4 ms, 18.3 to 19.3 ms |

Against the first "After" table: the same frames, bytes and delays (the pointer only: no frame, no flush; a drag: 59 frames a
second, delay 15 ms at scale 1 and 17 ms at scale 2). The runs also show `compd: hardware cursor: image accepted` (the script
now fails without it, R7) and 0 hangs. The asynchronous `SetCursor` changed none of these numbers.

