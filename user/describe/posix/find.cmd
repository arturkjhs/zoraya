name     find
summary  find files
usage    find [-L] PATH [EXPRESSION...]
program  /bin/find
operand  PATH  read  the directory to search (a tree)
operand  [EXPRESSION...]  text  the tests and actions: -name '*.c' -type f -print
option   -L --follow  follow symbolic links
exit     0  searched
exit     1  an error
---
find is a native command in zosh; /bin/find is toybox's, for scripts (type /bin/find to run it).
Find files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
One PATH only. The shell lets through only the options listed here (ADR-0336 item 5), and it takes a word starting
with - for an option wherever it stands, so an expression of tests (-name, -type, -exec, -delete) is refused before
find runs: run /usr/libexec/toybox find for one, which has no description and gets the current directory and the
line's grants only. PATH is read: with -delete or -exec that changes
files the paths need +w. A command run by -exec holds exactly the entries find holds (ADR-0107, ADR-0336 item 5):
find . -name '*.o' -exec rm {} + needs +w .
Write the short options: the long names belong to the description.

EXAMPLES
  find . -name '*.c'
  find src -type f -print
  find . -name '*.o' -delete    (needs +w .)

SEE ALSO
  ls, grep, xargs

Lets through: -L
