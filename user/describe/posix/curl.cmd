name     curl
summary  transfer a page over HTTP or HTTPS (curl, a POSIX program)
usage    curl [-sSfLiIv] [-o FILE | -O] [-m SECONDS] [-H HEADER] [-A AGENT] [-X METHOD] [-d DATA] URL...
program  /bin/curl
operand  URL...  url  the address, http(s)://HOST[:PORT]/PATH; this command may reach those hosts and ports only
option   -o --output FILE create  write the page to FILE instead of the terminal
option   -O --remote-name  write the page to a file named like the last part of the address, in the current directory
names    --remote-name URL
option   -s --silent  print no progress meter or error message
option   -S --show-error  with --silent, still print an error message
option   -f --fail  fail with exit status 22 and no page on an HTTP error status
option   -L --location  follow a redirect, to a host and port the line also names
option   -i --include  print the response headers before the page
option   -I --head  ask for the headers only
option   -v --verbose  print the request and the response headers on the error stream
option   -m --max-time SECONDS text  give up after this many seconds
option   --connect-timeout SECONDS text  give up connecting after this many seconds
option   -H --header HEADER text  add a request header
option   -A --user-agent AGENT text  the User-Agent header to send
option   -X --request METHOD text  the request method, if not GET
option   -d --data DATA text  send DATA as the body of a POST (a leading @ would read a file; none is given, so it fails)
option   -4 --ipv4  use IPv4 (the only family there is)
exit     0  transferred
exit     6  could not resolve the host
exit     7  could not connect
exit     22  an HTTP error status, with --fail
exit     28  timed out
exit     35  the TLS handshake failed
exit     60  the certificate is not signed by a trusted root, is expired, or is for another name
---
curl 8.23.0 from the ports (ADR-0109, TASK-27 9a), as a POSIX program with the toolchain set, over OpenSSL 3.5
(static, no threads). It gets a connection that reaches the host and port of each URL written on the line and nothing
else, and, with it, /system/tls (the system's roots, read only): a certificate that is expired, is for another name
or is signed by no root in /system/tls/roots is refused before any request is sent, and there is no option to turn
that off (-k, --cacert and the rest of curl's are not offered). An address from a variable or a command is refused
by the shell, as for `fetch`.

-o FILE creates or replaces FILE. -O saves under the last part of the address's path, in the current directory:
`curl -O https://example.org/a/b.txt` creates ./b.txt, and the shell works that name out itself, so an address
whose last part is empty or is not plain letters, digits and . - _ + ~ @ , = is refused (use -o). Without either,
the page goes to the terminal as received, escape sequences included.

Its random numbers come from the system's random service (getrandom), its clock from the system's. Only HTTP/1.1
and HTTPS (TLS 1.2 and 1.3) are built; there is no proxy, cookie jar, netrc or configuration file.

EXAMPLES
  curl http://10.0.2.2:8080/
  curl -O https://example.org/index.html
  curl -sSf -o page.html https://example.org/

SEE ALSO
  fetch, wget, grants
