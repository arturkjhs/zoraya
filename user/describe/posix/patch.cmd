name     patch
summary  apply a patch
usage    patch [-R] [-p N] [-i PATCHFILE] [FILE]
program  /bin/patch
operand  [FILE]  write  the file to patch, if the patch does not name it
option   -R --reverse  undo the patch
option   -p --strip N text  remove N leading parts from the file names in the patch
option   -i --input PATCHFILE read  read the patch from PATCHFILE, not standard input
exit     0  done
exit     1  failed
---
Apply a patch: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
patch changes the files its input names, which no operand designates: it needs +w on the tree, as make does.
Write the short options: the long names belong to the description.

EXAMPLES
  patch -p1 -i fix.diff    (with +w .)

SEE ALSO
  diff, sed

Lets through: -R -p -i
