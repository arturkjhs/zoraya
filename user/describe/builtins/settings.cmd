name     settings
summary  show or change settings
usage    settings [--all]
usage    settings NAME [VALUE...]
usage    settings NAME +ITEM [--at N]
usage    settings NAME -ITEM
usage    settings --reset NAME
usage    settings --check
operand  [NAME]   text  a setting, GROUP.NAME
operand  [VALUE...]  text  its new value; for a list, its items, or +ITEM to add one, -ITEM to remove one
option   -a --all          list every setting, not only the ones you changed
option   --reset NAME text remove NAME from the file: its default applies
option   --check           report every problem in /config/settings, and change nothing
option   --at N text       with +ITEM: put it before item N of the list, from 0
cwd      none
exit     0  shown or changed
exit     1  no such setting, a value of the wrong type, or problems found
---
Settings live in /config/settings as NAME = VALUE lines, and every one has a default.
A change takes effect in this shell at once.

EXAMPLES
  settings
  settings shell.prompt 'zoraya {cwd}$ '
  settings --reset shell.prompt
  settings gui.keyboard-layouts us ua cz
  settings gui.keyboard-layouts +ru --at 1
  settings gui.keyboard-layouts -cz

FILES
  /config/settings

SEE ALSO
  help settings
