name     exit-with
summary  end with the exit code it is given
usage    exit-with CODE
operand  CODE  text  the exit code, 0 to 255
cwd      none
exit     2  CODE is not a number from 0 to 255
---
Ends at once with exit code CODE, so that the shell's reports and $status can be seen.

EXAMPLES
  exit-with 3; echo $status
