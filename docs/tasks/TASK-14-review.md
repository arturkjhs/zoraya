# TASK-14 review: stage 5, GUI

An independent review by a subagent that did not write the code (on the main model, as `CLAUDE.md` asks), of
`git diff main...stage-5` at commit `a9ae1a4`: ADR-0090 to ADR-0097, the new crates, the GUI programs, `init`, `zosh`,
the runner's GUI mode and the docs. The reviewer ran the host tests of every new crate, and confirmed finding R2 with a
scratch program outside the repository. Then mutation testing of the new crates, sorted by the `mutant-triage`
subagent. Fixes are in commit `b82926f` (review) and the commit after it (mutation holes).

## Findings and dispositions

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | warning | `gpud` took the device's interrupt down only before waiting, never after a wake. A completion that lands while the line is still high from the last one makes no edge, since virtio-mmio is edge-triggered on QEMU `virt`. This was the "missed interrupt" under TCG that ADR-0090 said had no known cause. The poll message also printed on any slow wait. | Fixed. `Gpu::call` acknowledges the device and looks at the queue again before every wait. After the fix, three TCG runs of `t_gui` and one of the criterion script never fired the 10 ms guard. The guard stays, and its message now says only that a wait passed 10 ms. ADR-0090's open question, `design.md` and the backlog now state the cause. |
| R2 | warning | INV-0096-3 was violated. In `ESC ]` followed by many `ESC x` pairs, the escapes did not count towards the string bound, so the terminal swallowed all later output. A CSI had no bound at all. | Fixed. Every byte of a string counts, and a CSI is cut at the same bound. Both patterns are now in `memory_is_bounded`. |
| R3 | warning | INV-0097-2's in-QEMU check would pass with the bug. `flush_ns` grows with no display at all, and a reattached input driver was only seen to be running. | Fixed. The compositor counts the flushes a driver answered (`StatsReply.flushes`). `Gui::restart_gpu` and `restart_input` send with a transaction and return true only on the compositor's `OK`. `t_gui` checks both. The invariant's text now says what is checked: the new channel is accepted, and the new GPU driver answers a flush. Input delivered through a restarted real driver would need QMP inside the test kernel's run; it is not checked. |
| R4 | warning | An `Attach` that failed after `RESOURCE_CREATE_2D` unpinned memory the device might still hold as backing, and resource 1 stayed, so the driver could never attach again. | Fixed. A failure before any command unpins and refuses. A failure after it is a protocol break: reset, then unpin, then exit. ADR-0090 now says so. |
| R5 | note | A device error response leads to a status, not a reset, which differs from the ADR's wording. INV-0090-3's check covers only the protocol break. | Fixed in text. ADR-0090 states the error policy, and INV-0090-3's check says the reset and exit in `gpud` are checked by reading. |
| R6 | note | Driver restart counts started again with every GUI restart. | Fixed. `init` carries the counts across GUI restarts. ADR-0097 says "counted across restarts of the GUI". |
| R7 | note | A failed `Gui::start` left its drivers running. | Fixed. They are ended. |
| R8 | note | The compositor blocks on its display driver for up to 2 s per call, which ADR-0093 did not say. | Recorded in ADR-0093 ("Driver calls block"), in `design.md` known limitations, and in the backlog as follow-up. |
| R9 | note | The GUI script's F12 step could not catch F12 reaching a client. | The script's comment now says so. `t_gui` and the host tests are the checks of INV-0094-1. |
| R10 | note | Small points: (a) the input frame's dropped count is ignored; (b) a dropped reply was counted as a dropped event; (c) NUL cannot be typed; (d) modifiers stay stuck when an input driver dies; (e) the old framebuffer stays mapped on a resize. | (b) and (d) fixed. (a) and (c) are stated in ADR-0092. (e) was already documented. |
| R11 | note | ADR-0093 driver 4 ("nothing half-drawn") is not met when a client writes the rectangle being copied. The argument against Option B was partly a strawman. | Reworded. Driver 4 is now "never shows pixels a client has not committed", and Option B's evidence states that both models tear in that case. |
| R12 | note | ADR-0091 described `init` reading a page with one mapping, while the code maps range by range. ADR-0090 leaned on GR-021 for "the device is untrusted". | Both reworded. |
| S1 | suspicion | A `gpud` start-up longer than 2 s under load would be dropped by `compd`. | Covered by R8's record. Not seen in any run. |
| S2 | suspicion | `term` cleared its dirty rows before a commit write that could fail. | Fixed. A failed write marks every row for the next render. |

The reviewer found these sound: rectangle arithmetic and indexing, INV-0093-4's host test, the kernel's refusal of a
mapping longer than its object (on which INV-0093-3 rests), handle closing on every path, the input driver's checks
and wait order, font clipping, `identify`, the queue's checks, the SAFETY comment of `copy_from`, and `zosh`'s console
rights. The "Effect of the vision" sections were judged honest.

## Mutation testing

`make mutants-host CRATE=…` (cargo-mutants 27.1.0) on commit `a9ae1a4`. The `mutant-triage` subagent sorted the
survivors.

| Crate | Mutants | Caught | Missed | Unviable | After the new tests |
|---|---|---|---|---|---|
| `zoraya-compositor` | 324 | 270 | 50 | 4 | AFTER_COMPOSITOR |
| `zoraya-term` | 317 | 255 | 58 | 4 | AFTER_TERM |
| `zoraya-virtio-gpu` | 111 | 89 | 10 | 12 | AFTER_GPU |
| `zoraya-virtio-input` | 110 | 92 | 12 | 6 | AFTER_INPUT |
| `zoraya-font` | 89 | 70 | 3 | 16 | AFTER_FONT |
| `zoraya-keymap` | 43 | 34 | 8 | 1 | AFTER_KEYMAP |
| `zoraya-virtio` | 485 | 373 | 101 | 11 | AFTER_VIRTIO |

- **`zoraya-virtio`.** 56 of the 101 survivors are in `fake.rs`, the device-side fake that tests use, and are left: it
  is test code. 25 were in the new default `read8` and `write8` of `Registers`, which every implementation in the
  tests overrode; `default_byte_access_uses_the_aligned_word` covers them. 8 are the stage 4 equivalents in the blk
  driver (backlog, "Mutation testing"). 12 were in `queue.rs`: 6 are equivalent, because `|` and `^` agree on the
  disjoint status bits, and 6 were holes, now tested: FAILED kept on a failed set-up, `num_max` equal to the size, a
  three-buffer chain, and addresses above 4 GiB.
- **Holes, now tested.** In the compositor: the frame's edges, a display that closes unasked, wrong `Info` answers, a
  refused `Attach`, a display of a new size, one-pixel displays, the manager's handle count, an object too small by a
  few pages, F12 by creation order, placement at the corner, part of a surface composed from its own pixels, the
  dropped count of `Pointer` and `Focus`, the flush rectangle of a closed client, the last axis value, every button,
  and raise on click. In the terminal: the padding in the sizes, row addressing, memory, redraw on scroll and cursor
  moves, 4-byte UTF-8, `ESC c`, intermediates and control bytes inside a CSI, a third parameter, more than eight
  parameters, relative moves, SGR 22, 27, 39, 49 and each colour, EL 2 and ED 0 and 1 below row 0, and every named
  key. In the GPU driver: the reply buffer's size, the layout's size, `size()`, a short error response, and a run of
  exactly `u32::MAX`. In the input driver: devices with one axis or none, axes with different ranges, an 8-byte axis
  info, and the layout's size. In the font: a pixel in the right margin. In the keymap: Ctrl and Alt on both sides,
  Caps Lock released, and Ctrl with `[` and `]`.
- **Equivalent, left on purpose** (the triage's reasons, checked against the code):
  - compositor: key ranges and guards that differ only on keys no `watch` registers; the display's `Empty` arm, which
    a single reader woken for readable-or-closed never sees; `!attached` beside a size of 0; `room < 1` against
    `<= 1`, which give the same 1; and `compose`'s length check, where a framebuffer is exactly `w × h`.
  - terminal: `|` against `^` when building UTF-8; `i < count` against `<=`, where `params[count]` is always 0.
  - input driver: `8 * BUFFERS` against `+`, which round up to the same page; `<=` in `config_read`, whose one extra
    byte nobody reads; `size > 0` against `>=`.
  - font: the early two-colour return, which the exact check after it repeats; and `unwrap_or(!bg)` against `bg`,
    which only the blank glyph reaches.

## Check of the fixes

One check of the changed lines only (`git diff a9ae1a4..4a585a9`), by a subagent that wrote none of them. It ran the
host tests of every changed crate and `adr-check`, and found 3 errors.

| Id | Finding | Disposition |
|---|---|---|
| C1 | error: `gpud`'s comments still said the cause of the missed interrupt was not found, and `MISSED`'s doc contradicted its new message. | Fixed. |
| C2 | error, a regression from R3: `manager_send` waited 2 s, but the compositor's attach of a new display driver makes two driver calls of up to 2 s each. `init` would then leave the new `gpud` unwatched, and the late reply would be taken for the reply to the next request, because replies were not matched by transaction. | Fixed. Each manager request with a reply gets a fresh transaction. `manager_send` skips replies to earlier ones and waits up to 6 s. `init` watches a restarted driver whenever it runs, and reports separately if the compositor did not take it. |
| C3 | error: `t_gui`'s doc still claimed the compositor "shows the screen again". | Fixed, with INV-0097-2's wording. |
| N1 | note: R10(b), R10(d) and R1's second look at the queue had no test. | Tests added for R10(b) and R10(d). R1's second look is not reached by the fake GPU, which completes at the notify; it is checked by the TCG runs (the guard never fired) and by reading. |
| N2 | note: R10(d) forgets every modifier, the keyboard's too, when any input driver goes. | Stated in ADR-0092. |
| N3 | note: the string bound counts an escape pair once, so a string ends within twice `MAX_STRING`. | Comment corrected. Still bounded. |
| N4 | note: R7 left the display and input ends open when `channel_create` failed. | Fixed. |
| N5 | note: ADR-0090 said "after `RESOURCE_CREATE_2D`", but the code also stops on its own failure. | Text says "from". |
| N6 | note: this file's placeholders. | Filled in. |

## Second check of the fixes

The first check found errors, so TASK-14 asks for one more check of the lines changed to fix them. See below.
