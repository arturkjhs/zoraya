name     du
summary  show disk use
usage    du [-acksxh] [-d N] [FILE...]
program  /bin/du
operand  [FILE...]  read  the files or directories to measure, the current directory if none
option   -a --all  show files as well
option   -c --total  add a total
option   -h --human-readable  sizes as 1K, 2M
option   -k --kilobytes  sizes in kilobytes
option   -s --summarize  only the total of each operand
option   -x --one-file-system  stay on one file system
option   -d --max-depth N text  show only to N levels
exit     0  done
exit     1  failed
---
Show disk use: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  du -sh .
  du -d 1 src

SEE ALSO
  ls, stat

Lets through: -a -c -h -k -s -x -d
