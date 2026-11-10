# ADR-0245: The chooser's wiring, the data directory and opening from the file manager in stage 7e

- **Status:** accepted
- **Date:** 2026-11-09
- **Depends on:** ADR-0153, ADR-0160, ADR-0167, ADR-0168, ADR-0172, ADR-0242
- **Evidence:** desktop/survey.md#macoss-sandboxed-open-panel, desktop/survey.md#xdg-desktop-portal, desktop/survey.md#capdesk-and-e

## Context
ADR-0167 (accepted) says what the chooser does: a trusted program outside the application shows the sheet, the
person's choice is the grant, the application gets one connection with ADR-0153's rights for its kind, and the frame's
pills show it. It leaves open how the pieces meet in a running system: who holds the chooser's channel to each
application and how the chooser knows which window to hang a sheet under; how the sheet is drawn by a program while the
compositor keeps its trusted parts; what a `Save` connection is; where an application's own files live; and how the
file manager hands a file to the application that opens it (ADR-0168 point 4). Stage 7e built one answer to each, and
its GUI test (`tests/gui/files.script`) runs them end to end. Measured: the test's round trips under HVF and TCG
(`docs/design.md`); nothing else.

## Decision drivers
1. No application names a file it was not given (ADR-0167 driver 1); the file manager's open gives read only.
2. The compositor alone draws what a client could forge: the sheet's strip, the emblem, the heading, the bar's mark
   (INV-0167-4).
3. Nothing new in the kernel; the panel stays the one launcher (ADR-0168), the chooser a system client (ADR-0167).
4. What an application holds is what its record shows (INV-0172-2).

## Considered options

### Option A: Panel-registered chooser channels, a system client's sheets, a data directory per room and application (built)
- **Description:** (1) The chooser is a compositor client the driver manager marks with `AddSystemClient`; it holds
  the volume's root. (2) For every application it launches, the panel makes a channel, gives one end to the
  application (slot 6) and the other to the chooser with `AddApplication { tag, name }`, the tag being the one the
  panel gave the compositor in `AddRoomClient`; an untrusted room's application gets none. (3) The chooser asks
  `CreateSheet { tag, kind }`, which the compositor accepts only from system clients, and draws the sheet's body
  with `zoraya-ui`; the compositor draws the strip, the emblem `Z` and "Open a file for APP" above it and the bar's
  middle mark, gives the owner no input while the sheet is open, and moves, hides and closes the sheet with its owner.
  (4) `Chosen` carries one connection: for `Open` the file with `+r`'s rights; for `Save` the folder narrowed to the
  one name (ADR-0153 rule 2) with `create`'s rights, so the application can create or replace that name and no
  other; for `PickDirectory` the folder with `read` or `tree`. The chooser then sends `Granted { tag, words }` and
  the compositor adds the pill (`+r ~/notes.txt`, `+c ~/nový.txt`). (5) An application's data directory is
  `/data/ROOM/APP` (`/data/home/APP` in home), made by the panel and given at slot 7 with every right but publish.
  (6) The file manager asks the panel `OpenWith { name, words }` with a file connection; the panel re-clones it to
  `READ | CLONE` whatever was sent and launches the application for the name's extension (`.txt`, `.md`: `t_open`
  in 7e, `edit` from 7f) with it at slot 10 and the words in its record.
- **Evidence for:** Each piece is one message on an existing channel; the chooser never learns a window id, only a
  tag the panel and the compositor share; the file manager cannot widen what it hands on.
- **Evidence against:** The chooser draws the sheet's body itself, so a bug there shows the person wrong text inside
  a trusted strip (the connection made is still exactly the grant). The opened application starts in `home`, not
  in the file manager's room (below).
- **Cost:** Built: about 2,400 lines with tests (`zoraya-proto-chooser`, `zoraya-chooser`, `zoraya-fsclient`,
  `zoraya-files`, the chooser, `files`, `t_open`, the panel's half).
- **Precedent:** macOS's open panel, a separate process that extends the sandbox (`desktop/survey.md#macoss-sandboxed-open-panel`);
  Flatpak's document portal (`desktop/survey.md#xdg-desktop-portal`); CapDesk's powerbox (`desktop/survey.md#capdesk-and-e`).

### Option B: The compositor draws the whole sheet
- **Description:** The chooser sends the listing and the compositor renders every widget.
- **Evidence for:** Every pixel of the sheet is the compositor's.
- **Evidence against:** The compositor grows a toolkit; ADR-0165 keeps widgets in `zoraya-ui`, in clients.
- **Cost:** Large.
- **Precedent:** EWS drew trusted labels only, not whole dialogs.

### Option C: What Linux does (the application opens its own dialog)
- **Description:** The application lists the volume and opens what the person picks.
- **Evidence against:** The application then holds the volume (ADR-0167 driver 1). Rejected by ADR-0167.

## Recommendation
Option A, as built. It fills the gaps ADR-0167 and ADR-0168 left without changing either: the messages are the ones
ADR-0167 names (`Open`, `Save`, `PickDirectory`, `Chosen`, `Cancelled`) plus three for the wiring (`AddApplication`,
`OpenWith`, and the compositor's `CreateSheet`/`Granted`), and the rights are ADR-0153's table.

## Consequences
- Positive: the 7e acceptance test runs it whole: the file manager opens a file read-only in `t_open`, which then
  opens one through the chooser (read, write refused) and saves one (written; a second name beside it refused).
- Negative: the application opened from the file manager starts in `home` even when the file manager is in a room
  (the panel's launcher channel does not yet carry the room); the chooser holds the volume's root, as ADR-0167 says.
- Follow-up: the room on `OpenWith`; drops (ADR-0170) reuse `Granted` for their pills.

## Invariants
- **INV-0245-1.** An application launched by the panel holds a chooser channel only outside untrusted rooms, and the
  chooser hangs a sheet only under the tag registered with that channel. *Check:* `zoraya-panel`'s host test of
  `application`, and the chooser's session tests (a sheet is opened with the registered tag).
- **INV-0245-2.** A file the file manager opens reaches the application with `READ | CLONE` only. *Check:*
  `tests/gui/files.script` (the write is refused).
- **INV-0245-3.** No input reaches a sheet within 500 ms of its mapping (its first commit), and no key or button held
  from before reaches it until released. *Check:* the compositor's host tests of the sheet guard (a press, a key and a
  key held from before, within and after the guard; a key pressed within it; two sheets; a lost input driver).

## Open questions
- From the review of 7e: re-placing a sheet when
  the scale or the display changes; the owner's popups while its sheet is open; a `Save` onto an existing folder's
  name; the re-clone of `OpenWith`'s connection waits on the file manager's channel (`files` is a program of the image).
- Drops (ADR-0170) as built in 7e: the pill is the compositor's `dropped: NAME read`, the name a display name the
  source chose (at most 64 bytes, no `/`); a drop on an untrusted room's window waits for the confirming card.
- Whether the file manager should hold the root (INV-0168-2 allows it) or the person's home and the rooms' grants.
- Whether `Chosen`'s words for `Save` should be the shell's line or the sheet's sentence (now the sentence).

## Revisit triggers
- A second launcher besides the panel; an application that needs more than one file per request.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request.

Option A, with two conditions: (1) a sheet ignores input for 500 ms after it maps, and ignores any key held from before it mapped; done now, with a test. (2) `Chosen`'s words stay the sheet's sentence, and show-commands mode shows the shell's line; for `Save`, which has no line of its own in `zosh`, the least grant that covers the name, `+w PARENT APP` (the architect's answer of 2026-11-10).
