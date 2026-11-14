name     touch
summary  make a file or set its time
usage    touch [-acm] [-r REFFILE] FILE...
program  /bin/touch
operand  FILE...  create  the files to make or to touch
option   -a --access  change the access time only
option   -c --no-create  do not make a file that does not exist
option   -m --modification  change the modification time only
option   -r --reference REFFILE read  take the time of REFFILE
exit     0  done
exit     1  failed
---
Make a file or set its time: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  touch new.txt
  touch -r a.txt b.txt

SEE ALSO
  ls, stat

Lets through: -a -c -m -r
