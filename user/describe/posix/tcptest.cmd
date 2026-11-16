name     tcptest
summary  test TCP listen, accept and connect on the loopback (a POSIX test program)
usage    tcptest [--no-listen | --no-reach | --serve PORT TAG | --probe PORT EXPECT [COUNT]]
operand  [WORD...]  text  with --serve a port and the tag byte to answer with; with --probe a port, a tag byte or none, and a count
program  /bin/tcptest
option   -L --no-listen  check that listen is refused (a line with no +listen word)
option   -R --no-reach  check that a connect to 127.0.0.1 is refused (a line with no +net word)
option   -S --serve  listen on PORT and answer each connection with the one byte TAG (plan part F9)
option   -P --probe  connect to PORT COUNT times: every one must be refused (none) or answer TAG
exit     0  every check passed
exit     1  a check failed; the output says which line
---
tcptest is the C test of plan part F7b (proposed ADR-0441): one program listens, accepts and connects over netd's loopback,
with the refusals (no listen rule, a port in use, the account's one listener, a loopback address no rule names, the
queue of 64 pending connections, a listener closed with connections pending). It needs `+listen` words for its ports
and a `+net 127.0.0.1` word on the line, which is how a person grants a program a listener and the loopback.

The F9 modes test each room's own loopback (proposed ADR-0442): a --serve job in one room is reached by a --probe of that
room and by none of another.

EXAMPLES
  +listen 9000-9009 +listen 50000-50010 +net 127.0.0.1 tcptest
  +listen 9100 tcptest --serve 9100 A &
  +net 127.0.0.1:9100 tcptest --probe 9100 A
