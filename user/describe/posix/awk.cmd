name     awk
summary  process text by patterns (one-true-awk)
usage    awk [-F FS] [-v VAR=VALUE] [-f PROGFILE] [PROGRAM] [FILE...]
program  /bin/awk
operand  [PROGRAM]  text  the awk program: '{ print $1 }'; with -f it is the first FILE instead
operand  [FILE...]  read  the files to process, standard input if none
option   -F --field-separator FS text  the field separator
option   -v --assign VAR=VALUE text  set a variable before the program runs
option   -f --file PROGFILE read  read the program from PROGFILE
when     -f PROGRAM read
exit     0  done
exit     1  failed
---
awk is the one-true-awk (second edition, UTF-8) from the ports, as a POSIX program with the toolchain set, with the current
directory read. With -f the first operand is a FILE to read, not a program.
Write the short options: the long names belong to the description.

EXAMPLES
  awk -F: '{ print $1 }' /etc/passwd
  awk '{ s += $1 } END { print s }' nums.txt
  awk -f prog.awk data.txt

SEE ALSO
  sed, cut

Lets through: -F -v -f
