name     wget
summary  download a page over HTTP or HTTPS (GNU wget, a POSIX program)
usage    wget [-qv] [-O FILE] [-T SECONDS] [-t N] [-U AGENT] [-4] URL...
program  /bin/wget
cwd      none
operand  URL...  url  the address, http(s)://HOST[:PORT]/PATH; this command may reach those hosts and ports only
option   -O --output-document FILE create  write the page to FILE ("-" for the terminal)
option   -q --quiet  print nothing
option   -v --verbose  print everything (the default)
option   -T --timeout SECONDS text  give up after this many seconds without data
option   -t --tries N text  how many times to try, 20 if not given
option   -U --user-agent AGENT text  the User-Agent header to send
option   -4 --inet4-only  use IPv4 (the only family there is)
option   --max-redirect N text  how many redirects to follow, 20 if not given
exit     0  downloaded
exit     1  generic error
exit     3  could not write the file
exit     4  network failure
exit     5  the certificate could not be verified
exit     8  the server answered with an error status
---
GNU wget 1.25.0 from the ports (ADR-0109, TASK-27 9a), as a POSIX program with the toolchain set, over OpenSSL 3.5
(static, with threads built in, used by neither program). It gets a connection that reaches the host and port of each URL written on the line and
nothing else, and, with it, /system/tls (the system's roots, read only): a certificate that is expired, is for
another name or is signed by no root in /system/tls/roots fails the transfer, and there is no option to turn that
off (--no-check-certificate and the rest are not offered). An address from a variable or a command is refused by
the shell, as for `fetch`.

-O FILE creates or replaces FILE, the only file in the current directory the program is given: it cannot read, list,
create or replace another there (a path below it, -O sub/f, gives that one file and nothing of the directory; -O - is the
terminal and gives none). Without -O, wget would save under the last part of the address in the current
directory, which it does not get: give +w . before the command to allow that, or name the file with -O FILE. A
redirect to a host and port the line does not name fails for want of a connection.

No configuration file is read (.wgetrc, WGETRC, SYSTEM_WGETRC, --config and -e are not there), HSTS and proxies are
off, and neither the environment's SSL_CERT_FILE or SSL_CERT_DIR nor OpenSSL's OPENSSL_CONF is read: what wget trusts
is the system's roots and nothing a file or a variable says.

EXAMPLES
  wget -O page.html https://example.org/
  wget -qO - http://10.0.2.2:8080/

SEE ALSO
  fetch, curl, grants
