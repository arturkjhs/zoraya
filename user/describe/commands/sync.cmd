name     sync
summary  make written data durable
usage    sync
exit     0  everything written is on the disk
exit     1  the volume could not be written
---
Waits until everything written so far is on the disk. poweroff does this itself.

SEE ALSO
  poweroff
