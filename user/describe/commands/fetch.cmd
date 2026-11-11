name     fetch
summary  fetch a page over HTTP
usage    fetch [--output FILE] [--sum] URL
operand  URL  url  the address, http://HOST[:PORT]/PATH; this command may reach that host and port only
option   -o --output FILE create  write the page to FILE instead of the terminal
option   -s --sum  print the size, an FNV-1a checksum and the time instead of the page
cwd      none
exit     0  fetched
exit     1  not fetched
---
Fetches URL with an HTTP/1.1 GET and prints the body, or writes it to FILE.
Only http:// is understood (there is no TLS yet), redirects are not followed,
and a page over 64 MiB is refused. The shell gives the command a network
connection that reaches the host and port of URL and nothing else.

With --sum nothing of the page is kept: the line `N bytes fnv1a HEX in M.mmm ms (hashing H.hhh ms)` is
what the network tests compare, and the time, less the hashing, is what they measure.

EXAMPLES
  fetch http://10.0.2.2:8080/
  fetch --output page.txt http://test.zoraya/page.txt

SEE ALSO
  net
