name     top
summary  each room's use against its limits
usage    top
usage    top ROOM
usage    top --watch
operand  [ROOM]  text  one room: its row, its limits and what refused
option   -w --watch        print again every second until Ctrl-C
cwd      none
exit     0  printed
exit     1  no such room, or no room keeper
---
Prints one row for each room that runs, then the session: how much memory, CPU,
processes, disk and connections it uses, and, where it has a limit, the limit
(USED of LIMIT). CPU is the share of the machine since the last time this shell
asked. A row ends with the limits that refused, if any did. The desktop shows
the same numbers: a window's pill at 90% of a limit and its hover card.

A room's own shell sees that room only. DISK and CONNS show the limit; their use
is not reported yet.

EXAMPLES
  top
  top agent-1
  top --watch

SEE ALSO
  help room
