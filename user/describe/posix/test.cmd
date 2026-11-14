name     test
summary  test a condition
usage    test [EXPRESSION...]
program  /bin/test
operand  [EXPRESSION...]  text  the test: -f NAME, -d NAME, -z STR, A = B, N -eq M
exit     0  true
exit     1  false
exit     2  an error
---
Test a condition: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
The names in a test are words, not designated paths: test sees what the current directory (read) and the line's
grants show. The shell is to hand words starting with - to test as operands.

EXAMPLES
  test -f Makefile && echo yes
  test "$a" = b

SEE ALSO
  expr, sh

Lets through: (no options)
