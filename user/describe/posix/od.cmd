name     od
summary  dump files in octal and other forms
usage    od [-cv] [-A RADIX] [-t TYPE] [-j N] [-N N] [FILE...]
program  /bin/od
operand  [FILE...]  read  the files to dump, standard input if none
option   -A --address-radix RADIX text  address base: d, o, x or n
option   -t --format TYPE text  output type: x1, c, d2
option   -j --skip-bytes N text  skip N bytes
option   -N --read-bytes N text  read only N bytes
option   -c --chars  characters, the same as -t c
option   -v --output-duplicates  do not shorten repeated lines
exit     0  done
exit     1  failed
---
Dump files in octal and other forms: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  od -A x -t x1 file.bin

SEE ALSO
  xxd, dd

Lets through: -A -t -j -N -c -v
