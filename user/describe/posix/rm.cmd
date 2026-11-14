name     rm
summary  remove files
usage    rm [-fRrv] PATH...
program  /bin/rm
operand  PATH...  remove  the files or empty directories to remove
option   -f --force  ignore a file that does not exist
option   -r --recursive  remove directories and everything in them
option   -R --recurse  the same as -r
option   -v --verbose  say what is removed
when     -r PATH remove-tree
when     -R PATH remove-tree
exit     0  done
exit     1  failed
---
rm is a native command in zosh; /bin/rm is toybox's, for scripts (type /bin/rm to run it).
Remove files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
Without -r a directory is removed only if it is empty.
Write the short options: the long names belong to the description.

EXAMPLES
  rm old.txt
  rm -rf build

SEE ALSO
  rmdir, mv

Lets through: -f -r -R -v
