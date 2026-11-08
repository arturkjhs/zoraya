# ADR-0241: Round window corners over a client's pixels

- **Status:** proposed
- **Date:** 2026-11-08
- **Depends on:** ADR-0160, ADR-0161, ADR-0164
- **Evidence:** desktop/survey.md#display-scaling, desktop/survey.md#macos-and-apples-human-interface-guidelines

## Context
ADR-0161 (accepted) gives windows corners of `radius.window` in the shape `radius.shape`, "the same on all four
corners", and the chosen mockups draw every window so (`clear-field`). ADR-0160 (accepted) has the compositor draw the
frame around a client's body. ADR-0164's INV-0164-1 (accepted) says "the compositor never resamples a client's
buffer: every client pixel reaches the display unchanged". A window's lower corners lie over its body: drawing them
round means that the client's pixels in the corners' arcs are not shown, and the one pixel of the arc's edge is
blended with what is behind. Stage 7b's compositor does this; its independent review (TASK-24, review 7b, finding 15)
asked whether that breaks INV-0164-1. The invariant was written against scaling a buffer (Windows' stretched pixels,
`desktop/survey.md#display-scaling`), not against a window's shape, and is silent on corners. Measured: at scale 1 a
corner of `radius.window` 10 in the smooth shape spans 16 pixels; 204 of a corner's 256 pixels are shown unchanged,
and the others are hidden or blended at the edge.

## Decision drivers
1. Keep what ADR-0161 and the mockups decided: four round corners (`desktop/survey.md#macos-and-apples-human-interface-guidelines`).
2. Keep what INV-0164-1 protects: no client pixel is scaled or filtered.
3. Keep the screendump tests exact.

## Considered options

### Option A: Square lower corners (only the title bar's corners round)
- **Description:** The body is drawn as a rectangle; the frame rounds its top corners only.
- **Evidence for:** Every client pixel is shown; INV-0164-1 holds word for word.
- **Evidence against:** Against ADR-0161's "the same on all four corners" and every mockup.
- **Cost:** *(estimate)* None.
- **Precedent:** X11 window managers without compositing.

### Option B: Ask clients to round their own corners
- **Description:** `Configure` tells the client the corner radius; the client leaves the corners transparent.
- **Evidence for:** The compositor shows every pixel it gets.
- **Evidence against:** A client's pixels then decide the frame's look, against ADR-0160's frame drawn by the
  compositor; every client must do it right.
- **Cost:** *(estimate)* A field and work in every toolkit.
- **Precedent:** Wayland clients with client-side decorations.

### Option C: Round the window's shape over the body, and read INV-0164-1 inside the shape
- **Description:** The compositor clips the body to the window's shape (the arcs of the two lower corners), with the
  shape's antialiased edge; inside the shape every client pixel is shown unchanged. INV-0164-1 is read as "every
  client pixel inside the window's shape", which is the frame's own geometry, a function of the tokens alone.
- **Evidence for:** Drivers 1 to 3: nothing is resampled, the corners are the mockups', the arcs are deterministic.
- **Evidence against:** Strictly, a few pixels per lower corner are not shown as the client drew them.
- **Cost:** *(estimate)* What 7b built.
- **Precedent:** macOS, which rounds every window's lower corners over its content.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. It reads how two accepted ADRs meet; if the
architect prefers the invariant's letter, Option A is one line in `zoraya-compositor`'s `draw.rs`.

## Consequences
- **Positive.** The desktop looks as chosen; the scale invariant keeps its meaning.
- **Negative.** A client cannot put content into its lower corners' arcs.
- **Follow-up.** None, if accepted; Option A otherwise.

## Invariants
- **INV-0241-1.** Inside the window's shape, every client pixel of the body reaches the display unchanged; outside
  it, none does. *Check:* the compositor's host tests at scale 1 and 2 (`an_image_is_copied_unchanged_where_covered`
  in `zoraya-paint`, `at_scale_two_sizes_are_logical_and_pixels_unchanged` in `zoraya-compositor`).

## Open questions
- None.

## Revisit triggers
- A client that needs its corners (a full-screen video): a full-screen mode without a frame.

## Architect's decision
