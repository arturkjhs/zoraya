name     sleep
summary  wait
usage    sleep SECONDS
program  /bin/sleep
operand  SECONDS  text  how long to wait; a fraction is allowed
exit     0  done
exit     1  failed
---
sleep is a native command in zosh; /bin/sleep is toybox's, for scripts (type /bin/sleep to run it).
Wait: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  sleep 2
  sleep 0.5

SEE ALSO
  date

Lets through: (no options)
