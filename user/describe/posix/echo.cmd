name     echo
summary  print words
usage    echo [-neE] [STRING...]
program  /bin/echo
operand  [STRING...]  text  the words to print
option   -n --no-newline  no newline at the end
option   -e --escapes  interpret backslash escapes
option   -E --no-escapes  do not (the default)
exit     0  done
exit     1  failed
---
echo is a native command in zosh; /bin/echo is toybox's, for scripts (type /bin/echo to run it).
Print words: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  echo hello
  echo -n a

SEE ALSO
  printf

Lets through: -n -e -E
