name     pdpmake
summary  build what a makefile describes (pdpmake, a POSIX program)
usage    pdpmake [-C DIR] [-f FILE] [-j N] [TARGET...]
program  /bin/pdpmake
operand  [TARGET...]  text  what to build
option   -C --directory DIR tree  the directory to build in, which it may change throughout
option   -f --file FILE read  the makefile, if not ./Makefile
option   -j --jobs N text  how many recipes at once
exit     0  done
exit     1  failed
---
pdpmake, the POSIX make, from the ports (ADR-0109), as a POSIX program with the toolchain set. With -C DIR it may change anything in DIR;
without it, only read the current directory: give +w . to build there.
Write the short options: the long names belong to the description.

EXAMPLES
  pdpmake
  pdpmake -C src all

SEE ALSO
  make, cc

Lets through: -C -f -j
