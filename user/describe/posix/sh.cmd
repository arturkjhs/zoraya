name     sh
summary  a POSIX shell: interactive on the terminal, or runs a script (dash)
usage    sh [-c COMMAND] [SCRIPT] [ARG...]
program  /bin/sh
operand  [SCRIPT]  read  the script to run
operand  [ARG...]  text  its arguments
option   -c --command COMMAND text  run COMMAND instead of a script
exit     0  done
exit     1  failed
---
sh is a link to dash, program /bin/sh: dash from the ports (ADR-0109), as a POSIX program with the toolchain set. It
has the terminal on the streams the line does not redirect or pipe (ADR-0331), so typed alone it is an interactive shell with job
control: a prompt, Ctrl-C and Ctrl-Z, fg and bg. A script run by sh has PATH=/bin:/usr/bin and gets
toybox's commands (ADR-0336 item 7).
Write the short options: the long names belong to the description.

EXAMPLES
  sh build.sh
  sh -c 'echo hi'

SEE ALSO
  dash, env

Lets through: -c
