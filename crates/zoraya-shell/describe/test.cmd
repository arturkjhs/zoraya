name     test
summary  check a condition
usage    test --directory PATH
usage    test --file PATH
usage    test --exists PATH
usage    test A = B
usage    test A != B
operand  [ARG...]  text  what to check
option   -d --directory  PATH is a directory
option   -f --file       PATH is a file
option   -e --exists     PATH is a file or a directory
cwd      read
question
exit     0  yes
exit     1  no
exit     2  wrong usage
---
Answers a question: yes is status 0, no is status 1, and a no stops nothing.

EXAMPLES
  if test -d build { echo yes } else { echo no }
  test $name = notes.txt && echo same

SEE ALSO
  if (help language)
