name     wait
summary  wait for background jobs
usage    wait [JOB]
operand  [JOB]  text  the job to wait for, as %N; all of them if none is given
cwd      none
exit     0  waited
exit     1  no such job, or it is stopped
---
Waits until the job, or every running job, has ended, and reports each end. A stopped job is
not waited for: fg or bg continues it. Ctrl-C stops waiting; the jobs go on.

SEE ALSO
  jobs, kill
