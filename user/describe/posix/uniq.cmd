name     uniq
summary  drop repeated lines
usage    uniq [-cdui] [-f N] [-s N] [-w N] [FILE]
program  /bin/uniq
operand  [FILE]  read  the file to read, standard input if none
option   -c --count  put the count before each line
option   -d --repeated  print only repeated lines
option   -u --unique  print only lines that are not repeated
option   -i --ignore-case  compare without case
option   -f --skip-fields N text  skip N fields
option   -s --skip-chars N text  skip N characters
option   -w --check-chars N text  compare at most N characters
exit     0  done
exit     1  failed
---
Drop repeated lines: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
The second operand of toybox's uniq, an output file, is not designated, so it is refused: use a redirection.
Write the short options: the long names belong to the description.

EXAMPLES
  sort names.txt | uniq -c

SEE ALSO
  sort, cut

Lets through: -c -d -u -i -f -s -w
