# ADR-0243: A room's tint on the frame, not on the client's pixels

- **Status:** accepted
- **Date:** 2026-11-08
- **Depends on:** ADR-0160, ADR-0161, ADR-0164, ADR-0172, ADR-0241
- **Evidence:** desktop/survey.md#spoofing-and-the-trusted-path, desktop/survey.md#eross-trusted-window-system

## Context
The chosen variant (`clear-field`, `room.style = ground`) tints a room's window: "the body at 0.10, the title bar at
0.18" (mockups' README, The Field variant). ADR-0172 (accepted) has the compositor draw the room's colour "on the
frame, in the measure the direction chooses (`room.style`: a thin edge, the tinted title bar or the tinted window
ground)". The body is the client's pixels: ADR-0164's INV-0164-1 (accepted) and the proposed ADR-0241 say every client
pixel inside the window's shape reaches the display unchanged. A compositor that blends a colour over the body changes
every client pixel by up to a tenth; a client that wants its ground in the room's colour cannot draw it either, since
INV-0172-1 forbids a client stating a room (`desktop/survey.md#spoofing-and-the-trusted-path`). Measured: nothing.

## Decision drivers
1. A client's pixels are shown as the client drew them (INV-0164-1, ADR-0241).
2. The room is told by pixels the compositor alone draws (INV-0172-1, `desktop/survey.md#eross-trusted-window-system`).
3. Closest to the mockups that drivers 1 and 2 allow.

## Considered options

### Option A: Blend the room's colour over the body, as the mockups draw it
- **Description:** The compositor blends `room.N` at 0.10 over every body pixel of a room's window.
- **Evidence for:** The mockups exactly.
- **Evidence against:** Driver 1: every client pixel changes; text in the body loses contrast against what the client
  chose.
- **Cost:** *(estimate)* A blend per body pixel per composed frame.
- **Precedent:** Qubes OS tints borders, not contents `[outside evidence]`.

### Option B: The tint on the frame only: the title bar at 0.18, the body untouched
- **Description:** The title bar (the compositor's) carries the room's tint, the badge and the pills; untrusted
  windows also carry their edge (ADR-0172 point 6); the body is the client's.
- **Evidence for:** Drivers 1 and 2; the room is still on every window and in the fields.
- **Evidence against:** Less colour than the mockups: a room's window body is neutral.
- **Cost:** None beyond the title bar.
- **Precedent:** Qubes OS's coloured frames `[outside evidence]`.

### Option C: Tell the client its room's ground colour (`Configure`)
- **Description:** The compositor sends the tinted ground colour; the client paints its own ground in it.
- **Evidence for:** The mockups' look with unchanged pixels.
- **Evidence against:** Driver 2: a client could paint any ground and claim a room by colour.
- **Cost:** *(estimate)* A field in `Configure`, every application paints it.
- **Precedent:** None known.

### Option D: Unix's baseline: the window manager draws a coloured border, the client its contents
- **Description:** X11 window managers colour the decoration.
- **Evidence for:** What Option B is.
- **Evidence against:** None beyond Option B's.
- **Cost:** As B.
- **Precedent:** Every X11 window manager.

## Recommendation
Recommended: **Option B**, as built in stage 7d. It is a recommendation, not a decision; it reads ADR-0172's
`room.style = ground` as the title bar's ground, the only ground the compositor owns.

## Consequences
- **Positive.** Client pixels unchanged; the room is on every frame.
- **Negative.** The mockups' tinted body is not drawn.
- **Follow-up.** None, unless the architect prefers Option A.

## Invariants
- **INV-0243-1.** A room's colour reaches a window's body only through the untrusted edge (ADR-0172 point 6): every
  other body pixel is the client's. *Check:* the compositor's host test that a room's window shows its body pixels
  unchanged inside the edge.

## Open questions
- Whether the architect wants the body tint at the cost of INV-0164-1 (Option A).

## Revisit triggers
- A measured confusion between rooms with neutral bodies.

## Architect's decision

Accepted 2026-11-09. Decided by the architect; recorded by an agent at the architect's request.

Accepted: the room's tint on the frame only; the compositor never changes client pixels.
