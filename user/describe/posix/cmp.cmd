name     cmp
summary  compare two files
usage    cmp [-ls] [-n N] FILE1 FILE2
program  /bin/cmp
operand  FILE1  read  the first file
operand  FILE2  read  the second file
option   -l --verbose  print every difference
option   -s --silent  print nothing
option   -n --bytes N text  compare at most N bytes
exit     0  the files are the same
exit     1  the files differ
exit     2  an error
---
Compare two files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  cmp a.bin b.bin

SEE ALSO
  diff

Lets through: -l -s -n
