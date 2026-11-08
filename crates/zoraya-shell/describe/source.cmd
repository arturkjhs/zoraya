name     source
summary  run a script in this shell
usage    source FILE [ARG...]
operand  FILE       read  the script
operand  [ARG...]   text  its arguments, as $args
exit     0  the script ran to its end
---
Runs the commands of FILE in this shell, so that its functions and variables stay. The first
failure that nothing checks stops the script and says on which line.

EXAMPLES
  source tidy.zosh

SEE ALSO
  help language
