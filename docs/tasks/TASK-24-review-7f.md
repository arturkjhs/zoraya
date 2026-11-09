# TASK-24 stage 7f: independent review

Scope: `git diff work/7e...work/7f` (commits c9ed4df..0abcb2f): list settings (ADR-0171), the desktop's settings
(ADR-0169), the settings window, the editor, the clipboard (ADR-0170) and `zoraya-app`'s theme handling. Method:
reading the code and the three ADRs. No tests, QEMU or mutation runs were made for this review, so every finding below
comes from reading the code. Where a finding depends on behaviour outside the diff, the code that shows it is cited.

## Summary

The trust boundary mostly holds. `ApplySettings` is accepted only on the client-manager channel, and a client channel
cannot send it. An `Offer` is taken only from the client that was just sent `Copy`, within 500 ms and only once. `Paste`
goes only to the focused window on System+V and is not sent when the clipboard is empty. The compositor gets `/config`
read-only and closes it after reading. `edit` gets a file opened from the file manager read-only, because the panel
reduces whatever it is given to `READ|CLONE`. The settings window gets `/config` narrowed to `settings` with exactly
ADR-0169's rights. The diff adds no `unsafe` block. Both new library crates have `forbid(unsafe_code)`, and the binaries
only have `#[unsafe(no_mangle)]`, as before.

The two errors are size mismatches in the clipboard. The protocol allows 32 KiB, but the compositor reads at most 4 KiB
and writes events into a 512-byte buffer. So copying more than about 4 KiB disconnects the copying application: `edit`
quits and its unsaved text is lost. A paste of more than about 490 bytes is silently not delivered. INV-0170-5 is marked
"checked" in the table, but in practice it does not hold. The warnings are:

- a paste into an untrusted room's window is not held back, while a drop there is (INV-0172-3);
- the settings window writes stale values over changes the shell made while it was open;
- `edit` drops unsaved changes on Open… without asking;
- `edit` truncates files longer than 4 Mi characters without saying so and then saves the truncated text;
- INV-0169-4 is marked "checked" although `files` holds the volume root with every right, `/config` included. ADR-0168
  allows this, but neither the invariant nor its check reflects it.

Counts: 2 errors, 5 warnings, 9 notes.

## Findings

1. **error.** The compositor cannot read an `Offer` over about 4 KiB, and drops the client that sends one.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:428` (`message: vec![0; 4096]`), `:2416-2420`
     (`client_message`: `Incoming::Refused` leads to `drop_client`), `user/src/bin/compd.rs:60-70` (any read error
     except WouldBlock and PeerClosed is `Refused`; the kernel's `channel_read` returns `BufferTooSmall` when the
     message is larger than the buffer, `kernel/src/syscall.rs:747-749`), `crates/zoraya-app/src/lib.rs:361`
     (`App::offer` allows up to `MAX_CLIPBOARD`, 32 KiB), `crates/zoraya-proto-gui/src/lib.rs:255,475`.
   - **What is wrong:** `Offer` can carry 32 KiB, but the compositor's read buffer is 4096 bytes.
   - **Scenario:** In `edit`, select about 5 KB of text and press System+C. `edit` sends `Offer` (about 5 KB). The
     compositor's read fails, `drop_client` closes the channel, `App::call` returns `None`, the next `poll` gives
     `Happened::Gone`, and `edit` returns 0 (`user/src/bin/edit.rs:153`). The window disappears and unsaved text is lost.
     The fake platform in the tests behaves the same way (`tests.rs:128`, `m.len() > bytes.len()` gives `Refused`). The
     clipboard test only uses short strings.
   - **Fix:** Pick one bound and use it everywhere. Either make the compositor's read buffer at least `HEADER_SIZE + 4 +
     MAX_CLIPBOARD` (and `zoraya-app`'s receive buffer the same), or lower `MAX_CLIPBOARD` below what both buffers hold.
     Add a compositor test that offers `MAX_CLIPBOARD` bytes and expects `OK`, and one with `MAX_CLIPBOARD + 1` bytes
     that expects a refusal while the client stays connected (INV-0170-5 says "refused", not "disconnected").

2. **error.** A `Paste` over about 490 bytes is sent as an empty message, so the paste is silently lost.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:349` (`out: [u8; 512]`), `:352-355` (`reply` turns an encoding
     failure into length 0: "Every message here is small and fixed"), `:958-965` (the `Paste` sent through
     `send_event_plain`, which writes `&self.out[..0]`).
   - **What is wrong:** The clipboard can hold up to 4 KiB (and 32 KiB once item 1 is fixed), but `encode` into 512
     bytes fails for any text longer than 512 − 16 (header) − 4 (window) − 4 (length) bytes.
   - **Scenario:** Copy a 600-byte paragraph in `edit` (accepted), then press System+V. The client gets a 0-byte
     message, which it ignores. Nothing is pasted, and no one is told.
   - **Fix:** Encode `Paste` into a buffer sized for `MAX_CLIPBOARD`: a separate `Vec`, or make `out` that large. Make
     `reply` refuse to write a 0-length message instead of sending it. Add a test that pastes `MAX_CLIPBOARD` bytes and
     checks the bytes that arrive.

3. **warning.** A paste into a window of an untrusted room is delivered without the confirming card.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:956-966`; compare with the drop path at `:1117-1125`, which leaves
     out untrusted windows.
   - **What is wrong:** ADR-0172 point 6 and INV-0172-3 (accepted) say "a drop or a paste (ADR-0170) into an untrusted
     window is delivered only after the person confirms it". The drop path checks `c.record.untrusted`. The paste path
     sends `Paste` to whatever window has focus.
   - **Scenario:** Not reachable today, because `Panel::pressed` refuses every launch in an untrusted room. As soon as
     untrusted launches are allowed, System+V into such a window gives it the clipboard without confirmation. The
     ADR-0170 check note mentions the drop half only.
   - **Fix:** Apply the same rule as for drops. Send nothing to a client whose record is `untrusted` until the card
     exists, and add a test for it. Optionally mention the paste half in INV-0172-3's open-question note.

4. **warning.** The settings window writes over changes other writers made while it was open.
   - **Where:** `user/src/bin/settings.rs:113,121-127` and `crates/zoraya-settings-window/src/lib.rs:135-160`.
   - **What is wrong:** `s` is read once at start and afterwards only replaced by the window's own result. `changed(&s,
     ...)` builds a complete `GuiSettings` from that stale `s`. `written` then compares all six keys against the file as
     it is now, and writes every key that differs, not only the one the person changed.
   - **Scenario:** The settings window is open with motion "yes". In a terminal: `settings gui.motion no`. The person
     then switches the theme to dark in the window. `written` sees motion "no" in the file and "yes" in `new`, and
     writes `gui.motion = yes` along with the theme. The shell's change is silently undone. This breaks ADR-0169
     driver 1 ("see each other's changes") and the spirit of INV-0169-3 (a line the person did not change in the window
     is rewritten).
   - **Fix:** Before applying the change, read `s` again from the current bytes (`s = settings::read(&current).0`, then
     `changed(&s, …)`), or have `changed` return only the key it changed and write just that key. The page should also
     be refreshed from the file.

5. **warning.** `edit` throws away unsaved changes on Open… without asking.
   - **Where:** `user/src/bin/edit.rs:202-209` and `:295-305`.
   - **What is wrong:** Open… sends the chooser request whether or not the buffer is modified. On `Chosen`, the buffer is
     replaced (`*buffer.borrow_mut() = Buffer::new(...)`). Only a close asks the question.
   - **Scenario:** Type for ten minutes, click Open… and choose another file. The typed text is gone, and undo cannot
     bring it back because the new `Buffer` has no history.
   - **Fix:** If `buffer.borrow().modified()`, show `question(&name)` first, as the close path does, and continue
     according to the answer.

6. **warning.** `edit` truncates a file longer than `MAX_CHARS` without saying so, and then saves the truncated text.
   - **Where:** `crates/zoraya-editor/src/lib.rs:63` (`text.chars().take(MAX_CHARS)`), `user/src/bin/edit.rs:116,298`.
   - **What is wrong:** `read_all` allows up to 16 MiB, and the buffer keeps 4 Mi characters. The rest is dropped
     without a status message. Saving writes the shorter text and sets the file's size (`write_all` ends with
     `SetSize`).
   - **Scenario:** Open a 6 MB ASCII log through the chooser, fix one line, and Save to the same name. The last 2 MB of
     the file are gone.
   - **Fix:** Refuse to open a file over the limit (with a status message), or open it with Save turned off. At the
     least, report "only the first N characters were opened" and mark the buffer as modified, so the close question
     appears.

7. **warning.** INV-0169-4 is marked "checked", but `files` holds `/config` with every right.
   - **Where:** `crates/zoraya-panel/src/lib.rs:174-182` (`files` gets `Give::Root` and `+w /`),
     `user/src/bin/panel.rs:288-293` (`clone_as(ALL)`), `crates/zoraya-panel/src/tests.rs:236-267`, and the INV-0169-4
     row in `docs/adr/invariants.md`.
   - **What is wrong:** INV-0169-4 says "No launch record the panel builds carries `/config` with a writing right except
     the settings window's". The test only checks for `Give::Config`. `files` gets the volume root with `ALL`, which
     includes `/config/settings` and `/config/rooms`. INV-0168-2 (accepted) allows `files` to hold the root, so the two
     accepted invariants conflict. 7f marks INV-0169-4 as checked without recording that conflict.
   - **Scenario:** A bug in `files`, or a drag/rename the person makes there, can rewrite `/config/settings` without the
     writer crate or the atomic publish. The table says this cannot happen.
   - **Fix:** Record the conflict in ADR-0169's open questions (rule 6 note), for the architect to decide. Have the test
     either assert the known exception (`files` through `Give::Root`) or narrow `files`' root so that `/config` is not
     writable.

8. **note.** `SettingsChanged` is accepted on every launcher channel, and the settings window can also send `OpenWith`.
   - **Where:** `user/src/bin/panel.rs:396` and `crates/zoraya-panel/src/lib.rs:184-196`.
   - **What is wrong:** ADR-0169 describes "a channel to the panel that the settings program alone receives". In the
     code, the settings window gets the same `Give::Launcher` channel as `files`, and the panel accepts both requests
     from both programs. Nothing is gained this way: `SettingsChanged` has no payload, the panel reads the file itself,
     and `OpenWith` reduces the given handle to `READ|CLONE`. It is still more than the ADR describes.
   - **Fix:** Record per launcher which requests it may make (settings: only `SettingsChanged`; files: only `OpenWith`)
     and answer `INVALID` to the others.

9. **note.** `SettingsApplied` does not carry the compositor's answer, contrary to its documentation.
   - **Where:** `crates/zoraya-proto-chooser/src/lib.rs:124-133` and `user/src/bin/panel.rs:352-372,397-405`.
   - **What is wrong:** The documentation says the status is "the compositor's answer … `INVALID` when it refused the
     values". The panel's `send` uses `NO_REPLY` and only reports whether `channel_write` succeeded. So the window shows
     "Saved and applied." even if the compositor refused. In practice the panel's values come from the same reader, so a
     refusal should not happen.
   - **Fix:** Correct the documentation, or have the panel wait for `ManagerDone` and pass its status on.

10. **note.** Doc comments are attached to the wrong items.
    - **Where:** `crates/zoraya-proto-gui/src/lib.rs:375-394` and `user/src/bin/panel.rs:349-352`.
    - **What is wrong:** `ApplySettings` was inserted between `AddRoomClient`'s doc comment and `AddRoomClient`. As a
      result, `ApplySettings` carries both texts and `AddRoomClient` has none. In the same way, `apply_settings` in
      `panel.rs` now carries the "`OpenWith` from a file manager" comment above its own.
    - **Fix:** Move the inserted items above the existing doc comments.

11. **note.** `copy_asked` is not cleared when its client goes away.
    - **Where:** `crates/zoraya-compositor/src/lib.rs:969,1053-1066`; `drop_client` does not reset it.
    - **What is wrong:** If the client that was sent `Copy` closes within 500 ms, and a new client takes the same slot
      (the lowest free slot, `add_client`), the new client can `Offer` and set the clipboard without having been asked.
      The window is narrow but real (INV-0170-2).
    - **Fix:** In `drop_client`, set `copy_asked = None` when it names the slot being dropped.

12. **note.** `Gui::start` leaks the `/config` handle on its early returns.
    - **Where:** `user/src/gui.rs:248` (`start_driver("gpud", …)?`), `:266-272` (the `channel_create` failure path) and
      `user/src/bin/init.rs:419-430`.
    - **What is wrong:** `config` is moved into `entries` only just before `start("compd")`. The earlier returns do not
      close it. This is a one-time leak, because `gui_allowed` then turns false. In the same function,
      `ends.iter().take(4)` now silently leaves out (and leaks) input driver ends beyond the fourth. The old code gave
      the fifth slot 11, so it is better than before, but those ends are neither closed nor reported.
    - **Fix:** Close `config` (and the extra ends) on every failure path, or open `config` only after the drivers have
      started.

13. **note.** The editor view ignores tabs for the selection and for clicks.
    - **Where:** `crates/zoraya-editor/src/view.rs:120-128` and `:226-229`.
    - **What is wrong:** The text and the cursor draw a tab as four cells (`:137`, `:142-146`). The selection rectangle
      and the click-to-column mapping count a tab as one cell. On a line that starts with a tab, the highlight starts 3
      cells too far left, and clicking places the cursor 3 characters too far right.
    - **Fix:** Map character index to display column (and back) with one helper that expands tabs, and use it in all
      three places.

14. **note.** `edit` can corrupt files that are not UTF-8, and suggests a meaningless name when saving.
    - **Where:** `user/src/bin/edit.rs:116,117,298`.
    - **What is wrong:** `String::from_utf8_lossy` replaces invalid bytes with U+FFFD. Saving then writes the
      replacements, without warning. Separately, a file opened from the file manager is named "the opened file", and
      that text is what the Save sheet suggests as the file name (`ask(c, Some(&name), …)`).
    - **Fix:** Refuse to open (or refuse to save) text that is not UTF-8, with a status message. Pass the display name
      to `edit` (the launch has the words `+r ~/a.txt`) and suggest that name.

15. **note.** The clipboard ignores the find field; copy and paste always use the editor buffer.
    - **Where:** `user/src/bin/edit.rs:175-189`.
    - **What is wrong:** With focus in the Find field, System+V inserts into the document at its cursor, not into the
      field. ADR-0170 says `zoraya-ui` should handle copy and paste for `TextField`, and that is not done yet.
    - **Fix:** Send `Copy` and `Paste` to the focused widget, or record the limitation in `docs/design.md`.

16. **note.** The `settings` command allows combinations that the compositor later reports and corrects.
    - **Where:** `user/src/bin/zosh.rs:2474-2530`.
    - **What is wrong:** Commands such as `settings gui.keyboard-layout ru` (with only `us` enabled) and `settings
      gui.keyboard-layouts -us` (while `us` is the active layout) are accepted. ADR-0169 and ADR-0171 allow this: the
      compositor reports it and uses the first layout. Two other limits:
      - There is no command form for an empty list: `settings NAME ''` gives a list with one empty item, and ADR-0171
        writes the empty list as `NAME =`. No current declaration allows 0 items.
      - A list edit applied to a bad line in the file starts from the default.
    - **Fix:** None needed for this stage. Consider a warning from `settings` when the active layout is not in the list.

## Dispositions

By the main model, after the review.

| # | Disposition |
|---|---|
| 1 | Fixed. The compositor reads a client's message into a buffer of the protocol's largest message (64 KiB), and `zoraya-app` encodes and reads with the same bound; a 20 KiB clipboard is offered and pasted whole (test). INV-0170-5 now holds as marked. |
| 2 | Fixed with 1: the compositor's event buffer is the largest message too. |
| 3 | Fixed. No paste into an untrusted room's window until the confirming card exists, as for drops. |
| 4 | Fixed. The settings window applies each change to the file as it is at that moment, so a `settings` command run meanwhile is kept. |
| 5 | Fixed. Open over unsaved changes asks first; a second Open discards them. |
| 6 | Fixed. A file larger than the editor holds is not opened (no silent cut on save). |
| 7 | For the architect: INV-0168-2 gives `files` the volume's root, which includes `/config`; INV-0169-4 says no launch but the settings window gets `/config` to write. Recorded in the report's queue; the panel's test checks the `Config` slot only. |
| 8 | Fixed. The panel takes `OpenWith` only from the file manager's channel and `SettingsChanged` only from the settings window's. |
| 9 | Fixed: `SettingsChanged`'s doc says what `SettingsApplied` carries (whether the panel sent the settings on). |
| 10 | Fixed: the doc comments are on their items again. |
| 11 | Fixed. A pending `Copy` is cleared when its client goes away. |
| 12 | Open (note): `Gui::start`'s early failures; recorded. |
| 13 | Open (note): tabs in the selection and the press position; recorded for the editor's next step. |
| 14 | Fixed. A file that is not UTF-8 is not opened (saving would have corrupted it); the name suggested for a file opened from the file manager is `untitled.txt`. |
| 15 | Open (note): copy and paste in the toolkit's text fields; recorded in design.md's "not built". |
| 16 | As the ADRs allow; recorded. |

## Check of the fixes

By the reviewer, reading `git show 1f5fe7c`. No tests were run for this check. "Confirmed" means the changed lines fix
the finding and add no new problem in the review's areas (trust boundary, rights narrowing, `unsafe`; the commit adds
no `unsafe`).

- **1 (offer over 4 KiB): confirmed, but the fix causes the new issue under 2.** The compositor's `message` buffer and
  `zoraya-app`'s send and receive buffers are now `MAX_WIRE_MESSAGE` (64 KiB), the kernel's `MAX_MESSAGE`. A 32 KiB
  `Offer` fits. The new test covers 20 KiB.
- **2 (paste over 490 bytes): confirmed, with a new issue (error).** `out` is now a 64 KiB `Vec`, so `Paste` is encoded
  whole. But `term` reads the compositor's channel into an 8192-byte buffer (`user/src/bin/term.rs:436`). It treats any
  read error except WouldBlock as fatal: `Err(_) => zoraya_user::process_exit(1)` (`:277-285`).
  - *Scenario:* copy more than about 8 KiB in `edit`, focus a terminal and press System+V. `channel_read` returns
    `BufferTooSmall` and `term` exits, ending the shell session. Before the fix, the paste was an empty message that
    `term` ignored.
  - *Fix:* give `term`'s buffer the size `MAX_WIRE_MESSAGE`, or have the compositor send `Paste` only to clients that
    can read it. Also check every other compositor client that does not use `zoraya-app` (`t_gui`, `guibench`) for the
    same pattern.
- **3 (paste into an untrusted window): confirmed.** `Paste` is not sent when the focused client's record is
  `untrusted`, the same test the drop path makes. `Copy` is still sent to such a window, which ADR-0172 allows.
- **4 (stale values in the settings window): confirmed, with one small new issue (note).** `s` is read again from the
  file before `changed`, so a change made in a terminal meanwhile is kept. The controls send absolute values (theme,
  motion, scale, layout check boxes), except `ACTIVE`, which sends an index into the list the old page showed.
  - *Scenario:* if `gui.keyboard-layouts` changed in the file since the page was built (`us ua` became `cz us ua`), a
    click on "Ukrainian" (index 1) selects `us`.
  - *Fix:* compare the old page's list with the re-read `s.layouts` and refuse or rebuild the page when they differ,
    or give each `ACTIVE` choice its layout's name.
- **5 (Open discards unsaved changes): confirmed, with one gap (note).** The first Open over unsaved changes now only
  arms a warning, and nothing is lost until the chooser returns a file. But `discard_armed` is reset only by the next
  Open or Save as.
  - *Scenario:* click Open once (warning shown), keep typing, and much later click Open. The chooser opens without a
    new warning, and the new text is discarded.
  - *Fix:* reset `discard_armed` on `Response::Changed(TEXT)`, on Save and on Find, so the warning applies only to the
    click right after it.
- **6 (file over 4 Mi characters cut on save): confirmed.** `text_of` refuses a file of more than `MAX_CHARS`
  characters, on both the file-manager path and the chooser path, and the status says why.
- **8 (launcher requests): confirmed.** The panel records which program each launcher channel was given to (`l.app`).
  It takes `SettingsChanged` only from `settings` and `OpenWith` only from `files`. Any other request is answered
  `INVALID` and its handles are closed, and the `OpenWith` path still reduces the given handle to `READ|CLONE`.
  `watch_launcher` has a single caller, which was updated.
- **9 (`SettingsApplied` documentation): confirmed.** The doc now says what the status carries.
- **10 (misplaced doc comments): confirmed.** The `AddRoomClient` and `OpenWith` comments are back on their items.
- **11 (pending `Copy` after its client leaves): confirmed.** `drop_client` clears `copy_asked` for its slot. It is the
  only place that empties a client slot (`clients[slot].take()`).
- **14 (non-UTF-8 files; the suggested name): confirmed.** A file that is not UTF-8 is not opened. The suggested name is
  now `untitled.txt`, so the window title of a file opened from the file manager also says `untitled.txt`. That is
  accurate for a read-only grant, though a little odd.
- **Performance (note, not a correctness problem).** `App::poll`, `App::wait` and `App::call` now allocate and zero
  64 KiB on every call, and `message()` allocates 64 KiB on every send. `edit` calls `poll` in a loop on every wake-up.
  One buffer kept in `App` and reused would avoid this.

### After the check

- The new error (a paste over 8 KiB ended a focused terminal): fixed; `term` reads into a buffer of the largest
  message.
- 4's gap: the layout in use is resolved by its place in the list the page showed, then applied only if the file
  still enables it.
- 5's gap: any edit after the warning needs a new one.
- Performance: a sent message is encoded into 1 KiB first and into the largest only when it does not fit; the 64 KiB
  receive buffer per call remains, recorded as a note.
