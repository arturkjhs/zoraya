name     fg
summary  continue a job in the foreground
usage    fg [JOB]
operand  [JOB]  text  the job, as %N; the newest if none is given
cwd      none
exit     0  the job ended well
exit     1  no such job, or it failed
---
Prints the job's line and waits for it as if it had been started in front: Ctrl-C reaches it.

SEE ALSO
  bg, jobs
