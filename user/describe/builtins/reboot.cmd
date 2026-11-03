name     reboot
summary  start the machine again
usage    reboot
cwd      none
exit     1  not available
---
Writes everything to the disk and starts the machine again. Until the kernel has a call to reset the
machine (proposed in ADR-0159), it says that it is not available.

SEE ALSO
  poweroff
