name     zosh
summary  the shell
usage    zosh
option   --room NAME text  start in the room NAME, as the desktop's dock launches it
option   --net WORDS text  the room's +net words, space apart, as the dock gives them (the network the shell holds is those)
cwd      none
setting  shell.prompt        text   '{cwd}> '  the prompt; {cwd} is the current directory with ~ for home
setting  shell.home          path   /home  where an interactive shell starts, and what ~ and cd alone mean
setting  shell.path          text   '/zoraya/bin /bin'  where commands are looked for, in order
setting  shell.history       yes-no yes    keep history in /config/history
setting  shell.history-size  number 0 100000 1000  lines of history kept
setting  shell.suggestions   yes-no yes    suggestions from history
setting  shell.colour        yes-no yes    any colour from the shell
setting  console.columns     number 20 500 80  width of the serial console
setting  console.term        choice dumb|vt100|xterm|xterm-256color  dumb  the TERM of programs on the serial console; dumb says full-screen programs will not draw exactly there
setting  term.scrollback     number 100 10000 2000  lines of scrollback a terminal window keeps
---
The shell people type commands into. help language describes what it reads.

FILES
  /config/settings   settings
  /config/start.zosh run before the first prompt
  /config/history    the lines run
