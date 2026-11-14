name     dd
summary  copy and convert a stream
usage    dd [OPERAND...]
program  /bin/dd
operand  [OPERAND...]  text  the words if=FILE of=FILE bs=N count=N skip=N seek=N
exit     0  done
exit     1  failed
---
Copy and convert a stream: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
dd's if= and of= are not designated (they are not options): the line needs +r on what it reads and +w on what it
writes, or dd copies standard input to standard output. Words such as if= are text to the shell.

EXAMPLES
  dd if=disk.img of=copy.img bs=1M    (needs +r disk.img, +w copy.img)

SEE ALSO
  cp, od

Lets through: (no options)
