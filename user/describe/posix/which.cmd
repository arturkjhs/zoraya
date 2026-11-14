name     which
summary  find a command
usage    which [-a] NAME...
program  /bin/which
operand  NAME...  text  the commands to look for in PATH
option   -a --all  print every match, not only the first
exit     0  done
exit     1  failed
---
Find a command: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  which cc

SEE ALSO
  env

Lets through: -a
