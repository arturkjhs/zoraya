name     stat
summary  describe a file
usage    stat PATH...
operand  PATH...  read  the files or directories to describe
exit     0  described
exit     1  a path could not be described
---
Prints for each PATH its kind, size, times of last change in seconds since the volume was made, and mode.

EXAMPLES
  stat notes.txt

SEE ALSO
  ls
