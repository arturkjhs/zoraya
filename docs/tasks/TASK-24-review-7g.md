# TASK-24 stage 7g: independent review

Scope: `git diff work/7f...work/7g` (commits 10493f6..2227eb3): ADR-0246 (proposed), the display event and the second
resource in `zoraya-virtio-gpu` and `gpud`, `Resized` in `zoraya-proto-display`, the compositor's re-attach and
re-layout, `compd`'s release of the old framebuffer, the scale applied at once from the settings, `run-qemu.sh
--gui-size` and the two new `make test-gui` boot sizes. Method: I read the code against ADR-0246, ADR-0164, ADR-0090,
ADR-0093, ADR-0169 and ADR-0054 (pins). I ran the host tests of `zoraya-virtio-gpu` (17 passed),
`zoraya-proto-display` (3 passed) and `zoraya-compositor` (115 passed, 2 ignored), and `make adr-check` (0 errors).
QEMU was not run, so every finding about the running system comes from reading the code. Where a finding depends on
code outside the diff, that code is cited.

## Summary

The device side is sound. `Gpu::attach` makes the other resource (`3 - r`, only ever 1 or 2), backs it, points scanout
0 at it and only then sends `RESOURCE_UNREF` for the old one. `gpud` unpins the old pin only after that unref has
succeeded, so the device never reads memory that has been unpinned. `flush` checks the rectangle against the size of
the resource on screen, not against the newest display size, so a flush sent between `Resized` and the new `Attach`
cannot leave the resource. `compd` unmaps an older framebuffer only after the driver shows a newer one, and a pin holds
its memory object anyway (ADR-0054), so releasing it early could not free memory the device still reads. Sizes go
through `u64` products, and `reconfigure`'s `clamp` calls cannot panic: each upper bound is raised to at least its
lower bound. The diff adds no `unsafe` block.

The protocol between `gpud` and the compositor is where it fails. The compositor's synchronous `call` takes the first
message on the channel as its reply. `gpud`, meanwhile, sends a second `Resized` immediately after every `InfoReply`.
On every real resize, therefore, the compositor reads that `Resized` as the `Attach`'s `Done`, drops the driver, and
`init` restarts it. A driver is restarted at most three times, so the fourth resize leaves the desktop without a
display (item 1). Two more errors remain once item 1 is fixed:

- `Attach` uses the device's newest size, not the size the compositor was told (item 2);
- a display that returns to its attached size makes the compositor's `Attach` fail (item 3).

The host tests pass, but they cannot show any of the three. The fake driver answers `call` outside the channel and
accepts every `Attach`. And the logic that decides when `gpud` sends `Resized` lives in the binary, which has no host
test at all (item 7).

Counts: 3 errors, 5 warnings, 6 notes.

## Findings

1. **error.** Every resize sends a second `Resized`, and the compositor reads it as the `Attach` reply. The compositor
   then drops the driver, and after three restarts there is no display.
   - **Where:** `user/src/bin/gpud.rs:273-274` (`Info` sets `told = false`), `:221-234` (`display_event` sends when
     `pin` is set, `!told` and `size != attached`), `:389-396` (`display_event` runs after every `serve`);
     `user/src/bin/compd.rs:190-216` (`call` writes the request, waits until the channel is readable and returns the
     first message, whatever it is); `crates/zoraya-compositor/src/lib.rs:705-711` (`done_ok` fails on anything but
     `Done`), `:653-663` (`display_resized` then calls `lose_display`); `docs/design.md:395` ("restarted alone, at
     most three times each").
   - **What is wrong:** `told` is meant to keep `Resized` from standing where the compositor waits for a reply, as the
     comments at `gpud.rs:98-101` and `lib.rs:755-756` say. But it is cleared by the very `Info` that starts the
     compositor's wait. Right after `gpud` answers `Info`, `display_event` sees that the size (the new one) still
     differs from `attached` (the old one, which changes only when the `Attach` succeeds). It sends `Resized` again. This
     needs no race: the second `Resized` is written before `gpud` even reads the `Attach`, so it always comes before
     the `Done` on the channel.
   - **Scenario:** The window is enlarged from 1024 × 768 to 1600 × 1000. `gpud` sends `Resized(1600, 1000)`. The
     compositor calls `Info` and gets 1600 × 1000, and `gpud` at once sends `Resized(1600, 1000)` again. The compositor
     makes the framebuffer and calls `Attach`. `call` returns the queued `Resized`, `decode::<Done>` fails with
     `WrongOrdinal`, `size_and_attach` returns `UNSUPPORTED` and `lose_display` closes the channel. `gpud` still attaches
     the new framebuffer, sees the channel close, resets the device and exits. `init` restarts it, and the compositor
     attaches the new driver at 1600 × 1000. The result looks right, but it is ADR-0246's rejected Option B, a device
     reset per step, and after the third restart the driver stays dead. ADR-0246 notes that cocoa reports every step of
     a drag, so one drag can use up all three restarts.
   - **Fix:** Fix both ends:
     - **`gpud`:** Clear `told` when an `Attach` completes, not on `Info`. After the `Done`, send one `Resized` if the
       newest size differs from the attached one. So nothing unasked is sent between an `InfoReply` and the next
       `Done`.
     - **Compositor:** Make the compositor's `call` (or `compd`'s) set aside a message that is not the reply (a
       `NO_REPLY` transaction, or ordinal 4) as "resized pending", instead of returning it.
     - **Tests:** Give the fake driver an inbox that `call` reads in order, and add a test that queues a `Resized` ahead
       of the `Done`.

2. **error.** `Attach` uses the device's newest size, not the size given in `InfoReply`. A size change between the two
   either drops the driver or leaves a sheared picture that stays.
   - **Where:** `user/src/bin/gpud.rs:212-220` (`display_event` updates the size between any two messages, so also
     between `Info` and `Attach`), `:189`; `crates/zoraya-virtio-gpu/src/lib.rs:299-326` (`attach` uses
     `self.width × self.height` for the resource and the coverage check).
   - **What is wrong:** The compositor makes its framebuffer at the size in `InfoReply`. The driver makes the resource
     at whatever `GET_DISPLAY_INFO` gave last. If the device's size changes in between, the two disagree.
   - **Scenario:** During a drag, `InfoReply` says 1600 × 1000, and before the `Attach` arrives a display event gives
     1700 × 900:
     - **The picture shears.** 1700 × 900 × 4 is less than the 6.4 MB the framebuffer has, so the check passes.
       Resource 2 is made at 1700 × 900 over memory the compositor writes at a stride of 1600. The picture is sheared.
     - **The flushes fail silently.** Every flush of the compositor's full 1600 × 1000 rectangle is `BAD_RANGE`, which
       the compositor does not check (`lib.rs:741-748` takes any `Done`).
     - **Nothing repairs it.** `attached` becomes 1700 × 900, which equals the size, so no `Resized` follows. It stays
       like this until the next display event.
     - **A larger size drops the driver.** If the new size were larger, the attach would be `BadBacking`, then
       `BAD_BUFFER`, and the driver would be dropped as in item 1.
   - **Fix:**
     - **`gpud`:** Record the size sent in each `InfoReply`, and make the next `Attach` use exactly that size.
       `Gpu::attach` should take the width and height, checked against `MAX_SIDE`, instead of reading `self.width`.
       After the `Done`, send `Resized` if the device has moved on since.
     - **Test:** Add a `zoraya-virtio-gpu` test where `display_info` runs between the reported size and `attach`.

3. **error.** A display that returns to the attached size before the compositor asks makes the `Attach` fail, and
   the driver is dropped.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:653-663,667-712` (`display_resized` always attaches);
     `user/src/bin/compd.rs:141-145` (`framebuffer` at the same size returns the same object); `user/src/bin/gpud.rs:193`
     and `crates/zoraya-virtio-gpu/src/lib.rs:299-304` (an `Attach` at the size already shown is `Attached`, then
     `BAD_BUFFER`).
   - **Scenario:** The display is attached at 1024 × 768, and the window is dragged to 1100 × 800:
     1. `gpud` sends `Resized`, and `told` is set.
     2. A flush is in flight, so the compositor only sets `resized`.
     3. The drag comes back to 1024 × 768. `gpud` reads the new size, but does not send again (`told`).
     4. The flush's `Done` arrives, and `display_resized` calls `Info`, which gives 1024 × 768. `new_size` is false
        and `framebuffer` duplicates the current object.
     5. The `Attach` is refused with `BAD_BUFFER`, and `lose_display` drops the driver.
   - **Fix:** In `display_resized`, do nothing more when `Info` gives the size already attached (`!new_size`). Keep the
     unconditional attach for a new driver (`attach_display`). Let the fake driver refuse an `Attach` at the size
     already attached, as `gpud` does, and test the round trip.

4. **warning.** Popups and sheets do not follow the re-layout: popups stay where they were, and neither is told the new
   scale.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:1716-1718` (`reconfigure` skips every non-toplevel), `:487-488`
     (the comment "popups and sheets follow their owners"), `:2925-2928` (a popup's `x`, `y` are absolute: the parent's
     body plus the offset at creation), `:1396-1408` (a drag moves the popups by hand, which `reconfigure` does not);
     `:1744` (`configure` sends nothing to a non-toplevel).
   - **Scenario:** A terminal at x = 1300 on a 1600-wide display has a menu open, which is focused. The window is
     shrunk to 1024 wide. The toplevel is clamped left by several hundred pixels, but the popup keeps x ≈ 1300+, off
     the new screen. The popup still has the keyboard and cannot be seen. On a scale change from the settings window,
     the parent's position doubles and the popup stays. The popup's buffer, and a sheet's, keep the old scale's
     physical size and are drawn at half or double their size until the client happens to commit again. `place_sheets`
     re-centres the sheets, but nothing tells them the scale.
   - **Fix:** In `reconfigure`, close every popup (`close_popups_of` for each toplevel), as toolkits close menus on a
     configure. Or, when only the size changes, move each popup by its parent's delta, and close popups only on a scale
     change. For sheets, send the owner's sheet a `Configure` or close it with `CloseRequested`. Correct the comment.
     Add a test with an open popup across a resize.

5. **warning.** Legacy windows are not moved back into the work area, so one can be left wholly off screen.
   INV-0246-1 says "every window's frame".
   - **Where:** `crates/zoraya-compositor/src/lib.rs:1716-1718` (a legacy window is skipped before the clamp);
     `crates/zoraya-compositor/src/window.rs:102-107` (legacy windows are framed, so the invariant covers them).
   - **Scenario:** A legacy surface sits at x = 1400 on a 1600-wide display, and the display shrinks to 1024 × 768. Its
     `x`, `y` stay, the whole frame is past the right edge, and nothing on the screen can reach it.
   - **Fix:** Clamp the position of legacy windows, which keep their buffer and size, in the same loop. Extend the
     host test's `inside` check to a legacy window.

6. **warning.** A scale set in the settings window blurs the wallpaper at the old scale's radius.
   - **Where:** `crates/zoraya-compositor/src/lib.rs:484-490`: `make_wallpaper()` runs before `self.scale =
     self.chosen_scale()`, and `make_wallpaper` uses `glow::PANEL_BLUR.physical(self.scale)`
     (`crates/zoraya-compositor/src/draw.rs:54`).
   - **Scenario:** On a 2048 × 1536 display, a person chooses scale 1 in Settings. The panels' glass is now blurred with
     the scale-2 radius, twice ADR-0161's. It stays that way until the next change of size or appearance. The reverse
     change gives half the radius. In `size_and_attach` the order is right (`:697-699`).
   - **Fix:** Set `self.scale` before calling `make_wallpaper`. A test can compare `blurred` after the settings change
     with `blurred` after a fresh attach at that scale.

7. **warning.** The rule INV-0246-2 states is not where its check looks, and the host tests cannot see the protocol's
   ordering. That is why items 1 to 3 pass.
   - **Where:** ADR-0246 INV-0246-2 ("*Check:* host test of `zoraya-virtio-gpu`"); `user/src/bin/gpud.rs:207-242`
     (the rule: sent only after an attach, only for a size that differs, at most one before `Info`);
     `crates/zoraya-virtio-gpu/src/tests.rs:433-526` (tests the event bit and the second resource only);
     `crates/zoraya-compositor/src/tests.rs:258-291` (`Fake::call` answers directly, never from the channel, and
     accepts every `Attach`).
   - **What is wrong:** The decision to send `Resized` is logic that needs no hardware, but it lives in the `gpud`
     binary, which has no host test. CLAUDE.md asks for such logic in a `no_std` crate. No test covers the "differs from
     the size it last reported" half of INV-0246-2. The code also compares with the attached size rather than the size
     last reported (item 1 sends the same size twice). The compositor tests prove the re-layout, but not that it works
     against a driver that behaves like `gpud`.
   - **Fix:** Move the `told`/`attached`/offered-size state (items 1 and 2) into `zoraya-virtio-gpu`, or a small state
     type beside it, with host tests that drive Info, Attach and display events in each order. Then make the
     compositor's fake driver queue its replies and refuse a same-size `Attach`.

8. **warning.** `docs/design.md` still describes a display whose size is fixed.
   - **Where:** `docs/design.md:362-367` (`gpud` serves `Info`, `Attach` and `Flush`, with no display event and no
     `Resized`); `:492-495` (scale: nothing on a size change, and no `--gui-size`); `:673-674` ("the scale at the next
     session", while 7g applies it at once); `:1168-1170` ("On a Mac the window scales its picture when it is
     enlarged").
   - **Fix:** Update those four places. Line 1189 was updated, but the others contradict it.

9. **note.** ADR-0246 describes some things the code does differently.
   - **Where and what:** ADR-0246 has four of them:
     - **Option C and the Follow-up:** they say `gpud` acts "on the configuration interrupt". The code never looks at
       it: it reads `events_read` after every message and every 100 ms (`gpud.rs:32-34,389-396`). That also wakes the
       driver ten times a second while it is otherwise idle.
     - **"Maximised or tiled windows resized":** the compositor keeps no maximised or tiled state. `tile` sets a
       position once (`lib.rs:1658`), so a tiled window is only clamped like any other.
     - **"`Configure` to every window whose size or scale changed":** `reconfigure` configures every toplevel, even
       one that did not change. The test asserts exactly that (`tests_display.rs:46-50`).
     - **INV-0246-2's "the size it last reported":** the code compares with the attached size.
   - **Fix:** Bring the ADR into line with the code, or the code with the ADR. Using the interrupt, which is already
     subscribed on `port`, would remove the poll.

10. **note.** `--gui-size` lets bad sizes through.
    - **Where:** `tools/run-qemu.sh:196-202`.
    - **What is wrong:** `case $xres$yres` tests the two values joined into one string, so `1600x` passes, with `yres`
      empty. `0x0` passes, and so does `5000x100`, although the comment says 1 to 4096. QEMU then fails, or `gpud`
      reports "no display" and exits.
    - **Fix:** Check each value on its own: not empty, digits only, from 1 to 4096.

11. **note.** When an attach fails part-way at a new size, the old pin is quarantined rather than unpinned.
    - **Where:** `user/src/bin/gpud.rs:197-203`.
    - **What is wrong:** `self.pin = Some(pin)` drops the old pin's handle without unpinning it. Then `stop` resets the
      device and unpins only the new pin. The old frames stay quarantined until the successor's
      `dma_release_quarantine`. That is safe, so nothing is lost, but the comment "goes with the process" hides it.
    - **Fix:** Keep both pins and unpin both after a reset that succeeds, or say "quarantined until the successor
      releases it".

12. **note.** INV-0246-1 does not hold at the small end of the range ADR-0246 accepts, and no test resizes to 1 or
    4096.
    - **Where:** `crates/zoraya-compositor/src/rooms.rs:284-288`; `crates/zoraya-compositor/src/lib.rs:1726-1733`.
    - **What is wrong:** When the display is shorter than the bar plus a title bar, `work_area` begins below the
      screen. Every frame is then clamped to `area.y + title`, which is entirely off screen. The new tests resize to
      1280 × 800, 2048 × 1536, 640 × 360 and 800 × 600 only.
    - **Fix:** State a minimum usable size in the ADR (or clamp frames to the screen when the work area is empty), and
      add a resize to 1 × 1 and to 4096 × 4096 with a window open.

13. **note.** The index row for ADR-0246 has no Depends-on cell.
    - **Where:** `docs/adr/index.md:131`.
    - **What is wrong:** The row has four cells where the other rows have five. ADR-0246 lists ADR-0090, 0093, 0097,
      0160, 0164 and 0169.

14. **note.** The new `make test-gui` runs test booting at a size, not resizing.
    - **Where:** `Makefile:466`.
    - **What is wrong:** `terminal:auto:1600x1000` and `terminal:auto:1920x1200` only boot at those sizes, and the
      headless runs never raise a display event. So no in-QEMU test exercises `display_changed`, `Resized`,
      `RESOURCE_UNREF` or `compd`'s `release`. ADR-0246 says so ("No GUI test resizes a running guest"). Until items 1
      to 3 are fixed, a resize by hand under `make run-gui` is the only check of that path, and it would show item 1 at
      once. The task report should say so, as a known limitation.

## Dispositions

By the main model, after the review.

| # | Disposition |
|---|---|
| 1 | Fixed. When `gpud` may send `Resized` is now `zoraya-virtio-gpu`'s `Announce`: one before each `Info`, none between an `Info` and the request after it (the display's size is not even asked then), only for a size other than the attached one. Host test of `Announce`. |
| 2 | Fixed with 1: between `Info` and the next request `gpud` does not ask the device, so `Attach` uses the size `Info` gave. |
| 3 | Fixed. A size back at the attached one is not attached again: the compositor asks `Info`, sees its own size and draws the whole screen, whose flush ends `gpud`'s wait. Host test. |
| 4 | Fixed. A re-layout closes the popups (as a press outside them does), and the sheets too at a new scale; their clients are told with `CloseRequested`. Host test for popups. |
| 5 | Fixed. Legacy windows are moved back into the work area (not resized). Host test. |
| 6 | Fixed. The wallpaper is made after the scale changes. |
| 7 | Fixed with 1: INV-0246-2's check is `Announce`'s host test, and `gpud` asks it before each `Resized`. The compositor's fake still answers `call` outside the channel; the ordering it cannot see is `Announce`'s. |
| 8 | Fixed: the `gpud`, Scale and settings paragraphs and "Using the desktop" in `docs/design.md`. |
| 9 | Fixed in ADR-0246: the 100 ms look instead of the interrupt, what the re-layout does (no maximised or tiled state; every toplevel configured), INV-0246-2 as built. |
| 10 | Fixed: `--gui-size` takes only WxH, each side from 1 to 4096. |
| 11 | Fixed: the comment says the older pin stays quarantined with the device (ADR-0070). |
| 12 | INV-0246-1 now says it holds when the display is at least as large as the bar, the dock's strip and one title bar. No test resizes to 1 or 4096 (recorded). |
| 13 | Fixed: the index row's Depends-on cell. |
| 14 | Recorded in the report: the GUI tests boot at four sizes; no test in QEMU resizes a running guest (QEMU's QMP cannot). |
