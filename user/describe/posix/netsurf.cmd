name     netsurf
summary  browse the Web in a window (NetSurf, a POSIX program)
usage    netsurf [-v] [URL]
program  /bin/netsurf
cwd      none
window   NetSurf
option   -v --verbose  print NetSurf's log (warnings and information) on the error stream
operand  [URL]  url  the first page, http(s)://HOST[:PORT]/PATH; this command may reach that host and port only
exit     0  closed by the person
exit     1  it could not start
---
NetSurf 3.11 from the ports (ADR-0109, ADR-0318, TASK-27 9d), as a POSIX program with a window (ADR-0317). It is
given a window of the desktop, drawn with the grants of the line as pills (so the person sees that it reaches the
network), and nothing it does with the window can move, resize or raise it: the person does that. Only a
terminal of a trusted room can run it; in an untrusted room the line is refused.

With a URL it gets a connection that reaches the host and port of that address and nothing else, and, with it,
/system/tls (the system's roots, read only): a certificate that is expired, is for another name or is signed by
no root in /system/tls/roots is refused, and there is no option to turn that off. A link to another host or port
is refused by the network, not by NetSurf: add `+net HOST` words to the line for the hosts a page may use.
Without a URL it shows its own welcome page and reaches no host.

Its resources are in /usr/share/netsurf, which the toolchain set gives. It gets no current directory, so it
cannot save a page or a download (the chooser comes later); its cache and settings are in memory.

Known limits: JavaScript and SVG are not built; the text uses a bitmap font with Latin characters (ADR-0318);
there is no cursor shape and no clipboard (ADR-0317). The wheel scrolls the page or the frame under the
pointer, 100 pixels a notch; a horizontal wheel does nothing (libnsfb has no button for it).

EXAMPLES
  netsurf
  netsurf https://example.org/
  netsurf http://10.0.2.2:8080/

SEE ALSO
  curl, wget, grants
