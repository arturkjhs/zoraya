# ADR-0329: The wheel in the toolkit: a third widget event, three rows a notch, the view moves and the cursor stays

- **Status:** accepted
- **Date:** 2026-11-14
- **Depends on:** ADR-0164, ADR-0165, ADR-0168, ADR-0244, ADR-0322
- **Evidence:** desktop/survey.md#focus-and-stacking, desktop/survey.md#own-no_std-widgets

## Context
ADR-0322 (accepted) gives a client a `Wheel` message, `dx` and `dy` in 120ths of a notch, to the body under the
pointer, and left `zoraya-app` ignoring it; the architect decided on 2026-11-14 (part 9f-wheel) that the file manager,
the editor and settings scroll with it, a notch "a sensible number of rows or lines, consistent with the terminal's
three lines", smoothly with a high-resolution wheel, clamped to the content, a list keeping its selection and cursor
rules, and the other native applications with scrollable content too where they share the toolkit. Read in this
part: ADR-0244 (accepted) gives `zoraya-ui` two events of its own (a key and a logical pointer; `Focus` was added
since) translated by `zoraya-app`, and names "a widget that needs a protocol event the two do not carry" as its
revisit trigger. `ScrollView` held one child and an offset in logical pixels, but laid the child out at the frame's
top with its height cut by the offset, because `Logical` is unsigned and an `LRect` cannot start above its parent:
nothing it showed ever moved, and nothing called its `scroll`. Settings put its page in one only when the Rooms
section is there. The file manager's list (`zoraya-files`), the editor's view (`zoraya-editor`), the toolkit's
`ListView` (the chooser's sheet) each keep a first row shown and bring the cursor or the selection into view at every
paint, so a view moved by anything but the cursor would spring back. Settings and the editor build their window's
tree again after a change (`App::replace`), which loses any widget state. Measured: nothing; this is a design of
what 9f-wheel built.

## Decision drivers
1. ADR-0244's split: widgets see only what they act on, in logical pixels; `zoraya-ui` depends on no protocol crate
   (INV-0244-1).
2. The pointer's rule (ADR-0094, `desktop/survey.md#focus-and-stacking`): a wheel goes to what is under the pointer
   and moves no focus.
3. One unit across the system: the terminal's three lines a notch (`zoraya_term::WHEEL_LINES`).
4. A high-resolution wheel (parts of a notch) must scroll as far as one with notches and in smaller steps.
5. The least change to `zoraya-ui`'s geometry: `Logical` stays unsigned (ADR-0164), no off-screen buffers.

## Considered options

### Option A: The Linux/GTK way, a scrolled window in pixels with adjustments
- **Description:** GTK's `GtkScrolledWindow` and `GtkAdjustment` (from GTK's documentation, not `docs/research/`):
  the child is allocated its whole height and painted through a translated, clipped context; a wheel notch scrolls
  a step that grows with the page, smooth deltas scroll pixels.
- **Evidence for:** Pixel-smooth scrolling of any content; what toolkits on Linux do.
- **Evidence against:** Needs signed layout coordinates or a translation in the canvas and in every hit test: every
  widget's `rect()` and `LRect::physical` change, against driver 5; a page-relative step is not the terminal's.
- **Cost:** *(estimate)* 600 lines across `zoraya-ui`, `zoraya-paint` and every widget.
- **Precedent:** GTK, Qt's `QAbstractScrollArea`.

### Option B: A third widget event, `Wheel`, routed up from the widget under the pointer; views scroll by rows
- **Description:** `zoraya_ui::Event::Wheel { x, y, dx, dy }` (logical point, 120ths of a notch), made by
  `zoraya-app` from `Wheel` (ordinal 46). `Window` gives it to the deepest widget under the point, then to each
  ancestor until one answers other than `Response::None`; a widget answers `None` when it cannot go that way (at its
  end), so a turn passes from a list at its end to the page around it. A notch is `ROWS_PER_NOTCH` (3) rows, lines
  or children; `WheelSteps` keeps the part of a row a turn leaves (40 of 120 is a row) and forgets it at an end.
  `ListView`, the file manager's list and the editor move their first row shown and leave the selection and cursor;
  a key, a press or a cursor the program moves (a find, a new folder) brings the cursor back into view. `ScrollView`
  becomes a column of children shown from its `first`: a child before it is laid out empty (it paints nothing and
  takes no press); `first` never passes the point where the last child is at the bottom; the keyboard's focus on a
  child out of view scrolls it in. `Window::replace_keeping_scroll` carries each scrolling widget's place (`Scrolled`)
  across a rebuilt tree by its path; `App::replace` stays as it was.
- **Evidence for:** Drivers 1 to 5: no protocol type in the toolkit, focus untouched, the terminal's unit, parts of
  a notch kept, no change to `Logical` or `LRect`.
- **Evidence against:** Settings scrolls by whole children of its page (a row of controls, a label), not pixels; a
  child cut at the frame's bottom is laid out shorter, as `Column` already does; a third event against ADR-0244's
  "two".
- **Cost:** Built in 9f-wheel: about 250 lines in `zoraya-ui`, 60 in `zoraya-app`, 40 each in the three applications
  and the chooser, with host tests.
- **Precedent:** Windows scrolls a `WHEEL_DELTA` as three lines by default (`SPI_GETWHEELSCROLLLINES`, from its
  documentation, not `docs/research/`); `zoraya-term` (three lines a notch, ADR-0322), the own-widget precedent of
  `desktop/survey.md#own-no_std-widgets`.

### Option C: The wheel as keys
- **Description:** `zoraya-app` turns each notch into three Up or Down key presses to the focused widget.
- **Evidence for:** No new widget event; every list already moves with the arrows.
- **Evidence against:** Goes to the focused widget, not the one under the pointer (driver 2); moves the selection
  and the cursor; no part of a notch.
- **Cost:** *(estimate)* 20 lines.
- **Precedent:** Some terminal emulators in the alternate screen (`less`, `vi` under xterm's `alternateScroll`).

## Recommendation
Recommended: **Option B**, as built in part 9f-wheel. It is a recommendation, not a decision. It adds to ADR-0244
(a third event, as `Focus` was) without changing its split: the toolkit still sees no protocol type.

## Consequences
- **Positive.** The file manager, the editor, settings and the chooser's list scroll with the wheel under the
  pointer; a high-resolution wheel scrolls a row at a time; settings' Rooms section, below the window until now, can
  be reached by the wheel and by Tab.
- **Negative.** Settings scrolls by children; `ScrollView::new` takes the children in place of one child (its only
  user was settings); a horizontal turn (`dx`) scrolls nothing yet.
- **Follow-up.** Pixel-smooth scrolling if a widget needs it (Option A's translation); a scroll bar; `dx` for a wide
  view; Page Up and Page Down in the file manager and the editor.

## Invariants
- **INV-0329-1.** A wheel turn moves no keyboard focus, no selection and no cursor in the toolkit, the file manager
  and the editor; it goes to the widget under the pointer or, at its end, to an ancestor. *Check:*
  `cargo test -p zoraya-ui -p zoraya-files -p zoraya-editor` (`tests_wheel.rs`, `the_wheel_*`).
- **INV-0329-2.** A scrolled view stays within its content whatever the turn: never before its first row and never
  past the point where its last row (line, child) is at the bottom; a turn of up to `i32` either way is bounded at
  64 notches. *Check:* `cargo test -p zoraya-ui -p zoraya-files -p zoraya-editor -p zoraya-settings-window -p
  zoraya-app`.

## Open questions
- Whether a notch should scroll a fraction of the view (GTK) rather than three rows when a row is tall (settings'
  children); measured nothing yet.
- Shift with the wheel as a horizontal turn: the modifiers reach only the focused client (ADR-0322), and the
  toolkit's event does not carry them.

## Revisit triggers
- A widget whose content must move by pixels (an image viewer, a page of mixed heights); a touchpad on QEMU or
  hardware whose kinetic turns make row steps feel coarse.

## Architect's decision
Accepted 2026-11-14. Decided by the architect; recorded by an agent at the architect's request.

Option B.
