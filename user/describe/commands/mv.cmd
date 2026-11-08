name     mv
summary  move or rename
usage    mv FROM TO
operand  FROM  rename  the file or directory to move
operand  TO    rename  its new name
exit     0  moved
exit     1  not moved
---
Gives FROM the name TO, in the same directory or another. TO must not exist.

EXAMPLES
  mv draft.txt final.txt
  mv notes.txt project/notes.txt

SEE ALSO
  cp, rm
