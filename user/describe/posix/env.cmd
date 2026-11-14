name     env
summary  run a command with a changed environment
usage    env [-i] [WORD...]
program  /bin/env
operand  [WORD...]  text  NAME=VALUE assignments, then COMMAND and its arguments; with none, the environment is printed
option   -i --ignore-environment  start with an empty environment
exit     0  done
exit     1  failed
exit     125  env itself failed
exit     126  COMMAND found but not run
exit     127  COMMAND not found
---
Run a command with a changed environment: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
One text operand holds NAME=VALUE... COMMAND [ARG...] (a shell operand has one variable-length list at most). -u, -0 and -e are not
listed. The command it runs holds exactly the entries env holds (ADR-0107).
Write the short options: the long names belong to the description.

EXAMPLES
  env
  env -i PATH=/bin sh -c 'echo $PATH'

SEE ALSO
  sh, xargs

Lets through: -i
