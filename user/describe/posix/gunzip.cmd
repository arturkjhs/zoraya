name     gunzip
summary  decompress files
usage    gunzip [-cfkt] FILE...
program  /bin/gunzip
operand  FILE...  write  the files to decompress, replaced by the result
option   -c --stdout  write to standard output, keep FILE
option   -f --force  replace an existing result
option   -k --keep  keep FILE
option   -t --test  test the compressed file
when     -c FILE read
when     -t FILE read
exit     0  done
exit     1  failed
---
Decompress files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
FILE is changed in place, but the result is a sibling (FILE.gz or without .gz): that name is not designated, so the line
needs +w on the directory too, except with -c or -t, which only read. Use -c and a redirection for the least rights.
Write the short options: the long names belong to the description.

EXAMPLES
  gunzip -c big.log.gz > big.log

SEE ALSO
  zcat, tar

Lets through: -c -f -k -t
