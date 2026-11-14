name     wc
summary  count lines, words and bytes
usage    wc [-clmwL] [FILE...]
program  /bin/wc
operand  [FILE...]  read  the files to count, standard input if none
option   -c --bytes  count bytes
option   -l --lines  count lines
option   -m --chars  count characters
option   -w --words  count words
option   -L --max-line-length  the longest line
exit     0  done
exit     1  failed
---
Count lines, words and bytes: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  wc -l notes.txt

SEE ALSO
  cat, sort

Lets through: -c -l -m -w -L
