name     printf
summary  print formatted text
usage    printf FORMAT [ARG...]
program  /bin/printf
operand  FORMAT  text  the format: %s %d %x \n
operand  [ARG...]  text  the values for the conversions
exit     0  done
exit     1  failed
---
Print formatted text: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  printf '%s=%d\n' count 3

SEE ALSO
  echo

Lets through: (no options)
