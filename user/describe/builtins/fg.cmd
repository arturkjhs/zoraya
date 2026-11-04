name     fg
summary  continue a job in the foreground
usage    fg [JOB]
operand  [JOB]  text  the job, as %N; the newest if none is given
cwd      none
exit     0  the job ended well
exit     1  no such job, or it failed
---
Prints the job's line, continues it if Ctrl-Z stopped it, and waits for it as if it had been
started in front: the keys reach it, and Ctrl-C and Ctrl-Z act on it. A job that was in the
background has its input at its end.

SEE ALSO
  bg, jobs
