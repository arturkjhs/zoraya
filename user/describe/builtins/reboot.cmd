name     reboot
summary  start the machine again
usage    reboot
cwd      none
exit     1  the machine cannot be reset
---
Writes everything to the disk and starts the machine again, through the firmware's reset (ADR-0159). On a
machine without one it says so and changes nothing.

SEE ALSO
  poweroff
