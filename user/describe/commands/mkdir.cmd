name     mkdir
summary  make a directory
usage    mkdir DIR...
operand  DIR...  create  the directories to make
exit     0  made
exit     1  not made
---
mkdir is a native command; /bin/mkdir is toybox's, for scripts, with its own options (help /bin/mkdir).
Makes each DIR. Its parent must exist, and DIR must not.

EXAMPLES
  mkdir project

SEE ALSO
  rm, ls
