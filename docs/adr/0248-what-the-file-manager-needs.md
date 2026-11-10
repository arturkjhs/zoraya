# ADR-0248: What the file manager needs from the compositor, the panel and the file service (stage 7h)

- **Status:** proposed
- **Date:** 2026-11-10
- **Depends on:** ADR-0055, ADR-0153, ADR-0160, ADR-0167, ADR-0168, ADR-0169, ADR-0170, ADR-0172, ADR-0245
- **Evidence:** desktop/survey.md#macoss-sandboxed-open-panel, desktop/survey.md#haiku

## Context
The architect approved a redesign of the file manager (mockups `docs/research/desktop/mockups/clear-field/files*.png`,
2026-11-10) and asked for: selection with Shift and Command, Command+Up to the parent, Command+Shift+N for a new
folder, a context menu whose show-commands mode shows each item's `zosh` line, Copy path, Open terminal here (a
terminal in this folder and the window's room), the free space in the status line, and a drag onto a folder that
copies or moves while a drag onto another window grants the file (ADR-0170).

Several of these meet accepted rules. INV-0160-3: a chord with the system key never reaches a client, and the
Command keys are the system key under QEMU's cocoa window (ADR-0160). `WindowPointer` carries no modifiers, so a
client cannot tell a Shift or Command click. The show-commands mode is told to system clients only (ADR-0245's
condition). INV-0170-2: an `Offer` is accepted only after the compositor's `Copy`, so a menu item cannot copy.
INV-0170-3: the source of a drag hears only `DragEnded`, so it cannot know that the drop was on one of its own
folders. The file protocol (ADR-0055) says nothing of free space. The panel's launcher channel (ADR-0245) opens files
and nothing else.

Measured: nothing; host tests and the GUI tests of stage 7h.

## Decision drivers
1. The gestures the architect asked for, with the keys he named, on the Mac that is the one host.
2. The compositor keeps every chord it uses; nothing a client sends can reach another client or the clipboard
   without the person's gesture (ADR-0160, ADR-0170).
3. No new right: the file manager changes only what its places allow (INV-0168-2 as changed on 2026-11-10).
4. Every gesture has its text (ADR-0172 point 8).

## Considered options

### Option A: Six small widenings, each bound to the person's gesture (built)
- **Description:**
  1. *System chords the compositor does not use* (anything but W, M, Space, Left, Right, Enter, R, ?, C, X, V) reach
     the focused client as a `Key` with a new modifier bit, `SYSTEM`. The compositor's chords never do. A change to
     INV-0160-3.
  2. *`WindowPointer` carries the modifiers held* (Shift, Ctrl, Alt, `SYSTEM`), a new field.
  3. *`Commands` goes to every client*, not only to system clients: one bit, the mode the person turned on.
  4. *An `Offer` is accepted unasked within 1 s of a press or key the compositor delivered to that client's focused
     window* (a gesture inside the window, as browsers' transient activation): Copy path. A change to INV-0170-2.
  5. *`DragEnded` says where the drop was when it was on the source's own window* (`over`, `x`, `y`,
     `modifiers`), and nothing else; no handle moves. The source copies or moves what it already holds. A change to
     ADR-0170's `DragEnded`, not to INV-0170-3.
  6. *`FreeSpace`* (file protocol, ordinal 37): `Done`'s count is the volume's free bytes, on any connection.
  7. *`OpenTerminal { path }`* on the panel's launcher channel: the panel launches a terminal in the caller's room
     with its current directory at `path`, if a hold of that room (or `home`'s) covers `path`, else refuses.
- **Evidence for:** Drivers 1 to 4. Each widening carries only what the person did in that window; the system key
  still starts every compositor chord, and no client learns another's input.
- **Evidence against:** A client now sees System+letter chords the compositor leaves free, so a later compositor
  chord takes a key some application may use (a list kept in one place, `TAKEN_CHORDS`). Copy path writes the
  clipboard without System+C, within a second of a click in the window.
- **Cost:** Built: about 300 lines outside the file manager.
- **Precedent:** macOS sends Command chords the system does not take to the application; its open panel is a
  separate process (`desktop/survey.md#macoss-sandboxed-open-panel`). Haiku's Tracker copies on a drop onto a
  folder of another volume and moves on the same one (`desktop/survey.md#haiku`).

### Option B: Ctrl instead of Command, no new protocol
- **Description:** Ctrl+Up, Ctrl+Shift+N, Ctrl-click; no Copy path; no drop on one's own folders; no free space.
- **Evidence against:** Not what the architect asked (driver 1); half the features missing.
- **Cost:** Small.

### Option C: The compositor does the file manager's gestures itself
- **Description:** The compositor recognises a Command-click or a drop on a folder and tells the client what it
  means.
- **Evidence against:** The compositor learns the file manager's widgets (ADR-0165 keeps widgets in clients).

### Option D: What Linux does
- **Description:** Every key and modifier reaches the focused client; the clipboard is any client's to set.
- **Evidence against:** INV-0160-3 and INV-0170-2 exist to prevent it (driver 2).

## Recommendation
Recommended: **Option A**, as built in stage 7h. It is a recommendation, not a decision. The file manager also
accepts Ctrl where it accepts Command, so that it works as Option B if points 1 and 2 are refused.

## Consequences
- **Positive.** The file manager of the mockups, with the keys the architect named.
- **Negative.** A future compositor chord must be checked against the applications' keys.
- **Follow-up.** A trash inside each writable place (`docs/backlog.md`).

## Invariants
- **INV-0248-1.** A chord with the system key whose key is in the compositor's table, Alt+Tab and F12 never reach a
  client; any other System chord reaches only the focused client, with `SYSTEM` set. *Check:* host test of the
  routing over every key with the system key held.
- **INV-0248-2.** An unasked `Offer` is accepted only from the client whose focused window received a press or a key
  within the last second, once per such gesture. *Check:* host test of the compositor's offer rule.
- **INV-0248-3.** `DragEnded` carries a place only when the drop was over the source's own window, and no `Drop` is
  sent then. *Check:* host test of a drag released over the source, over another client and over nothing.
- **INV-0248-4.** The panel launches `OpenTerminal`'s terminal only in the caller's room and only at a path that
  room's holds cover. *Check:* host test of the panel's launch logic.

## Open questions
- Whether Copy path should instead wait for System+C (no change to INV-0170-2).

## Revisit triggers
- A compositor chord that an application already uses.

## Architect's decision
