name     bg
summary  continue a stopped job in the background
usage    bg [JOB]
operand  [JOB]  text  the job, as %N; the newest stopped one if none is given
cwd      none
exit     0  continued
exit     1  no stopped job
---
Continues a job that Ctrl-Z stopped, in the background.

SEE ALSO
  fg, jobs
