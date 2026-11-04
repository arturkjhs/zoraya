name     head
summary  print the first lines of files
usage    head [--lines N] FILE...
operand  FILE...  read  the files
option   -n --lines N text  how many lines of each, 10 if not given
exit     0  printed
exit     1  a file could not be read
---
Prints the first lines of each FILE. With more than one FILE, each is headed by its name.

EXAMPLES
  head notes.txt
  head --lines 1 *.txt

SEE ALSO
  cat, search
