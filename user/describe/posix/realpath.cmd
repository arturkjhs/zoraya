name     realpath
summary  print a resolved path
usage    realpath [-sq] FILE...
program  /bin/realpath
operand  FILE...  read  the files whose full name to print
option   -s --no-symlinks  do not resolve symbolic links
option   -q --quiet  no error message
exit     0  done
exit     1  failed
---
Print a resolved path: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
-e and -m are not listed.
Write the short options: the long names belong to the description.

EXAMPLES
  realpath ../lib

SEE ALSO
  basename, ls

Lets through: -s -q
