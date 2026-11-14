name     xargs
summary  run a command with arguments from input
usage    xargs [-0rt] [-n N] COMMAND [ARG...]
program  /bin/xargs
operand  COMMAND  text  the program to run
operand  [ARG...]  text  its first arguments; the words read from input follow
option   -0 --null  words end with a NUL byte
option   -r --no-run-if-empty  do not run if there is no input
option   -t --verbose  print each command before it runs
option   -n --max-args N text  at most N words per run
exit     0  done
exit     1  failed
---
Run a command with arguments from input: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
-I is not in toybox 0.8.15. The command it runs holds exactly the entries xargs holds (ADR-0107), no more.
Write the short options: the long names belong to the description.

EXAMPLES
  find . -name '*.c' | xargs grep -n main
  ls | xargs -n 1 echo

SEE ALSO
  find, env

Lets through: -0 -r -t -n
