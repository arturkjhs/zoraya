name     room
summary  list, enter, make or remove rooms
usage    room
usage    room NAME
usage    room --leave
usage    room new NAME GRANT...
usage    room set NAME WORD VALUE...
usage    room remove NAME
operand  [WORDS...]  text  a room to enter, or new or remove and their words
option   --leave           go back to home
option   --untrusted       with new: an untrusted room (refused until quotas, ADR-0056)
option   --mem SIZE text   memory of everything in the room: 512M, 2G; none removes it (trusted rooms)
option   --cpu PCT text    at most this share of the machine: 25%
option   --procs N text    live processes
option   --disk SIZE text  on each +w directory of the room
option   --conns N text    network connections
option   --tmp SIZE text   the /tmp of each shell in an untrusted room, inside --mem
option   --kill            with remove: end the room's processes first
cwd      none
exit     0  done
exit     1  no such room, a bad name or grant, or a grant outside what the shell holds
---
A room is a named set of grants and limits that programs run in (ADR-0172). In a room the
prompt shows its name, and every line gets the room's grants as if written before
the command; grants shows them with (room NAME). The table is /config/rooms, one
room a line; the desktop's dock shows the same rooms.

EXAMPLES
  room new project +w ~/project +r ~/include
  room new agent-1 --mem 64M --cpu 25% +w ~/sandbox/agent-1
  room set agent-1 --mem 128M
  room project
  room --leave

FILES
  /config/rooms

SEE ALSO
  help grants
