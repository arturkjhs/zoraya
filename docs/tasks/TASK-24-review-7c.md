# TASK-24 review of 7c

An independent review by a subagent that did not write the code (TASK-24, "Review, for every part"), of
`git diff work/7b..517b2d6`: the layout tables against the reference, the dead-key machine against ADR-0163, the trust
boundary and focus, the `Title` message, the invariant claims. The reviewer ran the crates' tests and verified findings
1, 2 and 4 to 6 with a scratch program over `zoraya-keymap`. Dispositions by the main model; the fixes are in the
commit after `517b2d6`.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | error | A key with no letter (Enter, Backspace, Tab, Esc, arrows) left a pending dead key armed: on `cz`, `=` Enter `e` gave a newline and then `é`. | Fixed: any press but a modifier takes the pending dead key; only a letter can use it. `non_letters_cancel_a_dead_key_and_altgr_falls_back_to_level_three`. |
| 2 | error | A Ctrl chord left it armed too: `=` Ctrl+C `e` gave `^C` then `é`. | Fixed with 1. Same test. |
| 3 | warning | A chord's keys (System+`=`) and the switcher's ran through the dead-key machine, so a chord could arm a dead key for the client's next letter. | Fixed: a key the compositor takes leaves the dead state as it was. `a_switch_keeps_a_held_key_and_tells_only_the_focused_client`. |
| 4 | warning | Caps or Shift with AltGr on a key without a fourth level gave the plain capital (`K`) instead of the third level (`ł`). | Fixed: a missing fourth level falls back to the third, and Caps applies to a level pair only when that pair is a case pair. Same test as 1. |
| 5 | warning | Ctrl used the US position in Latin layouts, swapping Ctrl+Z and Ctrl+Y on QWERTZ. | Fixed: Ctrl uses the active layout's plain Latin character, the US one only where the layout has none (Cyrillic). In `a_switch_keeps_modifiers_and_clears_the_dead_key`. |
| 6 | warning | A right Alt held across a switch from a layout with AltGr to one without turned into Alt: a modifier change INV-0163-4 forbids, asserted by a test. | Fixed: a held right Alt keeps its role until it is released; the test now asserts no change. |
| 7 | note | `ua`'s backslash key level 4 is `|`, not `/` (medium confidence). | Fixed. |
| 8 | note | `cz` AltGr gives `` ` `` on h, `'` on j, `^` on m (medium-low confidence). | Added; the module comment no longer claims Czech's third level is complete beyond what is listed. |
| 9 | note | No row for key 86 in `ua` and `ru`; Caps does not capitalise Czech's number-row letters as XKB's semi-alphabetic type does. | Kept: key 86 is absent on the Mac keyboard QEMU passes and in the GUI tests; the Caps rule is ADR-0163's case-pair rule, and the deviation from XKB is now stated in `layouts.rs` and `docs/design.md`. |
| 10 | warning | INV-0163-4's check lacked a non-modifier key held across a switch and "Layout only to the focused client"; INV-0163-1's test never asserted at most one character; INV-0163-3 covered only ASCII bases. | Added: `a_switch_keeps_a_held_key_and_tells_only_the_focused_client`, `a_press_gives_at_most_one_character_in_every_layout`, `every_accent_composes_or_passes_every_base_of_every_layout` (every letter of every layout at levels 1 and 2 with every accent). |
| 11 | note | The runner's `presses()` is correct for the script; it cannot reach letters only the fallback gives. | No change needed. |
| 12 | note | `Title`: no defect; capacity nit (`+ 8` for a nine-byte prefix). | Fixed the nit. |
| 13 | note | Trust boundary: no defect beyond 3. | — |

## Check of the changed lines

The main model reread every changed line against its finding; `cargo test -p zoraya-keymap -p zoraya-compositor`
(13 and 69 tests) and clippy with `-D warnings` pass. No open error remains.
