name     ls
summary  list a directory
usage    ls [DIR]
operand  [DIR]  read  the directory to list; the current one if none is given
exit     0  listed
exit     1  not listed
---
Prints the names in DIR, one per line, sorted byte by byte. A directory's name ends in /.
An empty directory prints nothing.

EXAMPLES
  ls
  ls project

SEE ALSO
  find, stat
