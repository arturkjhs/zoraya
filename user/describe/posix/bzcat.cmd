name     bzcat
summary  print bzip2 files
usage    bzcat [FILE...]
program  /bin/bzcat
operand  [FILE...]  read  the .bz2 files to decompress, standard input if none
exit     0  done
exit     1  failed
---
Print bzip2 files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  bzcat src.tar.bz2 | tar -x

SEE ALSO
  zcat, tar

Lets through: (no options)
