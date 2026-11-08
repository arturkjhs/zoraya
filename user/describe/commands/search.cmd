name     search
summary  find lines that contain text
usage    search TEXT [PATH...]
operand  TEXT       text  what to look for
operand  [PATH...]  read  files, or directories to search all through
question
exit     0  found
exit     1  not found
exit     2  wrong usage
exit     3  a path could not be read
---
Prints each line that contains TEXT as FILE:LINE: TEXT. Directories are searched with everything
in them. With no PATH it searches its input and prints each line that contains TEXT as it is.
Finding nothing is an answer, no: status 1, and it stops nothing.

EXAMPLES
  search milk notes.txt
  search TODO src
  ls | search .txt

SEE ALSO
  find, head
