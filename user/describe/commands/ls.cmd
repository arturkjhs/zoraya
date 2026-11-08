name     ls
summary  list a directory
usage    ls [PATH]
operand  [PATH]  read  the directory to list, the current one if none is given, or a file to name
exit     0  listed
exit     1  not listed
---
Prints the names in PATH, one per line, sorted byte by byte. A directory's name ends in /.
An empty directory prints nothing. A file is printed by the name it was given as.

EXAMPLES
  ls
  ls project

SEE ALSO
  find, stat
