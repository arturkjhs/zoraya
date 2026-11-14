name     expr
summary  evaluate an expression
usage    expr EXPRESSION...
program  /bin/expr
operand  EXPRESSION...  text  the words: 1 + 2, STR : PATTERN, A \< B (toybox has no length, substr, index or match)
exit     0  the value is not 0 or empty
exit     1  the value is 0 or empty
exit     2  an error
---
Evaluate an expression: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
toybox's expr is from its pending directory, admitted by ADR-0336 item 2 after a review and patch 0003: arithmetic
that would overflow a 64-bit integer fails with "overflow" (exit 2), and comparisons do not wrap. A number is read
leniently: '' counts as 0, blanks and a sign before it are taken, and an out-of-range number is clamped. Write a * as '*'.

EXAMPLES
  expr 1 + 2
  expr foo.c : '.*\.c'

SEE ALSO
  test, sh

Lets through: (no options)
