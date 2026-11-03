# ADR-0165: The widget toolkit

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0002, ADR-0050, ADR-0051, ADR-0052, ADR-0093, ADR-0094, ADR-0109, ADR-0160, ADR-0161, ADR-0162, ADR-0164, ADR-0166
- **Evidence:** CP-15, CP-22, DM-048, DM-081, DM-082, GR-016, alternatives-postmortem.md#haiku--beos, alternatives-postmortem.md#redox, desktop/survey.md#own-no_std-widgets, desktop/survey.md#lvgl, desktop/survey.md#slint, desktop/survey.md#egui-and-iced, desktop/survey.md#what-makes-a-desktop-coherent

## Context
Stage 7's applications (ADR-0168) need buttons, text fields, lists, menus and dialogs that look alike (ADR-0161) and speak the compositor's protocol (ADR-0093, ADR-0160). The terminal draws its grid by hand into its memory object; a toolkit is the same thing generalised. The survey (`desktop/survey.md#own-no_std-widgets`, `desktop/survey.md#lvgl`, `desktop/survey.md#slint`, `desktop/survey.md#egui-and-iced`) found: LVGL is MIT C with a flush callback and an input callback, 32 kB of RAM at minimum, its own theme system; Slint's royalty-free licence excludes embedded systems and requires a disclosure in the product, and its other licences are GPLv3 or paid; `egui` and `iced` are permissive Rust toolkits that run on `winit` and `wgpu` and need a Rust `std` (ADR-0166 recommends none in stage 7); `egui` says a native look is not its aim and relayouts every frame. Haiku's coherence comes from one toolkit written with the system (`alternatives-postmortem.md#haiku--beos`); Redox runs a ported desktop over its libc after ten years (`alternatives-postmortem.md#redox`). Measured: nothing on Zoraya. Assumed: the widget set below is what the first five applications need.

## Decision drivers
1. One look from one token file, with no translation layer between the tokens and the pixels (ADR-0161, `desktop/survey.md#what-makes-a-desktop-coherent`).
2. Input handling obeys ADR-0094 and ADR-0160: a toolkit never asks for focus, position or a grab (GR-016, DM-082).
3. `no_std`, permissively licensed, host-tested, as every Zoraya crate (CLAUDE.md; ADR-0166 keeps `std` out of stage 7).
4. Memory safety in the code every application runs (CP-15, DM-048), and no parser of hostile markup in clients (DM-081's class).
5. UTF-8 text through ADR-0162 only (CP-22).
6. Small enough to write in one task: a dozen widgets, not a framework.

## Considered options

### Option A: A ported `std` toolkit (GTK or Qt on Linux; `egui` or `iced` here)
- **Description:** Port a Rust toolkit once a `std` exists (ADR-0166), with a backend that speaks ADR-0093's protocol instead of `winit` and draws with a software renderer instead of `wgpu`.
- **Evidence for:** Widgets, layout, text editing and accessibility work exist; `egui` and `iced` are MIT or Apache (`desktop/survey.md#egui-and-iced`).
- **Evidence against:** Needs a Rust `std` (ADR-0166 says not in stage 7), a backend written anyway, and a renderer that produces its own look: `egui` "is not for you" if a native look is wanted, and its theming is weaker than CSS, so driver 1 is not met; both are "experimental" or "work in progress".
- **Cost:** *(estimate)* The `std` port first, then a backend of a few thousand lines, then restyling.
- **Precedent:** Redox's COSMIC port (`alternatives-postmortem.md#redox`).

### Option B: LVGL in C
- **Description:** Build LVGL with ADR-0109's toolchain as a static library linked into native Rust programs through a C ABI, with a display driver that commits damage to the compositor and an input driver fed from `Key` and `Pointer` events (`desktop/survey.md#lvgl`).
- **Evidence for:** MIT; 30 widgets, flex and grid layouts, UTF-8 text with complex scripts; a porting model of two callbacks; proven on small machines.
- **Evidence against:** C in every client (driver 4, DM-048); its own theme and style system, so ADR-0161's tokens must be translated into LVGL styles (driver 1); its own text renderer beside ADR-0162's, or a bridge; the native desktop then depends on the POSIX toolchain's build (ADR-0109) for its own programs.
- **Cost:** *(estimate)* The build integration, a theme translator, a text bridge, and a Rust wrapper.
- **Precedent:** Embedded products; no desktop.

### Option C: Slint
- **Description:** A declarative UI language compiled to Rust, with a software renderer that can target a framebuffer.
- **Evidence for:** A declarative UI language (`.slint`) compiled to Rust, with a designed widget set, layout, animations and a software renderer that can target a framebuffer without an OS, which is the closest existing thing to driver 1's "one look from one file"; its `LICENSE.md` is explicit about what each licence allows (`desktop/survey.md#slint`).
- **Evidence against:** Its licences (`desktop/survey.md#slint`): the royalty-free one excludes embedded systems and requires disclosing Slint in the product; GPLv3 is not permissive; the commercial one is paid. CLAUDE.md allows MIT, Apache-2.0, BSD, ISC and Zlib only.
- **Cost:** Not available under the repository's rules.
- **Precedent:** Commercial embedded products.

### Option D: Zoraya's own `no_std` widgets, `zoraya-ui`
- **Description:** A retained-mode widget crate: a tree of widgets with a layout pass (rows, columns, fixed and flexible sizes on ADR-0161's 4-pixel grid), a paint pass into the window's memory object producing damage rectangles, and an event pass from `Key` and `Pointer` events; drawing through a small raster module (rectangles, rounded rectangles, lines, glyphs and icons via `zoraya-text`); every colour and size from `zoraya-tokens`. An `zoraya-app` crate holds the client loop over a `Platform` trait, as `zoraya-compositor` does, so an application is tested on the host against a fake compositor.
- **Evidence for:** Drivers 1 to 6. It is `zoraya-term`'s pattern (`desktop/survey.md#own-no_std-widgets`); the compositor already uses the same loop shape; the tokens and the text crate are the only inputs.
- **Evidence against:** Every widget written by hand; no accessibility tree, no rich text, no tables with editing in stage 7; a toolkit is never finished.
- **Cost:** *(estimate)* 3,000 lines for the first dozen widgets, layout, raster and tests; 500 for `zoraya-app`.
- **Precedent:** Haiku's Interface Kit and SerenityOS's LibGUI, each written with its system (`alternatives-postmortem.md#haiku--beos`, `desktop/survey.md#what-makes-a-desktop-coherent`).

## Recommendation
Recommended: **Option D**. It is a recommendation, not a decision.

- **Widgets in stage 7:** `Label`, `Button` (text, icon, or both as ADR-0161's icon rules allow), `TextField` (single line, selection, UTF-8 editing by character, the same editing rules as `zosh`'s line editor where they overlap), `CheckBox`, `Switch`, `RadioGroup`, `ListView` (rows of text and icon, selection, keyboard navigation), `ScrollView`, `Separator`, `MenuBar` and `Menu` (popups, ADR-0160), `Dialog` (a toplevel with buttons), `ProgressBar`. Each is a struct with a `layout`, `paint` and `event` method; composition is a tree owned by the application.
- **Layout:** `Row`, `Column` and `Grid` containers with `space.*` gaps and `padding.*`; sizes from content and from `control.height`; all in logical pixels, multiplied by the scale at paint time (ADR-0164). No constraint solver.
- **Painting:** into the window's memory object through a `Canvas` over `&mut [u32]` with a clip rectangle; every widget records the rectangles it changed, and the application commits their union (ADR-0093). Text and icons through `zoraya-text`. Double buffering is not needed: the compositor copies at commit.
- **Events:** the application receives `Key`, `Pointer`, `Focus`, `Configure`, `CloseRequested` and `Layout` (ADR-0093, ADR-0160, ADR-0163) as the event types of `zoraya-proto-gui`, which `zoraya-ui` depends on for those types alone (one vocabulary, no translation layer; the crate never touches a channel), and hands them to the tree; keyboard focus inside a window moves with Tab and Shift+Tab over focusable widgets in tree order, arrows inside lists and radio groups, Enter activates the default button, Escape closes a dialog or a menu. A widget never sends a message to the compositor; only `zoraya-app` does, and only the messages ADR-0093 and ADR-0160 define.
- **`zoraya-app`:** the loop (port, channel, memory objects, commits), window and popup creation, the `Platform` trait with a host fake, and the chooser client of ADR-0167. An application is `fn main` building a tree and handling messages, as `term` is today.
- **Text input:** `TextField` keeps a `String`, a cursor as a character index and a selection; insertion is `Key.text`; deletion removes one `char`; combining marks are separate `char`s in stage 7 (grapheme clusters are out of scope, as in ADR-0162), the same rules as ADR-0154 rule 7; copy, cut and paste are ADR-0170's.
- **Tests:** every widget has host tests that lay out, paint into a buffer and compare against the tokens (not against reference images, which the mockups are for): a button's rectangle has `bg.control`, its label `fg.text`, its radius `radius.control`; and event tests that drive keys and pointer through the tree against a fake compositor.

**Tensions with accepted ADRs.** *ADR-0093*: none; the toolkit uses the client messages as they are plus ADR-0160's. *ADR-0096*: the terminal keeps its own grid renderer and does not become a widget; it uses `zoraya-app` for its loop once that exists.

### Effect of the vision
None on the choice. For target 2 the toolkit runs inside the sandboxed client and is trusted by nothing outside it; a language runtime for agents would need bindings, which a Rust-only toolkit lacks until a second language is wanted (ADR-0052's open question applies). Target 3 wants accessibility and rich text: an accessibility tree is a walk over the widget tree exported through a manager-side capability (GR-016's open problem), which the retained tree makes possible and immediate-mode toolkits make hard. Threads: none; the loop is single-threaded. Networking: none.

### Deferred to stage 6
Nothing here depends on POSIX. A C program that wants a GUI would need C bindings; not in scope.

## Consequences
- **Positive.** One look by construction; applications are small; everything is host-tested.
- **Negative.** A toolkit to maintain; no accessibility or rich text in stage 7; Rust only.
- **Follow-up.** `zoraya-ui`, `zoraya-app`; the clipboard ADR; an accessibility capability later.

## Invariants
- **INV-0165-1.** No widget paints outside its layout rectangle intersected with the canvas clip, whatever its content. *Check:* host test painting every widget with hostile content (long text, zero size, negative scroll) into a guarded buffer.
- **INV-0165-2.** `zoraya-ui` sends no message to the compositor and `zoraya-app` sends only the ordinals ADR-0093 and ADR-0160 define; no code path asks for focus, position or stacking. *Check:* `zoraya-ui` has no dependency on `zoraya-rt` or `zoraya-channel` (a `Cargo.toml` fact checked by a host test over the dependency graph; it uses `zoraya-proto-gui`'s types only), and a host test of `zoraya-app` against a fake compositor recording every ordinal.
- **INV-0165-3.** Every colour, size, radius and duration a widget uses is a token (INV-0161-1 applies to `zoraya-ui`). *Check:* the INV-0161-1 script.
- **INV-0165-4.** Every focusable widget in a window is reachable with Tab, and Escape always closes an open menu or dialog. *Check:* host test over generated trees driving Tab around the cycle and Escape from every state.

## Open questions
- Whether a `Grid` is needed by the first applications, or rows and columns suffice: decided while building settings (ADR-0168).
- Whether `TextField` and `zosh`'s editor (ADR-0154, in the `no_std` crate `zoraya-shell`) share code: `TextField` follows ADR-0154 rule 7's text rules (UTF-8 in, invalid sequences refused, movement by scalar value) either way.

## Revisit triggers
- A second language for applications.
- A program that needs rich text or tables with editing.
- A Rust `std` for Zoraya (ADR-0166): reconsider Option A for ports, not for the system's own applications.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
