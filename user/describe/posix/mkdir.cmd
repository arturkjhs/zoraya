name     mkdir
summary  make directories
usage    mkdir [-pv] [-m MODE] DIR...
program  /bin/mkdir
operand  DIR...  create  the directories to make
option   -p --parents  make missing parents too (only where the line's rights reach)
option   -m --mode MODE text  the mode of the new directories
option   -v --verbose  say what is made
exit     0  done
exit     1  failed
---
mkdir is a native command in zosh; /bin/mkdir is toybox's, for scripts (type /bin/mkdir to run it).
Make directories: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  mkdir build
  mkdir -m 700 private

SEE ALSO
  rmdir, ls

Lets through: -p -m -v
