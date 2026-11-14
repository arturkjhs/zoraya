name     rmdir
summary  remove empty directories
usage    rmdir DIR...
program  /bin/rmdir
operand  DIR...  remove  the empty directories to remove
exit     0  done
exit     1  failed
---
Remove empty directories: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
No options: -p would remove parents that no operand designates.

EXAMPLES
  rmdir build

SEE ALSO
  rm, mkdir

Lets through: (no options)
