name     seq
summary  print numbers
usage    seq [-w] [-f FORMAT] [-s SEP] [FIRST [INCR]] LAST
program  /bin/seq
operand  NUM...  text  FIRST, INCR and LAST; one to three numbers
option   -w --equal-width  pad with zeros to equal width
option   -f --format FORMAT text  printf-style format for each number
option   -s --separator SEP text  the separator, newline if not given
exit     0  done
exit     1  failed
---
Print numbers: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  seq 5
  seq 2 2 10

SEE ALSO
  expr, yes

Lets through: -w -f -s
