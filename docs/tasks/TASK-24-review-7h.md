# TASK-24 7h: independent review

An independent review (a fresh session on the main model) of `git diff fix/7-decisions...work/7h` at `2feb1b1`:
proposed ADR-0248's widenings in the compositor, the file protocol and the panel, `zoraya-files`, `files.rs`, the
runner's new steps and the two GUI scripts. It ran the workspace's host tests in a copy of the tip. 4 errors,
5 warnings, 7 notes.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | error | The workspace host tests failed: `zoraya-proto-gui`'s bad-modifier test wrote 0x10, now `SYSTEM`; `zoraya-tokens`' copy of `tokens.txt` drifted from the mockups' (their `screens` lists). | Fixed: 0x20 in the test; the token copy brought in line. The workspace's host tests pass. |
| 2 | error | The gesture `Offer` let any client, an untrusted room's included, replace the clipboard after any key or click. | Fixed: a press in the body only, never a key, never an untrusted room's client; ADR-0248 says the remaining cost (any trusted application clicked in may offer once). Tests `a_press_in_a_window_lets_it_offer_once`, `an_untrusted_window_never_offers_unasked`. |
| 3 | error | The gesture outlived a focus change and a client's end, and a gestured offer cancelled another client's pending System+C. | Fixed: cleared on a focus change to another client, on System+C or System+X, and when the client goes; `copy_asked` cleared only by its own answer. Test `a_gesture_does_not_outlive_the_focus_or_a_copy`. |
| 4 | error | `OpenTerminal` looked the room up by name at the request and fell back to `home` when it was gone. | Fixed: the panel keeps the launch's `Room` record with the launcher channel and uses it. |
| 5 | warning | A drop below the rows shown (the status line) named a folder scrolled out of view. | Fixed: `row_at` takes only a row shown (the view records how many). Test `a_drop_below_the_rows_shown_names_none`. |
| 6 | warning | INV-0160-3 and INV-0170-2 stay `checked` while their tests check proposed ADR-0248's wording. | Check notes in ADR-0160's and ADR-0170's open questions, and the report's queue: if ADR-0248 is refused, one constant or one branch undoes each. |
| 7 | warning | `WindowPointer`, `DragEnded` and `OpenWith` changed under their ordinals, against ADR-0052. | Stated in ADR-0248's evidence against (every encoder is in the tree); for the architect. |
| 8 | warning | Copy, move and delete are untested; a failure part way leaves what was done. | Model tests of a copy into another place and from a read-only folder; the GUI script copies a folder whole and deletes one with its contents. Moving the walk into a host-tested crate is left; the partial result is recorded in `docs/design.md`. |
| 9 | warning | INV-0248-1's and INV-0248-3's checks were narrower than they say. | Tests over every key below 0x110 with the system key held, and a drop over no window. |
| 10 | note | Pointer events told an unfocused window the modifiers. | Fixed: to the focused client only. Test `an_unfocused_window_hears_no_modifiers`. |
| 11 | note | The menu's Open showed `edit` for any file, Delete one path for a selection. | Fixed: the panel's opener for the name (or "no application opens NAME"), and every selected path. |
| 12 | note | `show`'s cursor filter compared the new folder with itself. | Fixed. |
| 13 | note | `start_drag`'s comment said folders are not carried. | Fixed. |
| 14 | note | A forwarded chord's release carries no `SYSTEM`. | Recorded in `docs/design.md`. |
| 15 | note | `OpenTerminal` is not rate-limited. | Recorded in `docs/design.md` (one request at a time per channel, as `OpenWith`). |
| 16 | note | `Commands` to every client changes ADR-0245's condition as recorded. | Said in ADR-0248's point 3. |

What the review found sound: `TAKEN_CHORDS` against every reserved chord; the forwarded chords keep the sheet guard and
the drag and switcher returns; `DragEnded`'s place only over the source's own window body; `terminal_at`'s refusals;
`OpenWith`'s hint; moves never crossing places, the volume refusing a folder into itself and replacement; links not
followed by the recursive delete; bounded reads; multibyte rename; the delete card taking every input; hit-testing and
painting agreeing at scale 2; `FreeSpace`; `rename_into`'s token; the selection kept by name across search, sort and
listing.
