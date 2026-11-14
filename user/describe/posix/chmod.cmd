name     chmod
summary  change file modes
usage    chmod [-Rfv] MODE FILE...
program  /bin/chmod
operand  MODE  text  the new mode: 644, u+x, a=rx
operand  FILE...  write  the files to change
option   -R --recursive  change directories and what is in them
option   -f --silent  do not report errors
option   -v --verbose  say what is changed
when     -R FILE tree
exit     0  done
exit     1  failed
---
Change file modes: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
A MODE that begins with - (as -x) cannot be given: write u-x or a-x.
Write the short options: the long names belong to the description.

EXAMPLES
  chmod 755 run.sh
  chmod u+x run.sh

SEE ALSO
  ls, stat

Lets through: -R -f -v
