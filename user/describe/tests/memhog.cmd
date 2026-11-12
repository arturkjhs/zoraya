name     memhog
summary  take memory until the job refuses, for the room tests
usage    memhog [--hold SECONDS]
option   --hold SECONDS text  after the refusal, keep the memory this long (default: exit at once)
cwd      none
exit     42  the job refused the memory (OverQuota)
---
Maps 64 KiB at a time until the job it runs in is over its memory limit, says
how much it got and exits with 42. For tests/shell/rooms.script (INV-0263-4,
INV-0266-1). Without a limit it stops after 256 MiB. With --hold it keeps what
it got for that many seconds before it exits (tests/gui/limits.script: the
desktop's pill at 90%).
