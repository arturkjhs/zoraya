name     file
summary  tell what a file is
usage    file [-bhLs] FILE...
program  /bin/file
operand  FILE...  read  the files to look at
option   -b --brief  do not print the file names
option   -h --no-dereference  do not follow symbolic links
option   -L --dereference  follow symbolic links
option   -s --special-files  read special files too
exit     0  done
exit     1  failed
---
Tell what a file is: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  file a.out

SEE ALSO
  stat, od

Lets through: -b -h -L -s
