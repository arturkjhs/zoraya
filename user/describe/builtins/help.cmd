name     help
summary  show help
usage    help [NAME]
usage    help --search WORD
operand  [NAME]  text  a command, a function or a topic
option   --search WORD text  list the pages whose name or summary contains WORD
cwd      none
exit     0  shown
exit     1  no such page
---
Without NAME, lists the topics and every command with a line about it. With NAME, shows its page,
a screen at a time.

EXAMPLES
  help
  help cp
  help --search copy
  help tour
