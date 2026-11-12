name     fetch
summary  fetch a page over HTTP
usage    fetch [--output FILE] [--sum] URL
operand  URL  url  the address, http(s)://HOST[:PORT]/PATH; this command may reach that host and port only
option   -o --output FILE create  write the page to FILE instead of the terminal
option   -s --sum  print the size, an FNV-1a checksum and the time instead of the page
cwd      none
exit     0  fetched
exit     1  not fetched
---
Fetches URL with an HTTP/1.1 GET and prints the body, or writes it to FILE.
With https:// the connection is TLS, checked against the roots in
/system/tls/roots (which the shell adds with the network, read only), the
system's date and the host named in the address: an expired certificate, a
certificate for another name or one no root signed fails before the request is
sent. Redirects are not followed, and a page over 64 MiB is refused. The shell gives the command a network
connection that reaches the host and port of URL and nothing else, and only
when URL is written on the line: an address that comes from a variable or a
command is refused. The page goes to the terminal as received, escape sequences
included. With --output, FILE is made or emptied when the first byte of a
successful page arrives, so a refused connection or a 404 leaves an existing
FILE as it was; a transfer cut half way leaves FILE partly written, with exit
status 1 and `connection reset` or a cut-connection message.

With --sum nothing of the page is kept: the line `N bytes fnv1a HEX in M.mmm ms (hashing H.hhh ms)` is
what the network tests compare, and the time, less the hashing, is what they measure.

EXAMPLES
  fetch https://example.org/
  fetch http://10.0.2.2:8080/
  fetch --output page.txt http://test.zoraya/page.txt

SEE ALSO
  net
