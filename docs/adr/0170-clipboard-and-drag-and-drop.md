# ADR-0170: The clipboard and drag-and-drop

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0040, ADR-0050, ADR-0052, ADR-0055, ADR-0093, ADR-0094, ADR-0096, ADR-0153, ADR-0160, ADR-0161, ADR-0165, ADR-0167, ADR-0168
- **Evidence:** CP-24, DM-079, DM-082, GR-016, desktop/survey.md#spoofing-and-the-trusted-path, desktop/survey.md#capdesk-and-e, desktop/survey.md#eross-trusted-window-system

## Context
A desktop without copy, paste and drag-and-drop is not one a person enjoys; EWS's authors treat cut and paste as an interprocess communication mechanism every window system must offer (`desktop/survey.md#spoofing-and-the-trusted-path`). They are also the oldest leak between windows: in X any client reads every other client's input and window contents (DM-079), and the selection with them `[outside evidence: X's selection protocol]`; window messages between programs of different trust were the shatter attacks (DM-082). Wayland offers selections "only to the client that has keyboard focus" (GR-016), which still lets a focused client read the clipboard whenever it likes, and clipboard managers read everything. EWS made paste traceable: "no events are transmitted to the destination until the user performs a 'paste' action", a key table says which keystrokes authorise it, and mouse-driven paste goes through server-defined regions with a distinguished cursor (`desktop/survey.md#spoofing-and-the-trusted-path`, `desktop/survey.md#eross-trusted-window-system`). CapDesk supported drag-and-drop as an act of designation that grants (`desktop/survey.md#capdesk-and-e`). The architect's rule for this ADR: an application receives contents only on the person's gesture (paste, drop), and no application can read the clipboard at an arbitrary moment. Zoraya's compositor already owns input and routes it only to the focused or pointed surface (ADR-0093, ADR-0094), draws every frame (ADR-0160) and holds the panel's launch records (ADR-0168); the chooser turns a choice into a file entry (ADR-0167). Measured: nothing. Assumed: that text and file entries are the two kinds stage 7 needs.

## Decision drivers
1. Contents move only on the person's gesture: a paste or a drop, never on an application's request (the architect's rule; `desktop/survey.md#spoofing-and-the-trusted-path`).
2. No application can read the clipboard at a time of its choosing, or learn whether and what it holds (DM-079, GR-016's caveat, CP-24).
3. The gesture is one the compositor sees itself (a reserved chord, a drag it tracks), so an application cannot forge it (DM-082).
4. A dropped or pasted file is a grant in ADR-0153's vocabulary, made by the person (ADR-0167).
5. The contents are bounded, copied once, and never shared memory between the two applications (ADR-0093 driver 2).
6. Small: two client messages, seven events and one state in the compositor.

## Considered options

### Option A: Selections announced and requested by clients (X11, the Linux baseline before Wayland)
- **Description:** A client claims ownership of a selection; any client can ask the owner for its contents at any time, and the server relays.
- **Evidence for:** Flexible: clipboard managers, primary selection on middle click, any format.
- **Evidence against:** Drivers 1 and 2 fail: every client reads the clipboard whenever it likes (DM-079); a clipboard manager is a keylogger of everything copied.
- **Cost:** *(estimate)* Small to build, impossible to secure later.
- **Precedent:** X11.

### Option B: Offers to the focused client only (Wayland)
- **Description:** The compositor forwards the offer of the current selection to the client that has keyboard focus, which may request the contents at any time while focused (GR-016).
- **Evidence for:** Driver 2 for unfocused clients; proven on Linux desktops.
- **Evidence against:** The focused client still reads the clipboard without a gesture (driver 1 fails): a text field that is focused can take what was copied elsewhere without the person pasting; GR-016 lists privileged clients (virtual keyboards, screen readers, screenshot tools, lockers) as the unsolved part.
- **Cost:** *(estimate)* 200 lines.
- **Precedent:** Wayland compositors.

### Option C: Traceable paste: the compositor holds the clipboard and delivers only on its own gesture (EWS)
- **Description:** The compositor is the only holder of the clipboard. A copy is the reserved chord System+C (or System+X for cut) delivered to the focused client as a `Copy` event; the client replies within a bound with `Offer { contents }`, which the compositor copies into its own memory. A paste is the reserved chord System+V: the compositor delivers `Paste { contents }` to the focused client, and nothing else ever delivers contents. There is no message to read, query or enumerate the clipboard. Drag-and-drop is the same with the pointer: a client replies `Drag { contents }` to a button press the compositor delivered to it; the compositor tracks the drag, draws the cursor, and at release delivers `Drop { contents, x, y }` to the window under the pointer, which saw nothing before the drop (`desktop/survey.md#spoofing-and-the-trusted-path`).
- **Evidence for:** Drivers 1 to 6. EWS resolved exactly this problem by tying paste to a key table and drag-and-drop to interaction events the server itself sees; the compositor already sees every gesture and already copies client data once (ADR-0093).
- **Evidence against:** An application cannot offer "Paste" in its own menu as a thing that works by itself (a click on a menu item is not a gesture the compositor can tell apart from any other click, which is EWS's finding too); the compositor holds a copy of the clipboard (bounded); no primary selection, no clipboard history, no formats beyond the two kinds below.
- **Cost:** *(estimate)* 300 lines in the compositor, two messages and seven events, 100 lines in `zoraya-ui`.
- **Precedent:** EWS (research); the traceable-paste idea only, no production system does it this strictly.

### Option D: A shared buffer file (Plan 9's snarf)
- **Description:** The window system serves the clipboard as a file every window can read and write `[outside evidence: Plan 9's /dev/snarf; alternatives-postmortem.md says nothing on it]`.
- **Evidence for:** The simplest possible model; fits "everything is a file".
- **Evidence against:** Any program reads it at any time (drivers 1 and 2).
- **Cost:** *(estimate)* Tiny.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Contents** are one of two kinds: `Text` (UTF-8, at most 1 MiB `[assumption]`) or `Entries` (file entries in ADR-0153's vocabulary, as ADR-0167's chooser returns them: a connection, a display name and a kind, at most 64). The compositor holds at most one clipboard, the newest.
- **Copy and cut.** System+C and System+X are chords of ADR-0160's system key: the compositor sends the focused client `Copy` or `Cut`; the client may reply `Offer { contents }` within 500 ms `[assumption]`; the compositor copies text into its own memory and keeps entries' handles; a late or absent reply leaves the clipboard as it was. A client cannot send `Offer` unprompted: one without a preceding `Copy` or `Cut` to that client is refused and counted. A `Cut` is a `Copy` the client may act on (delete its selection) after the compositor confirms `Offered`.
- **Paste.** System+V: the compositor sends the focused client `Paste { contents }`, with handles for entries duplicated for that client only (ADR-0040's rights of the entry, no more). With an empty clipboard System+V sends nothing, so emptiness is not revealed. No other message carries contents, and no message tells a client whether the clipboard is empty, what kind it holds, or when it changed. A client that cannot use the contents ignores them.
- **Menus.** A client may show "Copy", "Cut" and "Paste" items; they do what the chords do only for the client's own text (a copy within one application is the application's own business and needs no compositor: its own selection stays in its own memory). Putting such an item's click through the compositor as a gesture is not in stage 7; the item shows the chord beside it, as macOS menus show their shortcuts. EWS's server-defined regions with a distinguished cursor are the later option.
- **Drag-and-drop.** On a button press the compositor delivered to a client, the client may reply `Drag { contents, serial }` naming the press's serial within the same 500 ms; the compositor then captures the pointer, draws a drag cursor (a token of ADR-0161), sends the source `DragStarted`, and sends nothing to any other client while the pointer moves: no `DragEnter`, no hover, nothing that reveals the drag to a window it passes over. At release over a window, the compositor delivers `Drop { contents, x, y }` to that window's client (with entries' handles duplicated for it) and `DragEnded` to the source (it carries nothing: whether the target used the drop is the target's business, ADR-0093's INV-0093-2); a release outside every window or Escape ends the drag with nothing delivered. A drop on the panel or a system window is delivered to it like any other (the panel accepts entries as "open with").
- **Entries as grants.** An entry copied or dragged from the file manager (ADR-0168) is a grant the person made: `files` offers each entry with the rights ADR-0153's table gives the `read` kind: a copied or dragged file or directory is a `read` entry, and nothing a drag does widens it (ADR-0153's Decision makes kinds other than `read` automatic only for base-system commands, and a drop on a program is no command line); the compositor passes the connection on; the receiving application's runtime adds it to its namespace table under a synthetic absolute path of its own (`/chosen/N/NAME`, N a counter: no entry shares a prefix with another, so ADR-0153 rule 2's merging never applies and the display name is a label, never a path, ADR-0167), and the panel's tooltip lists it with the words `dropped: NAME read`. The compositor never opens an entry itself.
- **Bounds.** One drag or one offer in flight; contents over the bound are refused with `LimitReached` and the clipboard unchanged; the compositor's copy is released when replaced.
- **`zoraya-ui`** (ADR-0165) implements these events for `TextField` and `ListView` (text selection and entries), so an application gets copy, cut, paste and drop of text and files without code of its own.

**Tensions with accepted ADRs.** *ADR-0093*: two new client messages (`Offer`, `Drag`) and seven new events (`Copy`, `Cut`, `Offered`, `Paste`, `DragStarted`, `Drop`, `DragEnded`), all new ordinals (ADR-0052); the rule "no message carries another client's events to a client" holds, since contents are delivered as the person's own gesture, not as the source's event. *ADR-0094*: three more chords of the system key, within ADR-0160's rule; its `Pointer` event gains a `serial` that the `Drag` reply names, a new version of the event under ADR-0052's new-ordinal rule. *ADR-0040*: handles in `Paste` and `Drop` are duplicates with the entry's rights, never more. *ADR-0153*: no new right or kind; `files` offers entries with the table's rights.

### Effect of the vision
For target 2 this is the mechanism that stops an agent's window from reading what a person copied elsewhere: an agent gets contents only when the person pastes into it or drops on it, and a supervisor can see every paste as a gesture the compositor logged. Target 3 wants clipboard history, several formats and primary selection; each is a compositor feature over the same messages. Networking: a remote clipboard is a relay the compositor would trust, a later capability. Threads: none.

### Deferred to stage 6
A POSIX program in a terminal copies and pastes through the terminal client, which keeps its own grid (ADR-0165) and implements the events itself: the terminal offers its selected cells as text and inserts a pasted text into its input as typed bytes (ADR-0096), with every control byte but `
` and `	` dropped, so nothing POSIX changes.

## Consequences
- **Positive.** Copy, paste and drop with no way for an application to read the clipboard on its own, and file drops that are grants.
- **Negative.** No menu-driven paste through the compositor in stage 7; no clipboard history; the compositor holds a bounded copy.
- **Follow-up.** The messages in `zoraya-proto-gui`; the compositor's state; `zoraya-ui`'s handling; the drag cursor token; the terminal's selection.

## Invariants
- **INV-0170-1.** Contents reach a client only in a `Paste` that follows a System+V from the person while that client is focused, or in a `Drop` that follows a button release over that client's window at the end of a drag the compositor tracked. *Check:* host test of the compositor over every client ordinal and every event sequence without the gesture, asserting no `Paste` or `Drop`; and with the gesture, asserting exactly one delivery to exactly the focused or pointed client.
- **INV-0170-2.** No client ordinal returns, describes or signals the clipboard: an `Offer` or `Drag` without a preceding `Copy`, `Cut` or button press to that client is refused, and no reply or event carries the clipboard's kind, size or presence. *Check:* host test sending `Offer` and `Drag` unprompted and inspecting every reply and event the compositor produces for any clipboard-dependent field.
- **INV-0170-3.** During a drag no client but the source receives an event that depends on the drag, and the source receives only `DragStarted` and `DragEnded`. *Check:* host test with three clients and a drag crossing all three windows, recording every event.
- **INV-0170-4.** A handle delivered in `Paste` or `Drop` carries no right the offered entry did not have, and the compositor holds an entry's handle only while that entry is the clipboard or the drag in progress: after a `Drop`'s delivery and after the clipboard's replacement it holds none. *Check:* host test comparing rights before and after, and the handle count of the compositor's fake platform.
- **INV-0170-5.** Contents over the bound, and a second offer or drag while one is in flight, are refused and leave the clipboard unchanged. *Check:* host test at the bounds.

## Open questions
- *(Check note, 2026-11-10, ADR-0001 rule 6.)* On `work/7h` the compositor takes `Drag` for a still-held press within 3 s (`DRAG_NAMES_WITHIN_NS`) instead of the 500 ms assumption above (proposed ADR-0248, point 4b: TCG measurements); the copy reply keeps 500 ms. Its host test checks the 3 s.
- *(Check note, 2026-11-10, ADR-0001 rule 6.)* On `work/7h` the compositor now also takes one unasked `Offer` within a second of a press in a trusted client's focused window, and its test checks that (proposed ADR-0248, built at the architect's request for the file manager); INV-0170-2's row stays `checked` for the rest of its rule. If ADR-0248 is refused, the undo is the offer rule's gesture branch removed, and the file manager falls back to Ctrl.
- *(Check note, stage 7f.)* INV-0170-1 and -2: `tests_drops.rs`'s `the_clipboard_moves_only_on_the_persons_chords`
  (an unprompted `Offer`, another client's, a late one and a second one are refused; `Paste` only to the focused
  window on System+V). INV-0170-5: the protocol's bound (32 KiB, `MAX_CLIPBOARD`) refuses larger contents at decode,
  and a second drag while one is carried is refused (test).
- *(Check note, stage 7e, ADR-0001 rule 6.)* Drag-and-drop is built in 7e, the clipboard in 7f. INV-0170-3 and
  INV-0170-4: `zoraya-compositor`'s `tests_drops.rs` (no pointer event reaches any window during a drag, the source
  gets only `DragDone` and `DragEnded`, the target only `Drop`; the entry is passed on once and closed on every other
  end), with two clients rather than three. `DragDone`, the reply to `Drag`, stands for `DragStarted`. The press's
  serial is a field of `WindowPointer`, counted per client (review notes 51). INV-0170-1, -2 and -5 need the
  clipboard's half (7f); their drop half is in the same tests. A drop on an untrusted room's window is not delivered
  until the confirming card exists (INV-0172-3).
- The 500 ms reply bound and the 1 MiB text bound.
- Whether a menu item's click should become a compositor gesture through EWS's distinguished regions, and at what cost in the toolkit.
- Whether a drop should ever carry more than `read` (a `tree` entry for a directory), and by what gesture the person would say so.
- Bracketed paste for the terminal: a pasted `
` runs a line as typed; whether the terminal should mark a paste so that the shell can tell it from typing.

## Revisit triggers
- A program that needs the clipboard without a gesture (an accessibility tool): a granted capability on the manager side, as ADR-0094 foresees for synthetic input.
- A second content kind (images).

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
