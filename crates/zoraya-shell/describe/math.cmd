name     math
summary  calculate
usage    math EXPRESSION...
operand  EXPRESSION...  text  whole numbers, + - x / % and parentheses; * also multiplies when quoted
cwd      none
exit     0  calculated
exit     1  not a valid expression, a division by zero, or a result too large
---
Prints the value of the expression, in whole numbers. Division rounds toward zero.

EXAMPLES
  math 1 + 2
  math 6 x 7
  math '(1 + 2) * 3'
