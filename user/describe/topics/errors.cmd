topic    errors
summary  what the shell and commands say when something goes wrong
---
Every command says what went wrong in one line, NAME: SUBJECT: PROBLEM, and may add a
hint saying what to try. A command that succeeds says nothing but its result.

  0    success
  1    failure; for a question such as test or search, "no"
  2    the command was used wrongly: it shows its usage
  3    a question could not be answered

When a program ends in another way the shell says how:
  spin: interrupted (Ctrl-C)        fault: crashed (data abort, class 0x24)
  spin: killed                      make: ended by signal 6

A failure stops the rest of the line, and the shell says (the rest of the line was not
run). status describes how the last command ended.

SEE ALSO
  help language
