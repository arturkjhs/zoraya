name     status
summary  describe how the last command ended
usage    status [--code|--reason|--detail]
option   --code    print only the exit code
option   --reason  print only how it ended: exited, terminated, killed, fault or limit
option   --detail  print only the detail number of its record
cwd      none
exit     0  described
---
Describes the last command's exit record in words. $status holds the same as one word.

EXAMPLES
  status
  status --code

SEE ALSO
  help errors
