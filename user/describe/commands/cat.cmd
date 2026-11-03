name     cat
summary  print files
usage    cat FILE...
operand  FILE...  read  the files to print, in order
exit     0  printed
exit     1  a file could not be printed
---
Prints each FILE in turn. A file that is not UTF-8 text is described by its size instead.

EXAMPLES
  cat notes.txt
  cat a.txt b.txt

SEE ALSO
  head, ls
