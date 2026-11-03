name     set
summary  set a variable
usage    set [--global] NAME [VALUE...]
usage    set --query NAME
usage    set
operand  [NAME]      text  the variable's name: letters, digits and _
operand  [VALUE...]  text  its values; none makes it the empty list
option   -g --global  set it outside the current function
option   -q --query   succeed if NAME is set and not empty, and change nothing
cwd      none
exit     0  set; with --query, NAME is set and not empty
exit     1  not set: a name that is not valid, or with --query, NAME is not set or empty
---
A variable holds a list of values. $NAME gives one argument per value, never split at spaces, and
never matched as a pattern. Inside a function a variable is the function's own unless --global is
given. A pattern among the values that matches nothing gives no value. Without a name, set lists
every variable.

EXAMPLES
  set name 'my notes.txt'
  set files *.txt
  set --query editor || set editor tcc

SEE ALSO
  unset, export, count
