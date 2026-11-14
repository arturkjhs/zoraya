name     stat
summary  show file attributes
usage    stat [-L] [-c FORMAT] FILE...
program  /bin/stat
operand  FILE...  read  the files to describe
option   -L --dereference  follow symbolic links
option   -c --format FORMAT text  print FORMAT with %n name, %s size, %a mode filled in
exit     0  done
exit     1  failed
---
stat is a native command in zosh; /bin/stat is toybox's, for scripts (type /bin/stat to run it).
Show file attributes: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  stat notes.txt
  stat -c '%n %s' notes.txt

SEE ALSO
  ls, file

Lets through: -L -c
