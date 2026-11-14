name     netsurf
summary  browse the Web in a window (NetSurf, a POSIX program)
usage    netsurf [-v] [URL]
program  /bin/netsurf
cwd      none
window   NetSurf
net      web
option   -v --verbose  print NetSurf's log (warnings and information) on the error stream
operand  [URL]  url  the first page, http(s)://HOST[:PORT]/PATH; this command may reach that host and port only
exit     0  closed by the person
exit     1  it could not start
---
NetSurf 3.11 from the ports (ADR-0109, ADR-0318, TASK-27 9d), as a POSIX program with a window (ADR-0317). It is
given a window of the desktop, drawn with the grants of the line as pills (so the person sees that it reaches the
network), and nothing it does with the window can move, resize or raise it: the person does that. Only a
terminal of a trusted room can run it; in an untrusted room the line is refused.

It reaches the web: any public host on ports 80 and 443, with names resolved for it (ADR-0324; the window shows a
`web` pill and `grants netsurf` a line `net: web`), so links, redirects and images to other sites work. It never
reaches this machine, its local network (10.0.0.0/8 with QEMU's 10.0.2.0/24, 172.16.0.0/12, 192.168.0.0/16, the
gateway, the DNS servers), loopback, link-local addresses or other ports, also when a name resolves to one of them;
a `+net HOST` word on the line (or the URL itself) names such a host. With it comes /system/tls (the system's
roots, read only): a certificate that is expired, is for another name or is signed by no root in
/system/tls/roots is refused, and there is no option to turn that off. A URL on the line also gets its own host
and port. When the network refuses a host, NetSurf shows a page that names it and the line that would allow it,
`+net HOST:PORT netsurf URL` (ADR-0325). In an untrusted room the line is refused.

Its resources are in /usr/share/netsurf, which the toolchain set gives. It gets no current directory, so it
cannot save a page or a download (the chooser comes later); its cache and settings are in memory.

Known limits: JavaScript and SVG are not built; the text uses a bitmap font with Latin characters (ADR-0318);
there is no cursor shape and no clipboard (ADR-0317). The wheel scrolls the page or the frame under the
pointer, 100 pixels a notch; a horizontal wheel does nothing (libnsfb has no button for it).

EXAMPLES
  netsurf
  netsurf https://example.org/
  netsurf http://10.0.2.2:8080/
  +net 192.168.1.1:80 netsurf http://192.168.1.1/

SEE ALSO
  curl, wget, grants
