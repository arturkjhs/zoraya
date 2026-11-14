name     cc
summary  compile C programs (a link to tcc)
usage    cc [-c] [-o FILE] [-I DIR] FILE...
program  /bin/cc
operand  FILE...  read  the C sources and objects
option   -c --compile-only  compile to an object file, without linking
option   -o --output FILE create  the program or object file to write
option   -I --include DIR read  a directory of headers to read as well
exit     0  done
exit     1  failed
---
cc is a link to tcc, program /bin/cc: TinyCC from the ports (ADR-0109), as a POSIX program with the toolchain set. It gets
the current directory to read and what its operands designate: each FILE to read, -o's file to create or replace, -I's directory.
Write the short options: the long names belong to the description.

EXAMPLES
  cc -o hello hello.c
  cc -c util.c

SEE ALSO
  tcc, make

Lets through: -c -o -I
