# ADR-0244: The toolkit's own two events, translated by `zoraya-app`

- **Status:** accepted
- **Date:** 2026-11-08
- **Depends on:** ADR-0093, ADR-0160, ADR-0164, ADR-0165
- **Evidence:** desktop/survey.md#own-no_std-widgets, desktop/survey.md#egui-and-iced

## Context
ADR-0165 (accepted) says the application hands `zoraya-proto-gui`'s event types to the widget tree, "which `zoraya-ui`
depends on for those types alone (one vocabulary, no translation layer; the crate never touches a channel)". Widgets
need two things from those events: a key press with its text and modifiers, and a pointer position with its buttons
in logical pixels (ADR-0164). The protocol's messages carry more (a window id, a `dropped` count, releases, legacy
`Pointer` in buffer pixels, `Configure`, `Layout`), which belong to the window and the loop, not to a widget. Built in
stage 7d: `zoraya-ui` has an `Event` of two variants (`Key { code, text, modifiers }`, `Pointer { x, y, buttons }` in
`Logical`), and `zoraya-app` turns `Key`, `WindowPointer`, `WindowFocus`, `Configure` and `CloseRequested` into
widget events or into its own work. Measured: nothing.

## Decision drivers
1. Widgets see only what they act on, in logical pixels (ADR-0164), so a widget cannot depend on a window id or a
   physical size.
2. `zoraya-ui` stays free of any protocol crate, so INV-0165-2's first half holds by its manifest.
3. One place turns protocol into widget events: `zoraya-app`.

## Considered options

### Option A: The protocol's types in the widgets, as ADR-0165 says
- **Description:** `Widget::event` takes `zoraya_proto_gui::Key` and `WindowPointer`.
- **Evidence for:** No second type; ADR-0165 as written.
- **Evidence against:** Widgets see window ids, releases and `dropped`; the pointer's units are the protocol's.
- **Cost:** A dependency of `zoraya-ui` on `zoraya-proto-gui` and `zoraya-protocol`.
- **Precedent:** `egui`'s raw input passes the platform's events through (`desktop/survey.md#egui-and-iced`).

### Option B: Two widget events, translated by `zoraya-app`
- **Description:** As built.
- **Evidence for:** Drivers 1 to 3.
- **Evidence against:** A small translation layer (40 lines in `zoraya-app`).
- **Cost:** Built.
- **Precedent:** Toolkits with their own event types over a platform layer (`desktop/survey.md#own-no_std-widgets`).

### Option C: Unix's baseline: the toolkit reads the display protocol itself
- **Description:** GTK and Qt read Wayland or X11 events in the toolkit.
- **Evidence for:** No application loop crate.
- **Evidence against:** The toolkit would touch a channel, which ADR-0165 forbids.
- **Cost:** -
- **Precedent:** GTK, Qt.

## Recommendation
Recommended: **Option B**, as built. It is a recommendation, not a decision; it changes only where ADR-0165 says the
types come from, not what the toolkit sends (nothing) or how it behaves.

## Consequences
- **Positive.** Widgets are independent of the protocol; the tests drive them with two simple events.
- **Negative.** A translation in `zoraya-app` that must follow the protocol when it changes.
- **Follow-up.** None.

## Invariants
- **INV-0244-1.** `zoraya-ui` depends on no protocol or channel crate. *Check:* `zoraya-ui`'s host test over its
  `Cargo.toml` (`the_toolkit_sends_nothing`).

## Open questions
- None.

## Revisit triggers
- A widget that needs a protocol event the two do not carry (a drop, ADR-0170).

## Architect's decision

Accepted 2026-11-09. Decided by the architect; recorded by an agent at the architect's request.
