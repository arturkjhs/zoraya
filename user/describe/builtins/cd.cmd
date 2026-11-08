name     cd
summary  change the current directory
usage    cd [DIR]
operand  [DIR]  read  the directory to go to; home (shell.home) if none is given
exit     0  changed
exit     1  not changed
---
Makes DIR the current directory. .. is the directory above, taken off the path as written.

EXAMPLES
  cd project
  cd ..
  cd

SEE ALSO
  pwd, settings
