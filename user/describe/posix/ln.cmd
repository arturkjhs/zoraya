name     ln
summary  make a symbolic link
usage    ln -s [-fv] TARGET LINKNAME
program  /bin/ln
operand  TARGET  text  what the link points to (a path written as is, not opened)
operand  LINKNAME  create  the name of the link
option   -s --symbolic  make a symbolic link (the only kind that works: link() is refused, EPERM)
option   -f --force  replace LINKNAME if it exists
option   -v --verbose  say what is linked
exit     0  done
exit     1  failed
---
Make a symbolic link: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Only the -s form is described; a hard link is refused by the kernel in any case. Without -s the command fails.
Write the short options: the long names belong to the description.

EXAMPLES
  ln -s ../lib/libm.so libm.so

SEE ALSO
  cp, mv

Lets through: -s -f -v
