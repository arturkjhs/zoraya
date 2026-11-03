name     sleep
summary  wait for some seconds
usage    sleep SECONDS
operand  SECONDS  text  how long to wait, a whole number
cwd      none
exit     0  waited
---
Waits SECONDS seconds, then ends. Ctrl-C ends it sooner.

EXAMPLES
  sleep 5
  sleep 30 &

SEE ALSO
  jobs, wait
