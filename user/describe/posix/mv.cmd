name     mv
summary  move or rename files
usage    mv [-fnvi] FROM TO
program  /bin/mv
operand  FROM  rename  the file or directory to move
operand  TO  create  the new name
option   -f --force  replace TO if it exists
option   -n --no-clobber  do not replace an existing file
option   -i --interactive  ask before replacing
option   -v --verbose  say what is moved
exit     0  done
exit     1  failed
---
mv is a native command in zosh; /bin/mv is toybox's, for scripts (type /bin/mv to run it).
Move or rename files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Write the short options: the long names belong to the description.

EXAMPLES
  mv old.txt new.txt

SEE ALSO
  cp, rm

Lets through: -f -n -i -v
