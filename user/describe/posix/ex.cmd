name     ex
summary  edit text files a line at a time (nvi, a POSIX program)
usage    ex [-Rs] [-c COMMAND] [FILE...]
program  /bin/ex
cwd      none
operand  [FILE...]  create  the files to edit; each may be created or replaced
option   -R --readonly  open the files to read only
when     -R FILE read
option   -s --silent  no prompt or messages: for scripts
option   -c --command COMMAND text  an ex command to run first
exit     0  done
exit     1  failed
---
ex is vi's line mode: the same nvi 2.2.2 program, which takes its mode from the name it is run by (/bin/ex is a link to /bin/vi).
It works on the serial console. The grants are vi's: the files the line names and nothing else of their directory
(ADR-0335 item 3); saving is in place and not atomic; there is no recovery and no start-up file (see help vi).
Write the short options: the long names belong to the description.

EXAMPLES
  ex notes.txt
  ex -s notes.txt   (commands on standard input)

SEE ALSO
  vi, view, sed

Lets through: -R -s -c
