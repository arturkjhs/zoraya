# TASK-24 review of 7d

An independent review by a subagent that did not write the code (TASK-24, "Review, for every part"), of
`git diff work/7c..8f0a875`: the compositor's trust boundary (records, rooms, the dock, the two managers), rights in
launches and rooms, robustness against hostile records and files, `unsafe`, and the invariant claims. The reviewer ran
the crates' tests (compositor 83, panel 3, settings 12, ui 9, app 3, proto-gui 4) and verified findings 3, 5 and 7
with scratch programs. Dispositions by the main model; the fixes are in the commit after `d326bfe`.

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | error | A hand-written untrusted line in `/config/rooms` was refused only by `room new`: the panel launched a terminal in it, whose shell holds the root, and the compositor drew the orange edge on it; `room NAME` entered it too. | Fixed: the panel launches nothing in an untrusted room and `room NAME` refuses one, both naming ADR-0056. `a_launch_gives_exactly_its_record_in_its_room` asserts it. |
| 2 | error | INV-0172-2 was marked checked, but every room's terminal holds the root, so no launch holds "exactly the room's entries". | Status set to "not yet applicable (stage 7)", with the reason in ADR-0172's open questions (the only program a room launches is a terminal, whose shell holds the root by INV-0168-2). |
| 3 | warning | The INV-0172-1 client test was dropped by its first garbage message, so the manager ordinals never reached a client channel and the badge's pixels were not compared. | Fixed: the spoofing title's effect on the badge is asserted unconditionally, and every manager message, well formed and with its handle, is sent on a client's channel, a new client replacing a dropped one; records, table, dock and client count unchanged. |
| 4 | warning | INV-0242-1's test missed `AttachInput` and `Stats` on the panel's channel, `SetDock` on `init`'s, and a successful `AddRoomClient` on `init`'s. | Added to `each_manager_takes_only_its_own_ordinals`. |
| 5 | warning | A press in the dock's padding, outside any cell, reached the window under the dock. | Fixed: `under` and `press` test the dock's rectangle; asserted in `the_dock_is_never_covered`. |
| 6 | warning | The panel read the room table again before acting on a chip press, so it could enter another room than the one drawn; an oversized table left the compositor with a stale one. | Fixed: the press is handled first, the file read after; the panel keeps only the rooms whose text fits `SetRooms` (a compile-time check ties its limit to the protocol's), and says so. `the_table_kept_is_the_table_told`. |
| 7 | warning | `rooms::read` cut a file past 256 KiB silently, possibly mid-line into a wider grant. | Fixed: cut at the last whole line and reported. `a_long_file_is_cut_at_a_line_and_reported`. |
| 8 | warning | A failed spawn in the panel leaked the moved channel ends (the compositor would keep a dead client); `start_panel` leaked the job and the image on failure. | Fixed: handles moved into a failed spawn are closed; the job is killed and closed on every failure. |
| 9 | warning | INV-0172-5's lint is a hand list; `room new NAME` fails in `zosh` without grants; the panel's handlers are not in the table; the dock's `+` reads `room new`. | The table's text is now `room new NAME +w PATH` and parses. The hand list fails when a named handler disappears, not when one is added: stated here as the check's limit. The panel has no input handler of its own (its gestures are the compositor's dock presses, in the table). The `+` label stays the README's `room new`. |
| 10 | note | A record longer than 1024 bytes made a launch do nothing, silently. | The panel now says so. |
| 11 | note | A `home` window's title can be padded to sit where a badge would. | No change: `home` has no badge, tint or dot to imitate; the title is clipped before the pills. |
| 12 | note | An untrusted client's popups have no edge. | No change in 7d: nothing runs untrusted (finding 1). For the follow-up when quotas exist. |
| 13 | note | `init` does not restart a panel that exits. | Kept and stated (ADR-0242's follow-up). |
| 14 | note | `room remove` removed the first line of a name, leaving a duplicate to become active. | Fixed: every line of the name goes. |

No `unsafe` was added. No open error remains.

## Check of the changed lines

The main model reread every changed line against its finding. `cargo test -p zoraya-compositor -p zoraya-panel -p
zoraya-settings` (83, 4 and 13 tests), clippy with `-D warnings` on those crates and on `zoraya-user` for the target,
and `make adr-check` pass; `tests/gui/rooms.script` passes again under HVF and TCG after the fixes.
