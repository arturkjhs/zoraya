topic    jobs
summary  running programs in the background, and stopping them
---
Each line you run is a job. A line that ends in & runs in the background: the prompt comes
back at once, and the shell says when the job ends.

  Ctrl-C       ask the job in front to stop; after it says so, Ctrl-C again kills it
  Ctrl-\       ask it to quit
  jobs         list the jobs
  wait [%N]    wait for one job, or all
  kill [%N]    end a job: --interrupt --terminate --hangup --quit --kill
  fg [%N]      bring a background job to the front

Ending the shell with exit ends its jobs; the first exit only warns.

SEE ALSO
  help keys
