name     dash
summary  a POSIX shell: interactive on the terminal, or runs a script (dash)
usage    dash [SCRIPT] [ARG...]
program  /bin/dash
operand  [SCRIPT]  read  the script to run
operand  [ARG...]  text  its arguments
---
dash from the ports (ADR-0109), for scripts written for /bin/sh, as a POSIX program with the toolchain set.
It has the terminal on the streams the line does not redirect or pipe (ADR-0331), so typed alone it is an
interactive shell with job control: a prompt, Ctrl-C and Ctrl-Z, `fg` and `bg`. With a script it runs the script.
