name     basename
summary  strip the directory from a name
usage    basename [-a] [-s SUFFIX] NAME...
program  /bin/basename
operand  NAME...  text  the names to strip
option   -a --multiple  accept several names
option   -s --suffix SUFFIX text  also remove SUFFIX
exit     0  done
exit     1  failed
---
Strip the directory from a name: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  basename /usr/lib/x.so .so

SEE ALSO
  dirname, realpath

Lets through: -a -s
