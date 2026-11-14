name     date
summary  show or format the date
usage    date [-u] [-d STRING] [-r FILE] [FORMAT]
program  /bin/date
operand  [FORMAT]  text  a format written with a leading +: +%Y-%m-%d
option   -u --utc  use UTC
option   -d --date STRING text  show STRING instead of now
option   -r --reference FILE read  show the modification time of FILE
exit     0  done
exit     1  failed
---
date is a native command in zosh; /bin/date is toybox's, for scripts (type /bin/date to run it).
Show or format the date: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Setting the clock (-s) is not listed.
Write the short options: the long names belong to the description.

EXAMPLES
  date
  date -u +%Y-%m-%dT%H:%M:%SZ

SEE ALSO
  touch, sleep

Lets through: -u -d -r
