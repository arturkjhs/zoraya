# ADR-0163: Keyboard layouts and non-ASCII input

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0052, ADR-0060, ADR-0092, ADR-0093, ADR-0094, ADR-0096, ADR-0160, ADR-0169, ADR-0171
- **Evidence:** CP-22, CP-21, DM-069, DM-097, GR-016, alternatives-postmortem.md#plan-9--inferno, desktop/survey.md#keyboard-layouts, desktop/survey.md#shaping-and-what-latin-and-cyrillic-need

## Context
ADR-0092 put one layout, US, in the compositor's `zoraya-keymap` crate, and a focused client receives `{code, pressed, modifiers, text}`. Stage 7 needs at least US, Ukrainian, Russian and Czech layouts, a way to switch between them, and a path for non-ASCII text into the GUI terminal and `zosh`. The survey records the reference layouts (`desktop/survey.md#keyboard-layouts`): xkeyboard-config's `cz` basic (QWERTZ, the number row giving `ě š č ř ž ý á í é`, `ů` and `ú` on their own keys, dead acute and caron on `=`), `ua` unicode (ЙЦУКЕН with ї і є ґ and the apostrophe), `ru` winkeys (ЙЦУКЕН with ё ы ъ э), and `us`. XKB's model is a table of key to keysym per level and group, with groups switched at run time and dead keys handled in a separate compose state machine on keysyms; Wayland sends every client the whole keymap and raw codes; macOS switches with the Input menu or Control-Space; Haiku keeps keymaps as editable files. Every keysym these layouts produce is one precomposed code point (`desktop/survey.md#shaping-and-what-latin-and-cyrillic-need`). Measured: nothing on Zoraya. Assumed: QEMU's `virtio-keyboard-device` delivers the same evdev codes for the same physical keys whatever the host's layout, which is how the stage 5 US layout works today.

## Decision drivers
1. One place for the layout, so that a client cannot be told a different one and every client gets the same text (ADR-0092 driver 4, GR-016).
2. Text reaches clients as UTF-8 code points, never as a layout-dependent code (CP-22).
3. The person switches layouts the same way everywhere and sees which one is active (`desktop/survey.md#keyboard-layouts`, macOS and Haiku).
4. Dead keys and switching never lose or duplicate a character or a modifier.
5. No translation layer far from its consumer (DM-069) and no process-global input state in libraries (CP-21, DM-097).
6. Small: tables, not a keymap compiler.

## Considered options

### Option A: XKB keymaps compiled in the compositor and sent to every client (Wayland with libxkbcommon)
- **Description:** The compositor loads xkeyboard-config's layout files, compiles them, and sends each client the keymap as a file descriptor; the client runs its own `xkbcommon` state and compose machine from raw codes (`desktop/survey.md#keyboard-layouts`).
- **Evidence for:** Every layout in the world, with the dead-key and compose data to match; the client can show a key's label.
- **Evidence against:** A keymap compiler and a state machine in every client, against drivers 1 and 6; the layout lives in two places (ADR-0092 Option A's objection); a sandboxed client receives a file it must parse (DM-069's layer, now on the client side).
- **Cost:** *(estimate)* A port of a compiler and a parser, or thousands of lines.
- **Precedent:** Every Wayland compositor and toolkit.

### Option B: Layout tables in the compositor; clients get text (ADR-0092 extended)
- **Description:** `zoraya-keymap` holds one table per layout: for each evdev code, the code point at level 1 (plain), 2 (Shift), 3 (AltGr) and 4 (Shift+AltGr), or a dead key, or nothing. The compositor keeps the active layout, the modifier state and a one-entry dead-key state, and sends the focused client `Key { code, pressed, modifiers, text }` where `text` is the resulting code point. Switching is a reserved chord (ADR-0160's system key with Space) and a setting (ADR-0169).
- **Evidence for:** Drivers 1 to 6. The four layouts are a few hundred rows; the dead-key machine is the Compose rule for the pairs the layouts need (`desktop/survey.md#keyboard-layouts`); clients already consume `text`.
- **Evidence against:** A client cannot show the label of a key under the active layout (a keyboard viewer would need a query); per-window layouts are not possible; a layout a person wants is a change to the crate until layouts become files.
- **Cost:** *(estimate)* 400 table rows, 150 lines of state machine, 100 lines of switching and settings.
- **Precedent:** ADR-0092; Plan 9's kernel table of runes (`alternatives-postmortem.md#plan-9--inferno`), moved to the compositor.

### Option C: Per-client or per-window layout (Windows' per-thread input language)
- **Description:** Each window remembers its own layout; switching changes the focused window's.
- **Evidence for:** A terminal in Ukrainian and an editor in Czech at once.
- **Evidence against:** The person must know which window has which layout, which the survey's precedents show with an indicator per window; the compositor still holds the tables, so it is Option B plus a per-window field and a per-window indicator.
- **Cost:** *(estimate)* Option B plus 50 lines and an indicator.
- **Precedent:** Windows; macOS's "automatically switch to a document's input source" option.

### Option D: Layouts as editable files loaded by the compositor (Haiku's keymap files)
- **Description:** Option B's tables in a text file format a person can edit, loaded at start from the system image and from the settings (`desktop/survey.md#keyboard-layouts`).
- **Evidence for:** A person adds a layout without a rebuild.
- **Evidence against:** A parser of hostile input in the compositor for a stage with four layouts; the tables are also what the tests check.
- **Cost:** *(estimate)* Option B plus a parser and its fuzz test.
- **Precedent:** Haiku's `keymap -d` and `-s`.

## Recommendation
Recommended: **Option B**, with Option D's file format as the follow-up once a fifth layout is wanted. It is a recommendation, not a decision.

- **Tables.** `zoraya-keymap` gains `Layout` tables for `us`, `ua` (xkeyboard-config's "unicode" default), `ru` ("winkeys") and `cz` (QWERTZ "basic"), written in the crate from the reference files (`desktop/survey.md#keyboard-layouts`), each row `(code, [level 1, level 2, level 3, level 4])` of `KeyOutput::{Char(char), Dead(Accent), None}`. Levels: 1 plain, 2 Shift, 3 AltGr (right Alt), 4 Shift+AltGr (`desktop/survey.md#keyboard-layouts`); Caps Lock applies to letters only: it inverts Shift for rows whose level 1 and 2 are a case pair `[outside evidence: XKB's alphabetic key type, not in the survey]`. The crate stays total: every code below `0x300` has a row, the missing ones `None` (INV-0092-3 carries over).
- **Dead keys.** `Accent::{Acute, Caron, Ring, Diaeresis, Circumflex, Grave, Tilde, Breve, Ogonek, Dot, DoubleAcute, Cedilla, Macron}` with a table of (accent, base) to precomposed code point, covering the Latin-1, Latin Extended-A and B letters; a dead key followed by a base in the table gives the composed letter; followed by Space gives the accent's spacing character; followed by anything else cancels and the key is passed through (one of the three cancelling behaviours xkbcommon offers, `desktop/survey.md#keyboard-layouts`; chosen because it loses nothing the person typed); a second dead key replaces the first. The state is in the compositor, per keyboard, cleared on focus change.
- **Events.** `Key.text` stays one `char` on the wire (a `u32` code point, 0 for none), which already carries any code point; nothing in ADR-0093's message changes. The compositor adds `Layout { name }` events to the focused client at focus time and on every switch, so a client can show the layout, and tells the panel (ADR-0168) over its manager channel, which shows the active layout's short name beside the clock, so the person sees it without a client's help (the mockups show it there).
- **Switching.** System+Space cycles the enabled layouts in order; System+Shift+Space the other way. The enabled list and the one active at start are settings (ADR-0169: `gui.keyboard-layouts = us ua cz`, a list in ADR-0171's type with the items checked against this crate's names, in switching order, and `gui.keyboard-layout = us`, one of them, in ADR-0157's file); the active layout is one for the whole session, not per window (Option C is a later field). Switching clears the dead-key state and nothing else: a key pressed under one layout releases under the new one with the same `code`, which is why clients ignore releases they did not see pressed (ADR-0094).
- **Terminal and `zosh`: agreement with ADR-0154, point by point.** The terminal already encodes `text` as UTF-8 bytes into `Input` (ADR-0096); with Cyrillic `text` the bytes are two per letter. ADR-0154 rule 7 (accepted) already has the editor take UTF-8, refuse an invalid sequence, move "by Unicode scalar value" and count "one column for each", so erasing a two-byte letter and placing the cursor need nothing from this ADR; names compare byte for byte (ADR-0013). Two things follow for the terminal, not the shell: (1) a combining mark is one scalar value to the editor and one column, so ADR-0162's terminal gives it a cell of its own, drawn at the cell's start, and the editor's count and the screen agree (in proportional text ADR-0162 draws it over its base); (2) ADR-0154's stated limitation, that a wide character can misalign the cursor on the serial line, holds in the GUI terminal too, where ADR-0162 draws it clipped in one cell. On the serial console (ADR-0060) the same bytes arrive from the host terminal, so one editor serves both, as ADR-0154 requires (INV-0154-4).
- **Repeat and Caps Lock LED.** Repeat stays QEMU's (ADR-0092); the status queue for the LED stays follow-up.

**Tensions with accepted ADRs.** *ADR-0092* says "one layout for the whole system" and "a layout change needs the compositor"; the first changes to "one active layout, several enabled", the second stays true. *ADR-0096* keys-to-bytes gains nothing new: printable text was already UTF-8. *ADR-0154*: none; its rule 7 already edits by scalar value, which is what this ADR needs, and INV-0154-1 (the line buffer stays valid UTF-8) covers the bytes the terminal sends.

### Effect of the vision
None on the choice. For target 2, synthetic input stays a manager-side capability (ADR-0094) and a layout is never something a client can set. Target 3 needs input methods (CJK), which are a program between the compositor and the focused client holding a granted capability, and Option D's files; neither changes the `Key` event. Networking and threads: none.

### Deferred to stage 6
Nothing here depends on POSIX. A POSIX program on the terminal reads UTF-8 bytes through `ttyd` (ADR-0105); musl's multibyte functions in the C locale pass UTF-8 through unchanged (ADR-0100).

## Consequences
- **Positive.** Four layouts, dead keys, one switch chord and one indicator, with no change to the client protocol's `Key` event.
- **Negative.** Layouts are code until Option D; one layout per session; no key labels for clients.
- **Follow-up.** The tables and the dead-key table; the `gui.keyboard-layouts` and `gui.keyboard-layout` settings (ADR-0169); the panel's indicator.

## Invariants
- **INV-0163-1.** Translation is total over every layout: every code below `0x300`, every modifier state and every dead-key state gives exactly one `KeyOutput`, never a panic. *Check:* exhaustive host test over codes, states and layouts.
- **INV-0163-2.** Every letter the reference layouts produce is produced here: the Czech number row, `ú` and `ů` on their own keys, `ď ť ň` through the dead caron and `ó` through the dead acute, the Ukrainian `ґ є і ї` and apostrophe, the Russian `ё ы ъ э`, in both cases. *Check:* a host test with the expected code points per key and level, written from the reference files.
- **INV-0163-3.** A dead key never emits more than one character and never loses the base: for every (accent, base) pair the output is the composed letter if the table has it, else the base alone; a dead key followed by Space gives the spacing accent. *Check:* host test over every accent and every base code point.
- **INV-0163-4.** A layout switch changes no modifier or button state and reaches no client as a key. *Check:* host test switching with Shift held and with a key held, asserting the client's events; the GUI test types `ls` after switching to `ua` and back.

## Open questions
- Whether the Mac keyboard under QEMU delivers the right Alt as `KEY_RIGHTALT` so that AltGr works: checked by the first stage 7 task; if not, the Option key mapping is `tools/run-qemu.sh`'s concern.
- Whether Czech people on a Mac expect the QWERTZ `basic` or the `qwerty-mac` variant: both are tables; the setting names which. Not measured.
- Whether to import the reference files as test fixtures: their licence was not fetched; the tests are written by hand from them instead.

## Revisit triggers
- A fifth layout or a person's own: Option D.
- An input method for CJK.
- A request for per-window layouts.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
