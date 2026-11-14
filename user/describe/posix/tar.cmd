name     tar
summary  archive files
usage    tar (-c | -x | -t) [-jvz] [-f ARCHIVE] [-C DIR] [PATH...]
program  /bin/tar
operand  [PATH...]  read  the files to archive, or the members to extract or list
option   -c --create  make an archive
option   -x --extract  extract an archive
option   -t --list  list an archive
option   -z --gzip  with -x, -t or -c: gzip format
option   -j --bzip2  with -x or -t: bzip2 format
option   -v --verbose  list the files as they are handled
option   -f --file ARCHIVE read  the archive to list or extract (with -c, write to standard output instead)
option   -C --directory DIR read  go to DIR first, to archive from it (to extract into it, +w DIR)
when     -x PATH text
when     -t PATH text
exit     0  done
exit     1  failed
---
Archive files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
An option argument has one kind whatever the mode, so -f ARCHIVE and -C DIR are read (ADR-0336 item 5 asks no more
for -c or -t): to make an archive, leave out -f and redirect standard output, which the line designates. Extracting
writes files, so -x needs +w on the directory it extracts into (the current one, or DIR). Standard input and output
are used when -f is not given.
Write the short options: the long names belong to the description.

EXAMPLES
  tar -c -z src > src.tar.gz
  tar -x -f src.tar.gz -C /tmp
  tar -t -z -f src.tar.gz

SEE ALSO
  gzip, cpio

Lets through: -c -x -t -z -j -v -f -C
