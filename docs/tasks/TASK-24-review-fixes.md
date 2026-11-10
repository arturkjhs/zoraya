# Review of `fix/7-decisions` (the architect's decisions and fixes of 2026-11-10)

An independent review (a fresh session on the main model) of `git diff origin/main...93e1861`: the Retina scale
(proposed ADR-0247), the gear and the tooltip, ADR-0245 and ADR-0246 recorded, INV-0168-2 as changed, ADR-0245's
two conditions, toplevels that fit the work area, the Ctrl-Z flake, the subagents. It ran the host tests of the
changed crates and confirmed findings 1, 5 and 6 with probe tests in a copy of the branch. 1 error, 4 warnings,
8 notes.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | error | A key pressed within the sheet's 500 ms reached it through its repeats after the guard (only buttons were remembered). | Fixed: a key pressed within the guard stays guarded until its release. Test `a_key_pressed_within_the_guard_repeats_to_no_one`. |
| 2 | warning | `files` got `/config` to change through `~` when `shell.home` is `/` or under `/config` (the check was on the grant as written). | Fixed: the panel checks `files`' holds against the expanded home at launch (`confine_files`) and makes such a hold read-only, the pills saying so. Test `files_gets_no_config_through_its_home`. |
| 3 | warning | INV-0090-1 (accepted: `VERSION_1` only) is broken by code following proposed ADR-0247, its check rewritten. | Kept, at the architect's request (the scale for a Retina window needs the EDID); recorded as a check note in ADR-0090's open questions and in the report's queue: if ADR-0247 is rejected, `begin(..., 0)` restores the rule. |
| 4 | warning | INV-0168-2's new in-QEMU check (`files` cannot change `/config`) is listed but not written. | Recorded in ADR-0168's open questions: it comes with stage 7h's file manager, whose GUI test reaches `/config` from its sidebar. |
| 5 | warning | One global guard: a second sheet took the first one's guard away. | Fixed: one guard per sheet, pruned when its sheet is gone. Test `a_second_sheet_leaves_the_first_its_guard`. |
| 6 | note | A lost input driver left the guard waiting for releases that never come (and `delivered` stale). | Fixed: both are cleared. Test `a_lost_input_driver_clears_the_guards_keys`. |
| 7 | note | The guard started at `CreateSheet`, before the sheet could be seen. | Fixed: it starts again at the sheet's first commit. Test `the_guard_starts_when_the_sheet_is_first_committed`. |
| 8 | note | A short or mislabelled `GET_EDID` answer cost the whole display. | Fixed: it leaves the size unknown. Test `a_wrong_edid_answer_leaves_the_size_unknown`. |
| 9 | note | A change of density alone (same pixel size) is not followed. | Recorded in ADR-0247's open questions. |
| 10 | note | `Commands` is lost when a client's channel is full. | Recorded in the report as a limitation; the next toggle tells it again. |
| 11 | note | The Ctrl-Z fix rests on the terminal and the broker agreeing on the foreground; the broker test describes unchanged behaviour. | Recorded in the report; the traced cause (Ctrl-Z before the job's `tcsetpgrp`) is what the script now waits for. |
| 12 | note | ADR-0245's condition (1) has no invariant row. | For the architect (the report's queue): an INV-0245-3. |
| 13 | note | `ci-watch` polls with `sleep`, which some sessions refuse. | Fixed: it says to use the Monitor tool there. |

What the review found sound: the EDID parsing and its bounds, `scale_for`'s thresholds, every user of `InfoReply`,
the parts of the guard listed above as working, the toplevel fitting (whole logical pixels, the first window at
`first_body`, legacy surfaces never resized), `files.rs`'s resolution by the longest place, `Give::Root` gone, the
gear and the tooltip, and the scenarios script's waits.
