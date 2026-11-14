name     cat
summary  print files
usage    cat [-uv] [FILE...]
program  /bin/cat
operand  [FILE...]  read  the files to print, standard input if none
option   -u --unbuffered  write without buffering
option   -v --show-nonprinting  show control characters as ^X
exit     0  done
exit     1  failed
---
cat is a native command in zosh; /bin/cat is toybox's, for scripts (type /bin/cat to run it).
Print files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  cat notes.txt
  cat a.txt b.txt

SEE ALSO
  head, tail, tee

Lets through: -u -v
