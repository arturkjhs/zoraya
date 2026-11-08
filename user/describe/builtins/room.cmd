name     room
summary  list, enter, make or remove rooms
usage    room
usage    room NAME
usage    room --leave
usage    room new NAME GRANT...
usage    room remove NAME
operand  [WORDS...]  text  a room to enter, or new or remove and their words
option   --leave           go back to home
option   --untrusted       with new: an untrusted room (refused until quotas, ADR-0056)
cwd      none
exit     0  done
exit     1  no such room, a bad name or grant, or a grant outside what the shell holds
---
A room is a named set of grants that programs run in (ADR-0172). In a room the
prompt shows its name, and every line gets the room's grants as if written before
the command; grants shows them with (room NAME). The table is /config/rooms, one
room a line; the desktop's dock shows the same rooms.

EXAMPLES
  room new project +w ~/project +r ~/include
  room project
  room --leave

FILES
  /config/rooms

SEE ALSO
  help grants
