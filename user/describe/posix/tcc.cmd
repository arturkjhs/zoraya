name     tcc
summary  compile C programs (TinyCC, a POSIX program)
usage    tcc [-c] [-o FILE] [-I DIR] FILE...
program  /bin/tcc
option   -c --compile-only  compile to an object file, without linking
option   -o --output FILE create  the program or object file to write
option   -I --include DIR read  a directory of headers to read as well
operand  FILE...  read  the C sources and objects
---
TinyCC from the ports (ADR-0109). It runs as a POSIX program: it gets the toolchain set (/bin, /usr and /etc
to read, /dev/null and a /tmp of its own), the current directory to read, and what its operands designate:
each FILE to read, -o's file to create or replace, -I's directory to read. Only the option forms above are
understood for designation; tcc itself takes its own forms (-o FILE, -I DIR).
