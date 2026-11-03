topic    settings
summary  one file of settings, and the command that changes it
---
/config/settings holds lines NAME = VALUE. Every setting has a default, which applies
until a line changes it. The settings command reads and writes the file; you may also
edit it by hand, and settings --check says what is wrong with it.

  settings                     what you have changed
  settings --all               every setting
  settings NAME                one setting: its value, default, type and meaning
  settings NAME VALUE          change it
  settings --reset NAME        back to the default

/config/start.zosh runs before the first prompt: put functions and variables there.

SEE ALSO
  help zosh
