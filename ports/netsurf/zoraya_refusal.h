/*
 * The page NetSurf shows when Zoraya's network service refused a host
 * (ADR-0325): its host, its port and the line that would allow it. Plain C
 * with no NetSurf header, so that `make check-netsurf-refusal` tests it on
 * the host.
 */
#ifndef ZORAYA_REFUSAL_H
#define ZORAYA_REFUSAL_H

#include <stddef.h>

/*
 * The line that would allow `url` (whose host is `host` and port `port`):
 * `+net HOST:PORT netsurf URL`, the URL in single quotes when it holds a
 * character outside A-Za-z0-9 and . _ ~ : / % + , = @ -, a ' written %27 and
 * any byte outside printable ASCII written %XX. Writes it with a NUL into
 * `out` and returns its length; 0 when the host cannot be a +net word (not a
 * lower-case host name or a dotted address) or the line does not fit.
 */
size_t zoraya_refusal_line(const char *url, const char *host, unsigned port,
			   char *out, size_t size);

/*
 * The whole page (HTML, UTF-8) into `out` with a NUL: the title
 * `Not granted: HOST:PORT`, the heading, the line when there is one, every
 * part HTML-escaped. Returns its length, or 0 when it does not fit.
 */
size_t zoraya_refusal_page(const char *url, const char *host, unsigned port,
			   char *out, size_t size);

#endif
