name     zfetch
summary  fetch a page over HTTP (a POSIX program)
usage    zfetch [--sum] URL
program  /bin/zfetch
operand  URL  url  the address, http://HOST[:PORT]/PATH; this command may reach that host and port only
option   -s --sum  print the size, an FNV-1a checksum and the time instead of the page
exit     0  fetched
exit     1  not fetched
---
zfetch is the C program of N4 (TASK-26): `fetch` written against sockets, `getaddrinfo` and `libposix`, to show that a
POSIX program reaches the network as a native one does. It prints the body of a 200 response, or with --sum the line
`N bytes fnv1a HEX in M.mmm ms` that `fetch --sum` prints. The shell gives it a network connection that reaches the
host and port of URL and nothing else, and only when URL is written on the line.

EXAMPLES
  zfetch http://10.0.2.2:8080/
  zfetch --sum http://test.zoraya:8080/chunked/300000
