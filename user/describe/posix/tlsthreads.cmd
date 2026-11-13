name     tlsthreads
summary  TLS handshakes from four threads at once (a POSIX program, the test of OpenSSL with threads)
usage    tlsthreads [-r REASON] URL...
program  /bin/tlsthreads
cwd      none
operand  URL...  url  the address, https://HOST:PORT/; this command may reach those hosts and ports only
option   -r --refuse REASON text  every handshake must be refused for that certificate reason: expired, or name (another name)
exit     0  every handshake went as expected
exit     1  one did not
exit     2  a usage mistake
---
tlsthreads is the test program of ADR-0309 item 4 (TASK-27 9a): four threads share one OpenSSL context that trusts
/system/tls/roots and run three handshakes each against every URL of the line. Without -r each must succeed and
return the server's test page; with -r each must be refused for the reason given (expired, or name for a certificate for another name), and
the program sends no request byte. The shell gives it a connection that reaches the host and port of each URL and
/system/tls to read, and nothing else.

EXAMPLES
  tlsthreads https://test.zoraya:8443/
  tlsthreads -r expired https://expired.zoraya:8444/
