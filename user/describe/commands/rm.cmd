name     rm
summary  remove files
usage    rm [--recursive] PATH...
operand  PATH...  remove  the files or empty directories to remove
option   -r --recursive  also remove directories and everything in them
when     --recursive PATH remove-tree
exit     0  removed
exit     1  not removed
---
Removes each PATH. A directory is removed only when it is empty, or with --recursive.
A PATH that is /, or whose last part is . or .., is refused: name the directory itself.
Nothing asks first: what is removed is gone.

EXAMPLES
  rm notes.old
  rm *.bak
  rm --recursive build

SEE ALSO
  mkdir, mv
