name     tail
summary  print the last lines
usage    tail [-f] [-n N] [-c N] [FILE...]
program  /bin/tail
operand  [FILE...]  read  the files to print, standard input if none
option   -n --lines N text  print N lines, 10 if not given (+N: from line N)
option   -c --bytes N text  print N bytes
option   -f --follow  keep reading what is added
exit     0  done
exit     1  failed
---
Print the last lines: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
The old form tail -5 is not accepted: write tail -n 5.
Write the short options: the long names belong to the description.

EXAMPLES
  tail -n 20 build.log

SEE ALSO
  head, cat

Lets through: -n -c -f
