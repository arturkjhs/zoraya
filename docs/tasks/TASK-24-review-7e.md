# TASK-24 stage 7e: independent review

Scope: `git diff work/7d...HEAD` on `work/7e` (head e841d8d), read against ADR-0167, ADR-0170, ADR-0172 and the
proposed ADR-0245. Only reading was done. No test, QEMU run or mutation run was made for this review, so every
finding comes from reading the code, and the failure scenarios are traced through the code, not observed.

## Summary

The gating holds as built. Ordinals 11 (`CreateSheet`) and 12 (`Granted`) are accepted only from a slot that ordinal 42
(`AddSystemClient`) marked, and 42 is accepted only on the driver manager's channel. A sheet's strip, emblem and
heading, and the bar's mark, come from the compositor's own state: the application name comes from the panel's record,
`SetTitle` and `SetBuffer` refuse sheets, and a popup cannot reach above its parent's body line. The owner of a sheet
gets no pointer events, presses or focus through its own window. No pointer event reaches any client while an entry is
carried. A `Drop` goes only to the framed window under the pointer, and never to the source's client. The rights the
chooser asks for are `Access::rights()` row for row. The panel re-clones `OpenWith` to `READ | CLONE`. The file
manager drags with `READ | CLONE`. The data directory gets `tree` minus `publish`. The one new `unsafe` block is sound.

There are two errors, both about text the trusted parts show or hand on:
- The words of a dragged entry are chosen by the source client (any client may send `Drag`), and they become a pill in
  the target's frame. That breaks INV-0172-1, which `invariants.md` marks as checked.
- `Chosen` and `Drop` hand the application the entry's full path (`+r ~/project/plán.txt t_open`). ADR-0167 says an
  application never learns the path of what it was given.

Most of the warnings come from four causes:
- Size bounds that fail silently and lose a client or an answer.
- A sheet that takes the keyboard at a moment the application picks.
- A sheet that is not placed again after a scale change.
- The panel blocking, or spinning, on file-manager input.

## Findings

1. **error.** Any client chooses the pill text a drop puts in another client's frame.
   - Where: `crates/zoraya-compositor/src/lib.rs:939` (`start_drag` takes `d.words` from the client),
     `lib.rs:1011` (`add_entry(key.client, &c.words)`), `lib.rs:2433` (ordinal 13 is accepted from every client).
   - What is wrong: INV-0172-1 (status `checked`) says the records behind the pills change only through
     `AddClient`, the chooser's `Granted` and the compositor's own `Drop`, and that no client message changes them. In
     7e, the text of a pill added by a drop is whatever the source wrote in `Drag.words`. The only filter is "UTF-8,
     non-empty, no control characters". The handle's kind is not checked either. ADR-0170 gives the words as
     `dropped: NAME read`, made from the compositor's own knowledge.
   - Scenario: an ordinary application (not `files`) gets a body press, sends `Drag { words: "+r ~/taxes.pdf" }`
     with a channel to itself, and the person releases over `edit`. `edit`'s frame now shows a trusted pill
     `+r ~/taxes.pdf`, while `edit` holds a channel served by the attacker. `edit` speaks the file protocol to that
     channel and shows the attacker's contents under the forged name.
   - Fix: the compositor writes the words itself from the name (for example `dropped: NAME read`) and never takes
     `words` from the client. Better still, accept `Drag` only from clients the manager marks as entry sources (the
     panel launches `files` and could mark it, as `AddSystemClient` marks the chooser). Check
     `kind(entry) == Channel`. Add the INV-0172-1 test for ordinal 13.

2. **error.** The application learns the path of what it was given.
   - Where: `crates/zoraya-chooser/src/lib.rs:280-281` (`line` holds the shown path), `user/src/bin/chooser.rs:292`
     (`Chosen.words = grant.line`), `crates/zoraya-compositor/src/lib.rs:996-1004` (the `Drop` carries the source's
     words, a path from `files`).
   - What is wrong: under "What an application can never do", ADR-0167 says an application cannot "learn the path of
     what it was given (it gets a display name)". Its protocol paragraph words the reply as `grants` does: `read`,
     `create or replace NAME`, `change anything in`, with no path. In 7e, `Chosen.words` is
     `+r ~/project/plán.txt t_open` or `create or replace ~/nový.txt`, and the `Drop` the target gets carries
     `+r ~/…` from `files`. ADR-0245 does not state this deviation.
   - Scenario: `t_open` prints `t_open: given +r ~/project/plán.txt t_open`. Any application learns the directory
     layout above a granted file, and through `~` expansion the home's position. Under ADR-0172 an agent's window
     learns where the person keeps things.
   - Fix: send `Access::words()` (`read`, `create or replace NAME`, `change anything in`) in `Chosen` and the name
     alone in `Drop`. Keep the path for the sheet's sentence and the compositor's pill. Alternatively, state the
     change in ADR-0245 as a deviation for the architect. Under the task's rules, an ADR that changes an accepted
     ADR's decision blocks the merge.

3. **warning.** Drops into an untrusted room's window are delivered without the confirmation card.
   - Where: `crates/zoraya-compositor/src/lib.rs:979-1025` (`drop_carried` never looks at `record.untrusted`).
   - What is wrong: INV-0172-3 requires the person's confirmation in the compositor's card first. The panel refuses
     launches in untrusted rooms today, so this cannot happen yet. But drops now exist, and the code delivers them to
     any untrusted client the manager adds (the 7d test path creates such clients).
   - Scenario: an untrusted test client added through `AddRoomClient { untrusted: 1 }` receives a `Drop` and its
     handle with no card.
   - Fix: refuse (close the entry) when the target's record is untrusted, until the card exists. Add a host test, and
     record it in ADR-0172's check note.

4. **warning.** `WindowPointer.serial` leaks the count of other clients' presses.
   - Where: `crates/zoraya-compositor/src/lib.rs:1140`, `lib.rs:1307`; documented in
     `crates/zoraya-proto-gui/src/lib.rs` as "the serial of the last left press the compositor delivered to this
     client".
   - What is wrong: `press_serial` is one global counter, bumped by a body press on any window (sheets included), and
     every client's pointer events carry it.
   - Scenario: a background client compares the serials of two of its pointer events and learns how many times the
     person clicked in other windows meanwhile, including in the chooser's sheet. ADR-0093 forbids a client learning
     of another's input.
   - Fix: keep the serial per client, or send the press's serial only to the pressed window and the client's own last
     serial to the others. Add a test with two clients.

5. **warning.** The application picks the moment its sheet takes the keyboard, so typing or clicking meant for the
   application can confirm the sheet.
   - Where: `crates/zoraya-compositor/src/lib.rs:2034-2040` (focus moves to the sheet as soon as it is created),
     `crates/zoraya-chooser/src/session.rs:73-77` (the list has the focus), `session.rs:200-232`.
   - What is wrong: requests need no gesture. The application sees its own keys and presses, so it can send `Open` or
     `Save` just before a key or click the person is about to make. Keys (and auto-repeat of a held key) delivered
     after `CreateSheet` go to the sheet. INV-0167-2 is met to the letter (there is input), but not in its intent
     (the input was not a confirmation).
   - Scenario: the person is typing in the application's search field. The application sends `Open` as soon as it
     sees Down. The sheet appears, and the person's next Down and Enter select and confirm the first file of a folder
     with no subfolders. The same works with a "click here twice" game against the row and the Open button, since the
     sheet's position is known.
   - Fix: ignore keys and presses on a sheet for a short time after it maps (about 500 ms). Also ignore any key whose
     press was delivered before the sheet existed: the compositor's `delivered` bits already track this.

6. **warning.** A sheet is not placed or rescaled again when the scale or the display changes.
   - Where: `crates/zoraya-compositor/src/lib.rs:1475-1500` (`reconfigure` updates toplevels only, and nothing calls
     `place_sheets` afterwards).
   - What is wrong: after a scale change the owner's position and size are rescaled, but the sheet keeps its old
     `x`, `y` and physical body. Its strip no longer starts on the owner's title-bar line (ADR-0167's third mark), and
     it can cover another window. The sheet also cannot be given a new buffer, because `SetBuffer` refuses sheets.
   - Scenario: open a sheet at scale 1, then the display changes to a width that gives scale 2. The owner moves to
     doubled coordinates and the sheet stays at the old ones, half-sized.
   - Fix: in `reconfigure`, rescale each sheet's body as a toplevel's is, call `place_sheets` for every owner, and send
     the chooser a `Configure` for the sheet (or close the sheet and cancel the request).

7. **warning.** The grant line on a Save sheet is stale while the person types the name.
   - Where: `crates/zoraya-chooser/src/session.rs:214-221`.
   - What is wrong: `Changed(NAME)` updates `browser.name` but does not rebuild the tree. The label under the list
     (`create or replace ~/OLD`) keeps the suggested or last selected name, while Save confirms the typed name.
   - Scenario: the suggested name is `nový.txt`. The person types `report.txt` and presses Enter. The sheet showed
     "create or replace ~/nový.txt", and the grant is `report.txt`. The pill is right; the sheet's sentence (its
     "trusted" text) was not.
   - Fix: update the line label in place. Either rebuild with the text field's cursor kept, or give `Label` a setter
     the session calls on `Changed(NAME)`. Add a session test that the label follows the field.

8. **warning.** An oversized `Drag` makes the compositor drop the file manager.
   - Where: `crates/zoraya-app/src/lib.rs:123-128` (`message` encodes into 1 KiB and sends **zero bytes** when encoding
     fails), `lib.rs:315-326` (`App::drag`), `crates/zoraya-compositor/src/lib.rs:2274-2276` (a message without a
     valid header makes the compositor drop the client).
   - What is wrong: `Drag.words` is bounded by `MAX_LINE` (256 bytes). `files` sends `+r ` plus the shown path, which
     passes 256 bytes in a deep tree, sooner with Cyrillic or Czech names (2 bytes per character).
     `encode` fails, an empty message goes out with the entry, and the compositor drops the client.
   - Scenario: press on a file at `~/…` whose shown path is 254 bytes long. The `files` window disappears and the
     program sees `Gone` and exits.
   - Fix: make `message` return `Option` and fail the call instead of sending nothing. Check the bounds in
     `App::drag` and close the entry on refusal. Finding 1's fix also removes the long `words`.

9. **warning.** The chooser loses the answer or the pill when the words are long.
   - Where: `user/src/bin/chooser.rs:55-62` and `:289-303`.
   - What is wrong: if `Chosen` fails to encode (`words` over `MAX_WORDS`, 512 bytes, with a deep path),
     `reply` closes the handle and sends nothing. The application never gets `Chosen` or `Cancelled`, and
     `pending` is already cleared. `Granted.words` is bounded by `MAX_LINE` (256). For a pill between 257 and 512
     bytes, `Chosen` succeeds but `Granted` fails to encode and is skipped without a word. The application then holds
     a grant its frame does not show (INV-0172-2).
   - Scenario: open a file 300 bytes deep. `t_open` gets its handle and no pill appears. At 600 bytes, `t_open` waits
     forever with `asked = Some`.
   - Fix: on an encode failure, answer `Cancelled(FAILED)`. Do not hand out a connection whose pill cannot be shown.
     Either check the grant's length in `Browser::pending`, so the button is disabled, or send `Granted` first and
     answer `Chosen` only after `GrantedDone` is OK (which also covers finding 15).

10. **warning.** File-manager input can block or spin the panel.
    - Where: `user/src/bin/panel.rs:353` (`Err(_) => return`), `panel.rs:367` (`clone_as` on the received handle);
      `user/src/files.rs:57-87` (`call` waits with `DEADLINE_INFINITE`).
    - What is wrong, (a) spinning: the kernel leaves a message in the queue when the read buffer is too small
      (`kernel/src/syscall.rs:748`). `OpenWith` with two handles or more than 2048 bytes therefore stays at the head
      of the launcher channel, and its `Level` subscription wakes the panel every time. The panel spins, and every
      later `OpenWith` from that file manager is stuck behind it.
    - What is wrong, (b) blocking: `clone_as` writes to the handle `files` supplied and waits with no deadline. A
      channel whose peer never answers blocks the panel for good: no dock, no launches, no rooms.
    - What is wrong, (c) narrowing: re-cloning narrows only a real file-service connection. If the handle is a channel
      `files` serves itself, the application gets a server `files` controls. That is acceptable while `files` holds the
      root, but INV-0245-2 should say "for a file-service connection".
    - Scenario: a bug in `files` (or a future third launcher) sends a malformed `OpenWith`, and the panel goes to 100%
      CPU, or freezes on a bad handle.
    - Fix: (a) on `BufferTooSmall`, drop the launcher (unsubscribe and close), as the chooser does for applications.
      (b) give `clone_as` (or this one call) a deadline.

11. **warning.** The invariant table and the design document were not updated for what 7e implements.
    - Where: `docs/adr/invariants.md:361-376`; `docs/design.md`, which the diff does not touch.
    - What is wrong: INV-0167-1 to -4 and INV-0170-1 to -5 still read `not yet applicable (stage 7)`. Yet 7e
      implements them, and host tests exist (`tests_sheets.rs`, `tests_drops.rs`, `tests_session.rs`). CLAUDE.md and
      ADR-0001 rule 6 require the Status column to be true. Checks that cannot run yet need an open-question note.
      INV-0170-3 also names `DragStarted`, which 7e replaces with the `DragDone` reply, and that is not recorded.
      ADR-0245 cites measurements "in `docs/design.md`", and the task requires measurements there. The ADR-0245
      invariants are not in the table. This may be planned for a later step of 7e, so it is a warning, not an error.
    - Fix: set the statuses (`checked: make test-unit` and so on), add the check notes, record the
      `DragStarted`/`DragDone` change in ADR-0170's open questions or in ADR-0245, and update `docs/design.md`.

12. **warning.** The drag label can be wider than its damage rectangle. *(The visible effect is inferred from the
    compose path, not seen.)*
    - Where: `crates/zoraya-compositor/src/lib.rs:217`, `:933-936`; `crates/zoraya-compositor/src/draw.rs:244-260`.
    - What is wrong: the label is drawn at its measured width plus 20 logical pixels, from the pointer plus 16. The
      damage covers 260 logical pixels from the pointer plus 12. A name close to 255 bytes reaches past the damage, so
      part of the label is clipped. When another damage rectangle overlaps it, those pixels are composed and later not
      erased, and stale fragments of the label stay on screen.
    - Fix: cut the label (an ellipsis) to `CARRIED_WIDTH - 24`, or compute the rectangle from the measured width.

13. **note.** Escape does not end a drag, and presses during a drag still act.
    - Where: `crates/zoraya-compositor/src/lib.rs:717-841` and `press`, `lib.rs:1068-1098`.
    - What is wrong: ADR-0170 says Escape ends a drag with nothing delivered. In 7e, Escape goes to the focused client,
      and a right or middle press while an entry is carried still raises and focuses the window under the pointer,
      which then gets focus events (INV-0170-3), or presses the dock.
    - Fix: while `carried` is set, Escape cancels the drag, and `press` returns early.

14. **note.** The owner's open popups stay usable while its sheet is open.
    - Where: `crates/zoraya-compositor/src/lib.rs:1080-1104`, `create_sheet` (`lib.rs:1943`).
    - What is wrong: the sheet goes above the owner's popups, but the popups are not closed. Where a popup sticks out
      beside the sheet, a press focuses it (`set_focus` maps only the owner to the sheet), so the owner's client gets
      keys again and may open more popups. ADR-0245 says the owner gets no input while the sheet is open.
    - Fix: in `create_sheet`, close the owner's popups (sending `CloseRequested`, as a press elsewhere does).

15. **note.** Frames can show entries the client does not hold, and miss entries it does hold (INV-0172-2).
    - Where: `crates/zoraya-compositor/src/lib.rs:1011` and `:2066-2075` (`add_entry` returns `NO_MEMORY` at
      `MAX_RECORD_ENTRIES`, and both callers ignore it); `user/src/bin/chooser.rs:296-303` (`Granted` sent with
      transaction 0, so the result is never seen); `user/src/bin/panel.rs:281-293` (a `Give` that fails is skipped,
      but `entries` still lists `+w /data/...` or `+w /`). A drop is also added as a pill for clients that close the
      handle (the terminal closes every handle it reads).
    - Fix: deliver the entry only when the pill was added. Build `entries` from the handles that were actually given.

16. **note.** Registration replies can pair with the wrong request.
    - Where: `user/src/bin/panel.rs:201-208`, `user/src/gui.rs:203-216`.
    - What is wrong: every `AddApplication` uses transaction 1, and `wait_reply` does not check the transaction. After
      one 2-second timeout (the chooser busy listing a large folder), the late `Added` is read as the next launch's
      reply.
    - Fix: number the registrations, and skip replies whose transaction does not match.

17. **note.** Edge cases in the rights the chooser gives (each is the person's choice, but the sheet's words do not
    say so).
    - Where: `crates/zoraya-chooser/src/session.rs:250-283`; `crates/zoraya-fs-service/src/lib.rs:1323-1336`.
    - Save to the name of an existing folder gives a narrowed connection that the service opens as that folder,
      read-only, so its whole subtree can be read under "create or replace NAME".
    - Open is a TOCTOU: if the listed file is replaced by a folder before Open, `open(.., 0, READ|CLONE)` returns a
      folder connection.
    - `PickDirectory { write: 1 }` at `/` gives `tree` over the whole volume, `/config` included.
    - Fix: refuse Save onto an existing directory. Open with a flag or stat check that the object is a file. Consider
      refusing `tree` on `/` and `/config`.

18. **note.** The bar's sheet mark does not follow focus.
    - Where: `crates/zoraya-compositor/src/draw.rs:196-200`.
    - What is wrong: the bar shows the heading of any visible sheet, even one that is unfocused or under other windows.
      Two colluding applications can show the real mark beside a fake dialog drawn in the focused one. The fake gets
      no handle, but it could get typed text.
    - Fix: show the mark only while the focused window is the sheet.

19. **note.** Bidirectional controls pass the text filter.
    - Where: `crates/zoraya-compositor/src/lib.rs:945-950` (`char::is_control` covers only Cc).
    - What is wrong: names with U+202E and the like are drawn in the trusted material (the drag label), where they can
      reverse how a name reads.
    - Fix: also refuse Cf (format) characters in texts the compositor draws for clients.

20. **note.** `App::drag` leaks the entry when it refuses.
    - Where: `crates/zoraya-app/src/lib.rs:316-318`; `user/src/app.rs:77-93`.
    - What is wrong: `App::drag` returns `false` without closing `entry` when `held != Some(window)`, although its
      document says the entry is closed. `Gui::send` does not close a moved handle when `channel_write` fails. The
      kernel keeps the handle in the sender's table on failure, as `user/src/files.rs:64-71` assumes. The `files`
      caller is guarded by `app.held(w)`, so this is latent.
    - Fix: close the entry on both paths.

21. **note.** The chooser decodes requests as carrying no handles, whatever they carried.
    - Where: `user/src/bin/chooser.rs:86-101`.
    - What is wrong: `ask_of` decodes with a handle count of 0 instead of the count received. An `Open` that carries a
      handle is accepted (the handle is closed) instead of refused as malformed.
    - Fix: pass `sizes.handles` into `ask_of`.

22. **note.** `Chosen` can leak a handle when nobody is waiting for it (unreachable today).
    - Where: `user/src/bin/chooser.rs:280-285`.
    - What is wrong: when an outcome arrives but `apps[i]` or `pending` is `None`, the code reaches `continue`, and an
      `Outcome::Chosen` handle leaks. `leave` clears `shown`, so this cannot happen now.
    - Fix: close `handle` on those paths.

23. **note.** `start_chooser` leaks handles when the chooser fails to start.
    - Where: `user/src/gui.rs:323-355`.
    - What is wrong: if `start("chooser", …)` fails, `ours` (a compositor connection already marked as a system
      client) and `chooser` (the registration end) stay in `init`. Only `panel` is closed.
    - Fix: close `ours` and `chooser` there too.

24. **note.** `t_open` busy-loops once the chooser is gone.
    - Where: `user/src/bin/t_open.rs:205-263`.
    - What is wrong: with no request pending, a closed chooser channel (a `Level` subscription to `PEER_CLOSED`) wakes
      the loop forever, and nothing unsubscribes. It is a test program, but it runs in the GUI test.
    - Fix: unsubscribe the chooser channel, or drop it, on `PEER_CLOSED`.

25. **note.** `files` in a trusted room gets the volume root.
    - Where: `crates/zoraya-panel/src/lib.rs` (`application`, `Give::Root`).
    - What is wrong: `files` gets the root whatever the room, so a room's confinement does not apply to it. INV-0168-2
      allows this, and ADR-0245 lists it as an open question. This finding is only so the architect sees it next to
      ADR-0242's condition for terminals.

26. **note: `unsafe`.** The diff adds one `unsafe` block: `user/src/app.rs:146-151` (`Gui::pixels`).
    - The SAFETY comment is accurate. The mapping is writable, page-aligned and `size >= pixels * 4` bytes, and it lives
      until `free(&mut self)`. The slice's lifetime is tied to `&mut self`, so two slices cannot alias, and `free`
      cannot run while one exists.
    - The compositor maps a read-only duplicate of the same memory and reads it at commits. That is the shared-buffer
      protocol of ADR-0093 and not a Rust aliasing problem inside this process. The comment could say so.
    - The three `#[unsafe(no_mangle)]` attributes are the usual program entry points. Every new library crate has
      `#![forbid(unsafe_code)]`.

## Dispositions

By the main model, after the review; each fix with a test where one could be written.

| # | Disposition |
|---|---|
| 1 | Fixed. `Drag` carries only a display name (at most 64 bytes, no `/`, no control or bidirectional character) and a connection (the compositor checks the handle is a channel); the pill is the compositor's own, `dropped: NAME read` (ADR-0170's words). `tests_drops.rs` refuses a name with an escape, a bidirectional override, a `/`, an empty one and a memory handle, and checks the pill. |
| 2 | Fixed. `Chosen.words` is the kind and the display name (`+r plán.txt`, `+c nový.txt`); the full path stays in the compositor's pill and the sheet, which only the person sees. `Drop` has no words. Tests in `zoraya-chooser`. |
| 3 | Fixed. The compositor delivers no drop to a window of an untrusted room until the confirming card exists (test). |
| 4 | Fixed. Each client counts its own presses (test). |
| 5 | Open: recorded in ADR-0245's open questions (a delay after a sheet maps, and ignoring held keys). |
| 6 | Open: recorded in ADR-0245's open questions (re-placing a sheet on a scale or display change). |
| 7 | Fixed. The sheet is laid out again on every change of the name, so the grant's line follows it. |
| 8 | Fixed. `App::drag` refuses a name over 64 bytes before encoding and gives the entry back. |
| 9 | Fixed. `Chosen.words` is short; a grant whose pill is longer than a line is not given (`Cancelled`, `FAILED`), so a window never holds what its frame does not show. |
| 10 | Partly fixed: a launcher message the panel cannot read ends that launcher's channel instead of waking the panel for ever. The re-clone of `OpenWith`'s connection still waits on the file manager's channel; `files` is a program of the system image, recorded in ADR-0245. |
| 11 | Fixed in 7e's invariants step (`docs/adr/invariants.md`, `docs/design.md`). |
| 12 | Fixed. The label's damage rectangle covers 64 characters and the material's halo. |
| 13 | Fixed. Escape ends a drag with nothing delivered; other presses during a drag do nothing; keys reach no client (test). |
| 14 | Open (note): popups of a window with a sheet open; ADR-0245's open questions. |
| 15 | Open (note): the record cap and failed slots; recorded for 7f. |
| 16 | Open (note): one registration at a time from the panel, which waits for each reply; recorded. |
| 17 | Open (note): each is the person's choice in the sheet; recorded in ADR-0245's open questions. |
| 18 | Open (note). |
| 19 | Fixed. The compositor's text filter refuses bidirectional overrides and isolates in every record line and drop name. |
| 20 | Fixed. `App::drag` returns the entry when it did not send it, and `files` closes it. |
| 21–23 | Open (notes): latent leaks on paths unreachable today; recorded. |
| 24 | Fixed. `t_open` drops the chooser's channel when it closes. |
| 25 | As ADR-0168 decides (INV-0168-2 lists `files`); recorded in ADR-0245's open questions. |
| 26 | No change. |

## Check of the fixes

This check covers the changed lines of 9113cf7. It was done by reading the code only: no tests or QEMU runs. Only
findings marked Fixed or Partly fixed are checked.

| # | Result |
|---|---|
| 1 | **Confirmed.** `Drag` carries only `name` (at most 64 bytes, no `/`, `plain_text`) and the handle must be a channel. The pill is built by the compositor as `dropped: NAME read`. One part remains, and it is by design (ADR-0170: the compositor never opens an entry): the source still chooses the name, and the channel can be any channel. Now recorded in ADR-0245. |
| 2 | **Confirmed.** `Chosen.words` is `+r NAME` or `+c NAME`, and `Drop` has no words. The path is kept only in the sheet's line and in the compositor's pill. |
| 3 | **Confirmed.** `drop_carried` refuses a target whose record is untrusted: the entry is closed and the source gets `DragEnded`. |
| 4 | **Confirmed.** Each client has its own `presses` counter. `body_press` and `WindowPointer.serial` both use the pressed client's own count, so no client sees another's presses. |
| 7 | **Fixed, with a new issue (warning).** The line now follows the name, but `refresh` rebuilds the tree on every keystroke, and `TextField::new` (`crates/zoraya-ui/src/widgets.rs:167-173`) puts the cursor at the end. Typing or Backspace in the middle of the name moves the cursor to the end after one character: editing `nový.txt` to `nová.txt` from the middle gives the wrong name. Fix: keep the cursor across the rebuild, or update the label in place. |
| 8 | **Confirmed.** `App::drag` refuses an empty name or one over `MAX_DROPPED_NAME` before encoding and gives the entry back, and `files` closes it. `message()` still sends an empty message when encoding fails, but the `Drag` fields can no longer fail to encode. |
| 9 | **Confirmed.** `Chosen.words` is now short (at most 3 + 255 bytes, under 512). A pill over `MAX_LINE` closes the handle and answers `Cancelled(FAILED)`. When the record is full (finding 15), `Granted` can still fail while the connection was given. That case stays open, as the disposition says. |
| 10 | **Partly fixed, as the disposition says.** Every read error except `WouldBlock`, including `BufferTooSmall`, now unsubscribes and closes that launcher channel, so the spinning is fixed. The blocking `clone_as` remains, recorded in ADR-0245. |
| 11 | **Not fixed at 9113cf7.** The disposition says "fixed in 7e's invariants step", but `git diff work/7d...HEAD` still does not touch `docs/adr/invariants.md` or `docs/design.md`. INV-0167-x and INV-0170-x still read `not yet applicable`. This is still to be done before the merge. |
| 12 | **Confirmed for ordinary names, with a small gap (uncertain).** The rectangle now spans 16 px left of the pointer to 568 px right of it, and from 24 px above to 72 px below. The label starts 16 px right of the pointer and is its measured width + 20 px wide. With `text.caption = 11`, 64 wide glyphs (`W`, `M`) may measure more than about 530 px and pass the edge. Clamping the label's drawn width to `CARRIED_WIDTH` would close the gap. |
| 13 | **Confirmed, with a small gap (note).** Escape closes the entry, sends `DragEnded` and redraws, and while a drag is carried keys reach no client and `press` returns at once. But Escape leaves `body_press` set: while the left button is still held and within 500 ms of the press, the source can send `Drag` again with the same serial and restart the drag the person just cancelled. Fix: clear `body_press` on Escape. |
| 19 | **Partly fixed (note).** `plain_text` now covers record lines (`AddRoomClient`) and drag names. `granted()` (`crates/zoraya-compositor/src/lib.rs:2088`) still checks only `char::is_control`. The chooser's pill holds a file name from the volume, and a name may contain U+202E, so a bidirectional override can still reach the trusted pill through `Granted`. Fix: use `rooms::plain_text` in `granted()`. Side effect: a file opened from `files` whose path holds such a character now makes the whole `AddRoomClient` fail, and the panel's message says the record is too large. |
| 20 | **Confirmed for refusal before sending.** `App::drag` returns `Err(Some(entry))`, and `files` closes it. One latent leak remains: when `Gui::send` itself fails, the entry is neither sent nor returned (`Err(None)`). |
| 24 | **Confirmed.** The chooser channel is read every round. `Gone` unsubscribes, closes and drops the channel, and a `Chosen` with nothing asked has its handle closed. |

No new `unsafe` code. Rights narrowing does not change: `files` still drags with `READ | CLONE`, the panel's re-clone is
unchanged, and the chooser's rights are still `Access::rights()`. New issues: one warning (7, the cursor) and two notes
(13, re-drag after Escape; 19, bidirectional characters through `Granted`). Finding 11 is still to be done.
