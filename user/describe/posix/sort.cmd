name     sort
summary  sort lines
usage    sort [-bcdfnrsu] [-k KEYDEF] [-t SEP] [-o FILE] [FILE...]
program  /bin/sort
operand  [FILE...]  read  the files to sort, standard input if none
option   -b --ignore-leading-blanks  ignore leading blanks
option   -c --check  only check that the input is sorted
option   -d --dictionary-order  consider only blanks and alphanumerics
option   -f --ignore-case  fold lower case to upper
option   -n --numeric-sort  compare as numbers
option   -r --reverse  reverse the order
option   -s --stable  keep equal lines in input order
option   -u --unique  drop repeated lines
option   -k --key KEYDEF text  sort on a key: 2,2 or 3n
option   -t --field-separator SEP text  the field separator
option   -o --output FILE create  write the result to FILE
exit     0  done
exit     1  failed
---
Sort lines: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  sort names.txt
  sort -t: -k3n /etc/passwd

SEE ALSO
  uniq, cut

Lets through: -b -c -d -f -n -r -s -u -k -t -o
