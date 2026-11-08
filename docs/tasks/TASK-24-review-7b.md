# TASK-24 review of 7b

An independent review by a subagent that did not write the code (TASK-24, "Review, for every part"), of
`git diff aee4531..0bee8b1` on `work/7b`: the compositor's trust boundary, resources and panics, input routing,
`term`, the invariant claims, deviations from ADR-0160 and ADR-0164. The reviewer ran the crates' tests and two
tests of its own in a copy of the repository (findings 1 and 2). Dispositions by the main model; the fixes are in the
commit after `0bee8b1`.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | error | An unfocused client could open a display-sized frameless popup at the top of the whole stack, over another client's focused window (a spoof); the INV-0160-2 test compared only toplevels. | Fixed: a popup is opened only by the focused window or one of its popups (`UNSUPPORTED` otherwise), is inserted directly above its parent, is at most the work area, and is drawn clipped to the work area below the bar. The INV-0160-2 test now compares the whole stack; `only_a_focused_window_opens_popups_and_they_stay_with_it`. |
| 2 | warning | Moving a window left and resizing it again grew its body without bound (3,608 pixels wide on a 640-pixel display), and every mask with it. | Fixed: a resize stops at the work area. `a_resize_stops_at_the_work_area`. |
| 3 | warning | Minimising a toplevel left its popups drawn and reachable. | Fixed: hiding a toplevel closes its popups (the client gets `CloseRequested`). In the popup test. |
| 4 | warning | `close_asked` never expired: a client that asked "save?" and was cancelled would be ended by any later close press. | Fixed: a commit to the window clears it (a living client is asked again), and it lapses after 20 s; "close anyway" is a press between 5 and 20 s after an unanswered ask. `a_client_that_commits_is_asked_again_on_the_next_close`. |
| 5 | warning | A window taller than the work area was placed under the bar; presses on the bar reached windows and popups. | Fixed: placement never goes above the bar, and `under()` gives nothing in the bar's strip. `a_press_on_the_bar_reaches_no_window`; two stage 5 tests updated to the new place (1, 64). |
| 6 | warning | Per compose, a body mask and a buffer-sized cut were made for every window, a popup's shadow was blurred every frame, and while the switcher was open every pointer move blurred and shrank everything again. | Fixed: the body mask and a popup's shadow are in the window's cached `Chrome`; the cut is made only while a resize is unanswered; thumbnails are made when the switcher opens and when that window commits; a pointer that only moves over the open switcher is drawn on the kept frame. |
| 7 | warning | The first commit after `SetBuffer` showed zeros outside the committed rectangle. | Fixed: the new buffer is read whole at its first commit. `the_first_commit_to_a_new_buffer_reads_it_whole`. |
| 8 | warning | Popups were clamped (moved) onto the display, where ADR-0160 says clipped. | Fixed with 1: placed where asked, clipped to the work area. |
| 9 | warning | "A popup takes keyboard focus while open and returns it on close" was not built. | Fixed: a new popup takes focus; closing it gives focus back to its parent; the system chords of a focused popup act on its window. In the popup test. |
| 10 | warning | One client could hold eight toplevels and four popups of display size each; `SetBuffer` allocated a copy and dropped it. | Fixed: a client's buffers together are at most four displays' worth (`NO_MEMORY` past it); `SetBuffer` maps without a copy. `a_client_has_a_budget_of_buffers`. |
| 11 | note | On a legacy surface's border, `Pointer` coordinates wrapped (`x - body.x` negative). | Fixed: pointer events go only for points inside the body. |
| 12 | note | Releases were swallowed during a chord or the switcher, so a client saw Alt pressed and never released. | Fixed: a key whose press reached a client is released to it whatever happens meanwhile; a key pressed during a chord reaches nobody. `a_release_reaches_the_client_that_saw_the_press`. |
| 13 | note | A display re-attached at another size changed the scale without reconfiguring windows. | Fixed: every toplevel keeps its logical size at the new scale and gets a `Configure`. `a_new_display_size_reconfigures_the_windows`. |
| 14 | note | The switcher's rows fell off the screen past about eleven windows. | Fixed: as many rows as fit, the chosen one among them. |
| 15 | note | INV-0160-1's GUI check reads a strip, not the whole frame; INV-0164-1's "every client pixel unchanged" against the round lower corners; INV-0160-7's popup assertion was trivially true. | INV-0160-1 is checked pixel for pixel by the host test (`the_frame_does_not_depend_on_the_clients_pixels`); the GUI's regions are a sample, and the row says both. The corners: proposed [ADR-0241](../adr/0241-window-corners-and-client-pixels.md) reads INV-0164-1 inside the window's shape (Option A, square lower corners, is one line if the architect prefers the letter). INV-0160-7's test now relies on the bar's absorption and the work-area clip (finding 5), which it exercises. |
| 16 | note | `term`'s `buffer()` leaked the handle or mapping on a failure; `Term::resize`'s doc misdescribed the scrollback; `term` clears `outstanding` while a commit may be in flight. | Fixed the leak and the doc. The `outstanding` reset is kept: after a `SetBuffer` the next commit must be the whole new buffer, and the compositor serialises a client's commits (one at a time, ADR-0093), so a second commit waits there; noted here. |

## Check of the changed lines

The main model reread every changed line against its finding. `cargo test -p zoraya-compositor` (65 tests),
clippy with `-D warnings` on the compositor and on `zoraya-user` for `aarch64-unknown-none`, and `make adr-check`
pass; the GUI scripts are run again before the merge (below in the report). No open error remains.
