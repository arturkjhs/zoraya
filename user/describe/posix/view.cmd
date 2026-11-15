name     view
summary  look at text files in vi, read only (nvi, a POSIX program)
usage    view [-c COMMAND] [FILE...]
program  /bin/view
cwd      none
operand  [FILE...]  read  the files to look at
option   -c --command COMMAND text  an ex command to run first
exit     0  done
exit     1  failed
---
view is vi opened read only: the same nvi 2.2.2 program under another name (/bin/view is a link to /bin/vi). Its files are
given to read only, so :w and :w! are refused whatever the editor's own option says (a narrower grant than vi's, proposed ADR-0391).
See help vi for the rest: the terminal, cwd none, no recovery, no start-up file.
Put the options first, and write a command as -c COMMAND (no +10 or +/text): a word that starts with + is refused.
Two files of one directory are refused until the grant can name both: give the directory (+r . view a b) or name one.
Write the short options: the long names belong to the description.

EXAMPLES
  view /config/settings

SEE ALSO
  vi, less

Lets through: -c
