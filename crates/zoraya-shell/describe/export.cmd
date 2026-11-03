name     export
summary  give a variable to the programs you run
usage    export NAME [VALUE]
operand  NAME     text  the variable
operand  [VALUE]  text  its value; without one, the variable keeps the one it has
cwd      none
exit     0  exported
exit     1  not exported: the variable does not hold exactly one value
---
Programs receive only the variables you export, each as NAME=value, and the NAME=value words you
write before a command. Nothing else is inherited.

EXAMPLES
  export editor tcc
  CC=tcc make

SEE ALSO
  set
