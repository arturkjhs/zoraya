name     requests
summary  say each Ctrl-C and Ctrl-\ it takes, for the job-control tests
usage    requests [--read] NAME
operand  NAME  text  the name its lines begin with
option   --read  instead, read the input to its end and say how much came
cwd      none
exit     0  its input ended
---
Without --read it takes interrupt and quit requests itself and says each on its error output,
numbered, so that a test sees which jobs a key reached (ADR-0158, INV-0158-1); only terminate
and kill end it. With --read it reads its input until the end and says how many bytes came
(INV-0158-2).
