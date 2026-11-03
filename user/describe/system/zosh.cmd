name     zosh
summary  the shell
usage    zosh
cwd      none
setting  shell.prompt        text   '{cwd}> '  the prompt; {cwd} is the current directory with ~ for home
setting  shell.home          path   /home  where an interactive shell starts, and what ~ and cd alone mean
setting  shell.path          text   '/zoraya/bin /bin'  where commands are looked for, in order
setting  shell.history       yes-no yes    keep history in /config/history
setting  shell.history-size  number 0 100000 1000  lines of history kept
setting  shell.suggestions   yes-no yes    suggestions from history
setting  shell.colour        yes-no yes    any colour from the shell
setting  console.columns     number 20 500 80  width of the serial console
---
The shell people type commands into. help language describes what it reads.

FILES
  /config/settings   settings
  /config/start.zosh run before the first prompt
  /config/history    the lines run
