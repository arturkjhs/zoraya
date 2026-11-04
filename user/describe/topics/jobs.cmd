topic    jobs
summary  running programs in the background, and stopping them
---
Each line you run is a job. A line that ends in & runs in the background: the prompt comes
back at once, and the shell says when the job ends.

  Ctrl-C       ask the job in front to stop; after it says so, Ctrl-C again kills it
  Ctrl-\       ask it to quit
  Ctrl-Z       stop the job in front; the prompt comes back
  jobs         list the jobs, running or stopped, and what each was given
  wait [%N]    wait for one job, or all that run
  kill [%N]    end a job: --interrupt --terminate --hangup --quit --kill
  fg [%N]      bring a job to the front, and continue it if it was stopped
  bg [%N]      continue a stopped job in the background

A job in the background reads no keys: its input is at its end.

Ending the shell with exit ends its jobs; the first exit only warns.

SEE ALSO
  help keys
