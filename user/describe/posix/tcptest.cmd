name     tcptest
summary  test TCP listen, accept and connect on the loopback (a POSIX test program)
usage    tcptest [--no-listen | --no-reach]
program  /bin/tcptest
option   -L --no-listen  check that listen is refused (a line with no +listen word)
option   -R --no-reach  check that a connect to 127.0.0.1 is refused (a line with no +net word)
exit     0  every check passed
exit     1  a check failed; the output says which line
---
tcptest is the C test of plan part F7b (proposed ADR-0441): one program listens, accepts and connects over netd's loopback,
with the refusals (no listen rule, a port in use, the account's one listener, a loopback address no rule names, the
queue of 64 pending connections, a listener closed with connections pending). It needs `+listen` words for its ports
and a `+net 127.0.0.1` word on the line, which is how a person grants a program a listener and the loopback.

EXAMPLES
  +listen 9000-9009 +listen 50000-50010 +net 127.0.0.1 tcptest
