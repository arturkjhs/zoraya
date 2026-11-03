name     kill
summary  end a job
usage    kill [--interrupt|--terminate|--hangup|--quit|--kill] [JOB]
operand  [JOB]  text  the job, as %N; the newest if none is given
option   --interrupt  ask it to stop, as Ctrl-C does
option   --terminate  ask it to end (the default)
option   --hangup     tell it its terminal is gone
option   --quit       ask it to quit, as Ctrl-\ does
option   --kill       end it at once, without asking
cwd      none
exit     0  the request was sent
exit     1  no such job
---
Sends the job a request, and waits up to two seconds to report how it ended.

EXAMPLES
  kill %1
  kill --kill %2

SEE ALSO
  jobs, wait
