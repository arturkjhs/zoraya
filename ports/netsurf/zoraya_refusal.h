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
 * The longest URL the line repeats, and the most of a URL the page shows:
 * a longer one is cut on the page (with a mark that says so), and its line
 * opens the host's first page instead (`zoraya_refusal_page`).
 */
#define ZORAYA_REFUSAL_URL_MAX 1024

/*
 * The longest host the page shows; a longer one is cut with a mark (it can
 * never be a +net word, so it has no line).
 */
#define ZORAYA_REFUSAL_HOST_MAX 253

/*
 * A buffer of this size always holds the page, whatever the URL and the
 * host: every part is bounded above (`make check-netsurf-refusal` tests the
 * worst cases).
 */
#define ZORAYA_REFUSAL_PAGE_MAX 16384

/*
 * Whether the shell takes `host` as the word of `+net HOST:PORT` for that
 * one host: a lower-case host name (labels of a-z 0-9 - _, single dots, at
 * most 253 bytes, the last label not all digits) or a dotted address, and
 * not a word the shell reads otherwise (`any`, every host; `web`, which a
 * reader takes for the web grant). Mirrors `parse_word` and `valid_name` of
 * crates/zoraya-shell and crates/zoraya-abi; `*.`, `/` and `:` are not in
 * a host it takes.
 */
int zoraya_refusal_plain_host(const char *host);

/*
 * The line that would allow `url` (whose host is `host` and port `port`):
 * `+net HOST:PORT netsurf URL`, the URL in single quotes when it holds a
 * character outside A-Za-z0-9 and . _ ~ : / % + , = @ -, a ' written %27 and
 * any byte outside printable ASCII written %XX. Writes it with a NUL into
 * `out` and returns its length; 0 when the host cannot be a +net word
 * (`zoraya_refusal_plain_host`) or the line does not fit.
 */
size_t zoraya_refusal_line(const char *url, const char *host, unsigned port,
			   char *out, size_t size);

/*
 * The whole page (HTML, UTF-8) into `out` with a NUL: the title
 * `Not granted: HOST:PORT`, the heading, the line when there is one, every
 * part HTML-escaped. A URL longer than ZORAYA_REFUSAL_URL_MAX gets the line
 * of the host's first page (`+net HOST:PORT netsurf SCHEME://HOST:PORT/`),
 * and its address on the page is cut, with a mark. Returns its length, or 0
 * when it does not fit (never with ZORAYA_REFUSAL_PAGE_MAX bytes).
 */
size_t zoraya_refusal_page(const char *url, const char *host, unsigned port,
			   char *out, size_t size);

#endif
