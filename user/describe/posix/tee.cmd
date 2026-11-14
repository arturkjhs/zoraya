name     tee
summary  copy input to files
usage    tee [-ai] [FILE...]
program  /bin/tee
operand  [FILE...]  create  the files to write as well as standard output
option   -a --append  add to the files (they are still create or replace; an existing file is kept only with -a)
option   -i --ignore-interrupts  ignore the interrupt signal
exit     0  done
exit     1  failed
---
Copy input to files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  make 2>&1 | tee build.log

SEE ALSO
  cat

Lets through: -a -i
