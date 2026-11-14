name     head
summary  print the first lines
usage    head [-qv] [-n N] [-c N] [FILE...]
program  /bin/head
operand  [FILE...]  read  the files to print, standard input if none
option   -n --lines N text  print N lines, 10 if not given
option   -c --bytes N text  print N bytes
option   -q --quiet  no file name headers
option   -v --verbose  always the file name headers
exit     0  done
exit     1  failed
---
head is a native command in zosh; /bin/head is toybox's, for scripts (type /bin/head to run it).
Print the first lines: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
The old form head -5 is not accepted: write head -n 5.
Write the short options: the long names belong to the description.

EXAMPLES
  head -n 5 notes.txt

SEE ALSO
  tail, cat

Lets through: -n -c -q -v
