name     make
summary  build what a makefile describes (GNU make, a POSIX program)
usage    make [-C DIR] [-f FILE] [-j N] [TARGET...]
program  /bin/make
option   -C --directory DIR tree  the directory to build in, which it may change throughout
option   -f --file FILE read  the makefile, if not ./Makefile
option   -j --jobs N text  how many recipes at once
operand  [TARGET...]  text  what to build
---
GNU make from the ports (ADR-0109), as a POSIX program with the toolchain set. With -C DIR it may change
anything in DIR; without it, only read the current directory: give +w . to build there.
