name     wait
summary  wait for background jobs
usage    wait [JOB]
operand  [JOB]  text  the job to wait for, as %N; all of them if none is given
cwd      none
exit     0  waited
exit     1  no such job
---
Waits until the job, or every background job, has ended, and reports each end. Ctrl-C stops waiting; the jobs go on.

SEE ALSO
  jobs, kill
