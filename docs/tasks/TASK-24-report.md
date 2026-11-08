# TASK-24 report: stage 7, the desktop

A running report, one section per part, updated after each (TASK-24, "Report"). The architect's queue (proposed ADRs,
decisions taken without the architect) is at the top; each part's section says what was built, the screens, the
measurements, the invariant delta, the review, the mutants, what was merged and the time it took.

## The architect's queue

| Item | Part | One-line recommendation |
|---|---|---|
| [ADR-0240](../adr/0240-fonts-embedded-until-the-launcher.md) (proposed) | 7a | Until 7d's launcher hands out `/system/fonts`, the programs `init` starts embed the unmodified fonts through `zoraya-fonts`; a gap ADR-0162 leaves, not a change of it. |
| Pushing, merging and tagging | all | Not done: this session's permission settings refused `git push` (twice, at the first push of `work/7a`), so no part could be merged into `main`, tagged or seen by CI. Each part is on its local branch, the next part starts from it (TASK-24, "If a part cannot be merged"), and the commands to push, merge and tag are under "To finish by hand" below. |

## 7a: tokens and text

Branch `work/7a`, from `main` at `522269a`.

### What was built

- **The two backlog fixes, each its own commit with a test.**
  - *The TCG key-burst loss.* Two places lost keys. `term` sent one message per key to `zosh` (or `ttyd`) and dropped
    every key that found that 64-message queue full; it now holds up to 4 KiB of keys, sends them together and waits for
    `WRITABLE` (`9f6a6aa`). With that fixed a TCG run still lost 29 of 90 keys of one burst, with no drop counted
    anywhere in the system: QEMU's own trace (`-trace virtio_input_queue_full`, one diagnostic run) showed the
    virtio-input device dropping events, 14 times. Its event queue holds 64 events, 16 keys (a press and a release, each
    with its `SYN_REPORT`), and QEMU drops what arrives while the guest has not handed buffers back; QMP injects a key in
    well under a millisecond, which no keyboard does and which a guest under TCG, busy composing, does not keep up with.
    The runner now presses one key every 8 ms, USB full speed's polling interval, so the queue holds 128 ms of typing
    (`d7960a2`). The GUI tests type whole lines again: `tests/gui/terminal.script`'s long line in one burst, the tour's
    lines whole (`zoraya-tour` no longer cuts them in pieces of 12), and a 64-key line through `ttyd` in
    `tests/posix/gui.script`.
  - *The runner's `EAGAIN` panic* (`7605991`). `tools/run-qemu.sh` gives QEMU stdin from `/dev/null` in every test run
    without `--keys` and in the EL2 probes, which ran QEMU in interactive mode with the terminal as stdin (the likely
    source: `make test`'s `EL2_OR_SKIP`); the runner prints through `say!`/`esay!`, which retry a write that would block
    and never panic (host tests in `tools/test-runner/src/say.rs`).
- **`zoraya-tokens`**: `tokens.txt` is `clear-field`'s file, byte for byte (a test compares them), read at compile time
  by a `const` parser into `Logical` sizes (constructors private to the crate), `Scale`, `Color`, `Millis`, easing curves,
  the shape choices and both palettes. A missing name or a bad value fails the build.
- **`zoraya-text`**: ADR-0162's Option C, 1,100 lines without tests: the total `glyf` reader, the 26.6 fixed-point
  rasteriser with exact integer coverage, the two-way glyph cache, the `Shaper` trait and the simple shaper, the box,
  clipped drawing, and reference digests checked on the host and in QEMU.
- **Fonts**: `fonts/` with Inter 4.1 Regular and Medium, JetBrains Mono 2.304 Regular and Bold and both licences,
  unmodified (fetched by `tools/mockups/fetch-fonts.sh`, checked against its archive hashes), in a fonts table of
  `docs/dependencies.md` that a test checks by SHA-256; `zoraya-fonts` gives them as bytes.
- **The terminal on the new text**: JetBrains Mono at `text.mono` (8 × 18 cells at scale 1, as the mockup's terminal),
  `padding.terminal`, the `term.*` colours with derived bright and faint forms, bold in the bold font.
  `zoraya_term::read` reads cells back exactly; the GUI runner uses it.
- **The compositor's three stage 5 colours** are tokens (`bg.desktop`, `border.strong`, `accent`), so that `make
  check-tokens` covers every GUI crate that exists. `zoraya-font` is deleted.

### The 7a acceptance test

`plan.md`: "`make test-gui` reads Ukrainian and Czech names typed into the terminal back from a screendump at scale 1;
the mutation fuzz of the font reader runs under `make test-unit`". Done, with one difference: Ukrainian and Czech
letters cannot be typed before 7c's layouts (the keymap is US, and QMP sends key codes), so the names come from files
in the image (`rootfs/texts/uk/звіт-2026.txt`, `rootfs/texts/cz/příklad.txt`), are put on the command line by Tab
completion, and are read back from the screen with their contents; 7c's test types them. The font reader's fuzz
(`truncated_and_mutated_fonts_never_panic_and_draw_inside`: every truncation of a synthetic font and seeded mutations
of it and of both real fonts, drawn against a guard border) runs in `make test-unit`.

### Decisions taken without the architect

- **Homoglyphs.** INV-0162-4 asks that every typed glyph be distinct in the terminal's font. Twenty pairs of Cyrillic
  and Latin letters (А A, В B, С C, Е E, Н H, І I, К K, М M, О O, Р P, Т T, Х X, а a, с c, е e, і i, о o, р p, х x,
  у y) are drawn alike by JetBrains Mono, as by any font that draws them correctly. The test asserts that exactly
  these classes are equal and every other pair distinct; the reader reads a class as its Latin letter and the runner
  compares a script's text in the same classes. Recorded here, not as an ADR: it is how INV-0162-4's check is built.
- **Terminal colours.** `term.black`..`term.white` are SGR 30..37; the default foreground and background are `term.fg`
  and `term.bg` (new indices, so that SGR 37 is not the light set's dark text). Bright forms are a quarter of the way
  to `term.fg`, faint half way to `term.bg` (ADR-0161 lets the terminal derive them).
- **The grid** stays 80 × 24 (ADR-0096); the mockup's terminal window is wider (107 columns). 7b's `Configure` resizes
  it.

### Screens

In `docs/research/desktop/shots/7a/` (108 KB), all at scale 1:

| File | What | Against the mockup |
|---|---|---|
| `terminal-qemu.png` | The whole display after `tests/gui/terminal.script`'s Ukrainian and Czech steps, a QEMU screendump (`make test-gui`, HVF; the TCG dump is byte for byte the same). | `clear-field/terminal.png`: the same type (JetBrains Mono at 13 px), the same 8 × 18 cell and 14-pixel inset, `term.bg` and `term.fg`. Not yet: the frame, title bar, room tint and grant pills (7b, 7d), the bar and the dock (7b, 7d), the wallpaper (7b), and the wider window (107 columns in the mockup; 80 here until 7b's `Configure`). The background and the one-pixel frame are stage 5's in the tokens' `bg.desktop` and `accent`. |
| `terminal-qemu-text.png` | The terminal's surface alone, cut from the same dump. | Glyph for glyph as the mockup draws its terminal: same rasterising model (8-bit coverage, no hinting, whole-pixel origins), the system's in fixed point where the tool's is `f32`, so single pixels of coverage can differ by one level. |
| `terminal-host-light.png`, `terminal-host-dark.png` | The terminal in both sets, rendered on the host by the same crates (`cargo test -p zoraya-term -- --ignored samples`): red, green, blue, magenta, bold, faint, the cursor. The dark set cannot be shown in QEMU before 7f's settings. | The dark set's `term.*` colours as in `terminal-dark.png`; the prompt colours of the mockup (the room-coloured `project ~/project>`) are the shell's, not yet built. |
| `text-sample.png` | Inter at 13 and 20 px and JetBrains Mono at 13 and 26 px with Ukrainian and Czech text and the box glyph, from `zoraya-text` on the host. | The mockups' fonts at the token sizes. |


### Measurements

| | Host (M4, release) | HVF (debug, test kernel) | TCG (debug, test kernel) |
|---|---|---|---|
| A full 80 × 24 grid of Cyrillic, empty glyph cache | 0.52 ms | 0.47 to 0.57 ms | 4.5 to 4.6 ms |
| The same from the cache | 0.30 ms | 0.36 to 0.44 ms | 3.7 to 3.8 ms |
| Cache after two grids | 64 misses, 3,776 hits | the same | the same |

From `t_text` in every `test-qemu` configuration of one `make test` (three HVF, four TCG) and `zoraya-text`'s
`grid_cost` on the host. The rasteriser is fast enough and deterministic under TCG (the reference digests match
in every configuration, and the HVF and TCG screendumps are identical), so no proposed ADR and no conservative path
was needed. Kerning: Inter keeps its kerning in GPOS only and JetBrains Mono has none, so `kern` format 0 kerns
nothing in the system's fonts (ADR-0162's open question; GPOS pair adjustment would be a later addition).


### Invariants

| Invariant | Before | After |
|---|---|---|
| INV-0161-1 | not yet applicable (stage 7) | checked: `make check-tokens` |
| INV-0161-2, -3, -5, -6 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0161-4 | not yet applicable (stage 7) | not yet applicable (stage 7): needs `zoraya-ui` (7d) |
| INV-0162-1, -3, -5 | not yet applicable (stage 7) | checked: `make test-unit` |
| INV-0162-2 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-qemu` (`t_text`) |
| INV-0162-4 | not yet applicable (stage 7) | checked: `make test-unit` and `make test-gui` |
| INV-0095-1, -2 | checked: `make test-unit` | replaced by INV-0162-1 and -4 (`zoraya-font` deleted) |
| INV-0240-1 (new, proposed ADR) | | checked: `make check-tokens` |

### Review

`docs/tasks/TASK-24-review-7a.md`: 4 errors (three panics on hostile fonts and sizes, reproduced by the reviewer;
one invariant status that claimed too much), 5 warnings, 4 notes. All fixed in `831a350`, with tests; the check of the
changed lines found no open error. One more round was not needed.


### Mutation testing

MEASURE_MUTANTS

### Merge

Not merged: see the queue. `work/7a` is complete locally; 7b starts from it.

### Time

MEASURE_TIME

## To finish by hand

`git push` was refused in this session, so these steps are the architect's (or a session allowed to push), in order,
for each part that is complete:

```
git push -u origin work/7a
# merge as TASK-24 says: from a detached HEAD at origin/main
git fetch origin && git switch --detach origin/main
git merge --no-ff work/7a -m "Merge work/7a: stage 7a, tokens and text (TASK-24)"
git push origin HEAD:main          # then wait for every CI job
git tag stage-7a && git push origin refs/tags/stage-7a
```

To try the desktop by hand: `make run-gui`; the window shows the terminal in JetBrains Mono. `ls /texts/uk`, `cat
/texts/uk/` then Tab and Enter, and the same for `/texts/cz/`, show Ukrainian and Czech text; Shift with Page Up
scrolls back.

