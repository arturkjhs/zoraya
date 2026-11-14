name     cut
summary  cut out columns
usage    cut [-s] [-d DELIM] (-b LIST | -c LIST | -f LIST) [FILE...]
program  /bin/cut
operand  [FILE...]  read  the files to cut, standard input if none
option   -b --bytes LIST text  keep these bytes
option   -c --characters LIST text  keep these characters
option   -f --fields LIST text  keep these fields
option   -d --delimiter DELIM text  the field delimiter, tab if not given
option   -s --only-delimited  drop lines without a delimiter
exit     0  done
exit     1  failed
---
Cut out columns: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  cut -d: -f1 /etc/passwd

SEE ALSO
  sort, tr

Lets through: -b -c -f -d -s
