name     cp
summary  copy a file
usage    cp [--force] FROM TO
operand  FROM  read    the file to copy
operand  TO    create  the name of the copy
option   -f --force    replace TO if it exists
exit     0  copied
exit     1  not copied
---
Copies the file FROM to a new file TO. TO must not exist unless --force is given.

EXAMPLES
  cp notes.txt notes.bak
  cp --force draft.txt final.txt

SEE ALSO
  mv, rm
