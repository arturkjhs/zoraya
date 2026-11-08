name     echo-args
summary  print each argument it was given on a line
usage    echo-args [--streams] [WORD...]
operand  [WORD...]  text  the words to print
option   --streams  first, what is at each stream slot: in, out and err
cwd      none
exit     0  printed
---
Prints each argument it received, numbered from 0: what the shell passed to it. With --streams it
first prints the type of the handle at each of its three stream slots (ADR-0152 rule 1).
