name     rsnet
summary  the network test of Rust's std on Zoraya (a POSIX program built by tools/zcargo)
usage    rsnet URL
program  /bin/rsnet
operand  URL  url  the address, http://HOST[:PORT]/PATH; this command may reach that host and port only
exit     0  the group passed
exit     1  the group failed
---
rsnet is rstest installed under another name, so that its description can grant the connection to the URL on the line
(ADR-0453 item 4): std::net's TcpStream reaches that host and port, by address and by name, with a non-blocking connect
(connect_timeout); any other port is refused (EACCES). It prints "rstest: net passed".

EXAMPLES
  rsnet http://10.0.2.50:8080/pattern/300000
