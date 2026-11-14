name     tar
summary  archive files
usage    tar (-c | -x | -t) [-jvz] -f ARCHIVE [-C DIR] [PATH...]
program  /bin/tar
operand  [PATH...]  read  the files to archive, or the members to extract or list
option   -c --create  make an archive
option   -x --extract  extract an archive
option   -t --list  list an archive
option   -z --gzip  with -x, -t or -c: gzip format
option   -j --bzip2  with -x or -t: bzip2 format
option   -v --verbose  list the files as they are handled
option   -f --file ARCHIVE create  the archive, made or replaced with -c, read otherwise
option   -C --directory DIR tree  go to DIR first, which tar may change throughout
when     -x PATH text
when     -t PATH text
exit     0  done
exit     1  failed
---
Archive files: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
ARCHIVE is create for all three modes, because an option argument has one kind: with -x or -t it is read in fact, but the
line is granted the name to create or replace (a loss). -C DIR is a tree also with -c. Extracting into the current
directory needs +w on it, as a program that writes there. Standard input and output are used when -f is not given.
Write the short options: the long names belong to the description.

EXAMPLES
  tar -c -z -f src.tar.gz src
  tar -x -f src.tar.gz -C /tmp
  tar -t -f src.tar

SEE ALSO
  gzip, cpio

Lets through: -c -x -t -z -j -v -f -C
