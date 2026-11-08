name     dash
summary  run a POSIX shell script (dash, a POSIX program)
usage    dash [SCRIPT] [ARG...]
program  /bin/dash
operand  [SCRIPT]  read  the script to run
operand  [ARG...]  text  its arguments
---
dash from the ports (ADR-0109), for scripts written for /bin/sh, as a POSIX program with the toolchain set.
It has no terminal from zosh (ADR-0153), so it runs scripts, not an interactive session.
