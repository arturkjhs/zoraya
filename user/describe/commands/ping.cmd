name     ping
summary  send echo requests to a host and show the replies
usage    ping [--count N] [--timeout SECONDS] HOST
operand  HOST  host  the host to ping, a name or a dotted IPv4 address; this command may send echo requests to that host only
option   -c --count N text  how many echo requests, 1 to 16; 4 if not given
option   -W --timeout SECONDS text  how long to wait for the last reply, 1 to 30; 2 if not given
cwd      none
exit     0  at least one reply came
exit     1  no reply came, or the network is not available
exit     2  the options or the host are wrong
---
Sends COUNT ICMP echo requests, one every 200 ms, to HOST and prints a line for
each reply (`64 bytes from ADDRESS: seq=N time=T ms`), then a summary: how many
were sent and received, the loss, and the shortest, mean and longest round trip.
The network service sends the requests; this command holds no raw socket.

The shell gives the command a network connection that may send ICMP echo
requests to HOST and nothing else (no TCP connection, no datagram), and only
when HOST is written on the line: a host that comes from a variable or a
command is refused unless a +net word on the line covers it. A name is looked
up by the network service, inside that grant; the line `PING NAME (ADDRESS)`
shows what it came to. The network service
sends the requests and answers when the run has ended (the last reply or the
timeout), so the lines appear together at the end, not one by one. A request
that got no reply has no line.

EXAMPLES
  ping 10.0.2.2
  ping -c 2 -W 5 10.0.2.2

SEE ALSO
  net, fetch
