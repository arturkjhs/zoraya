name     random
summary  show random bytes
usage    random [COUNT]
operand  [COUNT]  text  how many bytes, 1 to 4096; 16 if left out
cwd      none
exit     0  shown
exit     1  the system has no random service
exit     2  COUNT is not a number of bytes
---
Prints COUNT random bytes in hexadecimal, from the random service every program is given a connection to. Two runs print different bytes.

EXAMPLES
  random
  random 32

SEE ALSO
  date
