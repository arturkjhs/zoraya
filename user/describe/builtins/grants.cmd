name     grants
summary  show what a command would be given, without running it
usage    grants LINE...
operand  LINE...  text  a command line, with its grants
cwd      none
exit     0  shown
---
Prints what the command on the line would receive, and runs nothing.

EXAMPLES
  grants cp notes.txt project/notes.txt

SEE ALSO
  help grants
