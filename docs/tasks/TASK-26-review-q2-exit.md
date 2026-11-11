# TASK-26 Q2: the `files-manage auto:2` failure at step 113 (Exit)

CI run 43014436719, attempt 2, head f16745e. Compared with the passing run 43001168090 (main, e7bb7d7).

## Finding

1. **warning**, `tests/gui/files-manage.script:143-146`. The step that failed is not a hang in the guest's power-off
   path. `Step::Exit` (`tools/test-runner/src/gui.rs:1161`) only waits for QEMU to exit, so its `timeout` is the
   scenario's deadline running out. `poweroff\r` never reached the shell, because the keys went to the Files window.
   The serial log shows it: `files: asks nothing` at 12:09:14.98, then `files: sorted by name up:` (the filter typed
   into the search hides every item) and `files: keys go to the search` at 12:09:15.77, then nothing until the
   timeout at 12:16:05. The passing run ends at `files: asks nothing` and then `PASS`. The keyboard and the tablet
   are two virtio devices, and a slow guest reads them in no fixed order. The script waits only `AFTER_CLICK`
   (150 ms) and `pause 300` after the focusing `click 85 400`, and it checks nothing before typing. `rooms.script` and
   `windows.script` check the focused title bar with `region 130 76 100 4 f7f7f8`; `files-manage.script` does not.
   In this run the guest was slow: `compd: the next 100 frames: mean 246500 us, longest 1686885 us`, against
   `118848 us / 887279 us` in the passing run. The first frame of the 1.7 s can swallow the whole 450 ms window.

   Fix: after `click 85 400`, add a `region` check of the first terminal's focused title bar (at its own position
   in this layout) before `type`, then `pause 500` as `settings.script` and `windows.script` do.

## Checked and not involved

The kernel's power-off path and the Q2 exit/teardown/reaper changes. The guest kept running and answering input
after the click (Files redrew and printed 0.8 s later), and no shell ever received `poweroff`.

## Open

Both comparable rows of this run draw frames about twice as slowly as on main (terminal 1920x1200: 49.7 ms against
31.5 ms). That is one sample on a shared runner. Compare the `compd: the next 100 frames` lines of several Q2 runs
against main before blaming Q2's interrupts-enabled teardown steps for the slowness.
