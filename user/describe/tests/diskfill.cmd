name     diskfill
summary  fill a directory until the limit refuses, and time syncs, for the room tests
usage    diskfill [--syncs N] [--loop SECONDS] DIR
option   --syncs N text  instead of filling: write and sync N times, and say the slowest
option   --loop SECONDS text  instead of filling: write and sync without end for this many seconds
operand  DIR  tree  the directory to write in
exit     42  the directory's limit (or the volume) refused the write
exit     0  the syncs were done
---
Writes files of 32 KiB blocks in DIR until a write is refused (OverQuota, or
NoSpace on a full volume), says how much it wrote and exits with 42. It stops
at 64 MiB if nothing refuses. With --syncs it writes 4 KiB and syncs, N times,
and says the slowest sync; with --loop it does the same until the time is up.
For tests/shell/gate.script (INV-0265-3, INV-0265-5).
