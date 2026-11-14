name     xxd
summary  hex dump
usage    xxd [-pri] [-c N] [-g N] [-l N] [-s N] [FILE]
program  /bin/xxd
operand  [FILE]  read  the file to dump, standard input if none
option   -p --plain  plain hex without addresses
option   -r --reverse  turn a hex dump back into bytes
option   -i --include  output as a C array
option   -c --cols N text  bytes per line
option   -g --groupsize N text  bytes per group
option   -l --len N text  stop after N bytes
option   -s --seek N text  start at offset N
exit     0  done
exit     1  failed
---
Hex dump: toybox 0.8.15 (0BSD) as a POSIX program with the toolchain set (ADR-0336).
toybox's xxd also takes an output file as second operand: it is not designated here, use a redirection.
Write the short options: the long names belong to the description.

EXAMPLES
  xxd file.bin | head

SEE ALSO
  od

Lets through: -p -r -i -c -g -l -s
