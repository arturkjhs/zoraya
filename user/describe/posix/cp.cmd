name     cp
summary  copy files
usage    cp [-Rafnipr] [-v] FROM TO
program  /bin/cp
operand  FROM  read  the file or, with -r, the tree to copy
operand  TO  create  the name of the copy
option   -r --recursive  copy directories and what is in them
option   -R --recurse  the same as -r
option   -a --archive  copy directories and keep attributes
option   -f --force  replace TO if it exists
option   -n --no-clobber  do not replace an existing file
option   -p --preserve  keep mode and times
option   -i --interactive  ask before replacing
option   -v --verbose  say what is copied
when     -r FROM tree
when     -R FROM tree
when     -a FROM tree
exit     0  done
exit     1  failed
---
cp is a native command in zosh; /bin/cp is toybox's, for scripts (type /bin/cp to run it).
Copy files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
With -r, -R or -a FROM is a tree to read. One FROM and one TO: copying several files into a directory, or into an
existing directory by its name, is not described (TO is one name to create or replace).
Write the short options: the long names belong to the description.

EXAMPLES
  cp a.txt b.txt
  cp -r src backup

SEE ALSO
  mv, rm, ln

Lets through: -r -R -a -f -n -p -i -v
