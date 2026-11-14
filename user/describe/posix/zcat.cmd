name     zcat
summary  print compressed files
usage    zcat [FILE...]
program  /bin/zcat
operand  [FILE...]  read  the .gz files to decompress, standard input if none
exit     0  done
exit     1  failed
---
Print compressed files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).

EXAMPLES
  zcat big.log.gz | head

SEE ALSO
  gzip, bzcat

Lets through: (no options)
