# ADR-0094: Input routing and focus

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0050, ADR-0092, ADR-0093
- **Evidence:** DM-079, DM-082, DM-072, GR-016, CP-24, CP-14, alternatives-postmortem.md#plan-9--inferno

## Context
ADR-0093 makes the compositor the only reader of input. It still has to decide which client gets a key, how that choice changes, which keys it keeps, and what happens when the chosen client does not read. In X any client can set the focus, grab the keyboard and send synthetic events (DM-079); Windows' UIPI was a retrofit against window messages across privilege levels (DM-082). Measured: nothing on Zoraya.

## Decision drivers
1. Only the person changes the focus (DM-079, DM-082, CP-24).
2. The compositor keeps at least one key that no client can see or block, so a person can always leave a window (GR-016).
3. A client that stops reading loses only its own input (CP-14).
4. Pointer events reveal nothing about other windows (DM-079).

## Considered options

### Option A: Client-set focus and grabs (X11, the Linux baseline before Wayland)
- **Description:** A client asks for the focus or grabs the keyboard or pointer; any client may send synthetic events.
- **Evidence for:** Menus, drag and drop and screen lockers are ordinary clients.
- **Evidence against:** DM-079: a client can take every key, including a password typed elsewhere, and inject keys. DM-072 is the terminal form of the same injection.
- **Cost:** *(estimate)* Small to build, impossible to secure later.
- **Precedent:** X11.

### Option B: Compositor-owned focus, click to focus (Wayland compositors)
- **Description:** The compositor sets keyboard focus when the person clicks a surface; clients cannot grab, and receive input only for their own surfaces (GR-016).
- **Evidence for:** Drivers 1, 3, 4, proven.
- **Evidence against:** Without a keyboard way to switch, a person with no pointer is stuck in one window.
- **Cost:** *(estimate)* 100 lines.
- **Precedent:** Weston, Mutter, KWin.

### Option C: Option B plus one reserved key
- **Description:** Click to focus and raise, and one key the compositor keeps, which moves focus to the next window.
- **Evidence for:** Drivers 1 to 4. The QMP-driven test can switch windows without a pointer.
- **Evidence against:** One key fewer for clients.
- **Cost:** *(estimate)* 120 lines.
- **Precedent:** Alt-Tab in most desktops `[outside evidence]`.

### Option D: Focus follows the pointer (Plan 9's rio by default)
- **Description:** In `rio` the window under the pointer is the one a click makes current, and its menu is the compositor's own (`alternatives-postmortem.md#plan-9--inferno` describes windows as file servers with their own devices).
- **Evidence for:** No reserved keys at all, and each window gets only its own input.
- **Evidence against:** Without a pointer nothing changes focus.
- **Cost:** *(estimate)* As Option B.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Keyboard focus** is one surface or none. It changes only when the person presses a pointer button over a surface (which also raises it), presses the reserved key, or the focused surface's client goes. A new surface takes the focus only if no surface has it. No message lets a client ask for focus.
- **The reserved key is F12**: on press it moves the focus to the next surface in creation order and raises it. Its press, repeat and release never reach a client. F12 was chosen because neither `zosh` nor a VT application needs it at this stage `[assumption]`.
- **Keys** go to the focused surface only, translated by the keymap (ADR-0092). Modifier state is the compositor's, so a key pressed in one window and released after a focus change gives the new window only the release of a key it never saw pressed; clients ignore such releases.
- **Pointer events** go only to the surface under the pointer, in its own coordinates, and only while the pointer is over it. A button press outside every surface changes nothing.
- **Delivery never blocks.** An event that does not fit a client's queue is dropped and counted; the next event that fits carries the count. A hung client loses its own input and nothing else.
- **Focus events.** A client gets `Focus { focused }` when it gains or loses keyboard focus.

**Tensions with accepted ADRs.** None.

### Effect of the vision
None on the choice. For target 2, an agent supervisor that wants to drive an agent's window needs synthetic input as a granted capability (DM-072, CP-24); it would enter through the compositor's manager side, never a client message, and this design leaves room for it. Target 3 wants configurable shortcuts, which turn the one reserved key into a table, at little cost.

### Deferred to stage 6
Nothing here depends on POSIX. Ctrl-C is a key like any other here; the terminal and the shell give it meaning (ADR-0096).

## Consequences
- **Positive.** A person can always switch windows, and no client can take keys typed elsewhere.
- **Negative.** No grabs, so no client-drawn menus that capture the pointer; F12 is unavailable to clients.
- **Follow-up.** Keyboard shortcuts as a table; synthetic input as a capability.

## Invariants
- **INV-0094-1.** The reserved key never reaches a client. *Check:* host test of the routing logic, and the GUI test presses F12 with a client watching.
- **INV-0094-2.** Focus changes only by a button press over a surface, the reserved key, or the focused client's end. *Check:* host test over every client message and event that the focus does not change otherwise.
- **INV-0094-3.** A client whose queue is full loses only its own events, counted, and the others get theirs. *Check:* host test with a fake channel that refuses writes for one client.

## Open questions
- Whether F12 suits a person: chosen without a measurement.

## Revisit triggers
- A client that needs a grab (a menu, drag and drop).
- A second reserved key.

## Architect's decision
