name     yes
summary  repeat a line forever
usage    yes [STRING...]
program  /bin/yes
operand  [STRING...]  text  the words to repeat, y if none
exit     0  done
exit     1  failed
---
Repeat a line forever: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  yes | head -n 3

SEE ALSO
  seq, echo

Lets through: (no options)
