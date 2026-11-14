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
toybox's expr is from its pending directory, admitted by ADR-0336 item 2. Write a * as '*'.

EXAMPLES
  expr 1 + 2
  expr foo.c : '.*\.c'

SEE ALSO
  test, sh

Lets through: (no options)
