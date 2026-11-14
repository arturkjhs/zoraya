/*
 * The page of a refused host (ADR-0325). See zoraya_refusal.h.
 */
#include "zoraya_refusal.h"

#include <stdio.h>
#include <string.h>

/* A buffer that remembers whether anything did not fit. */
struct out {
	char *at;
	size_t left;
	int full;
};

static void put(struct out *o, const char *s, size_t n)
{
	if (o->full || n >= o->left) {
		o->full = 1;
		return;
	}
	memcpy(o->at, s, n);
	o->at += n;
	o->left -= n;
	*o->at = '\0';
}

static void puts_(struct out *o, const char *s)
{
	put(o, s, strlen(s));
}

/* At most `n` bytes of `s` into HTML: & < > " ' escaped. */
static void put_html_n(struct out *o, const char *s, size_t n)
{
	size_t i;
	for (i = 0; i < n && s[i] != '\0'; i++) {
		switch (s[i]) {
		case '&': puts_(o, "&amp;"); break;
		case '<': puts_(o, "&lt;"); break;
		case '>': puts_(o, "&gt;"); break;
		case '"': puts_(o, "&quot;"); break;
		case '\'': puts_(o, "&#39;"); break;
		default: put(o, s + i, 1);
		}
	}
}

static void put_html(struct out *o, const char *s)
{
	put_html_n(o, s, strlen(s));
}

/* `s` into HTML, at most `max` bytes of it (cut at the start of a UTF-8
 * character), then `mark` if it was cut. */
static void put_html_cut(struct out *o, const char *s, size_t max, const char *mark)
{
	size_t n = strlen(s);
	if (n <= max) {
		put_html_n(o, s, n);
		return;
	}
	n = max;
	while (n > 0 && ((unsigned char)s[n] & 0xc0) == 0x80)
		n--;
	put_html_n(o, s, n);
	puts_(o, mark);
}

static int digits(const char *s, size_t n)
{
	size_t i;
	for (i = 0; i < n; i++)
		if (s[i] < '0' || s[i] > '9')
			return 0;
	return n > 0;
}

/* A dotted address as the shell reads one (zoraya_http::parse_ipv4): four
 * parts of one to three digits, each at most 255. */
static int dotted(const char *h)
{
	int parts = 0;
	const char *p = h;
	for (;;) {
		const char *dot = strchr(p, '.');
		size_t n = dot ? (size_t)(dot - p) : strlen(p);
		unsigned v = 0;
		size_t i;
		if (n == 0 || n > 3 || !digits(p, n))
			return 0;
		for (i = 0; i < n; i++)
			v = v * 10 + (unsigned)(p[i] - '0');
		if (v > 255 || ++parts > 4)
			return 0;
		if (!dot)
			break;
		p = dot + 1;
	}
	return parts == 4;
}

int zoraya_refusal_plain_host(const char *h)
{
	size_t n = strlen(h), label = 0, i;
	const char *last = h;
	if (n == 0 || n > 253)
		return 0;
	/* Words the shell reads otherwise: `+net any` is every host, and
	 * `web` reads as the web grant (9f-net review, finding 1). */
	if (strcmp(h, "any") == 0 || strcmp(h, "web") == 0)
		return 0;
	for (i = 0; i < n; i++) {
		char c = h[i];
		if (c == '.') {
			if (label == 0)
				return 0;
			label = 0;
			last = h + i + 1;
		} else if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_') {
			if (++label > 63)
				return 0;
		} else {
			return 0;
		}
	}
	if (label == 0)
		return 0;
	/* A name never ends in a number: such a host is a dotted address or
	 * nothing the shell takes. */
	if (digits(last, strlen(last)))
		return dotted(h);
	return 1;
}

static int safe_in_line(unsigned char c)
{
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
	       strchr("._~:/%+,=@-", c) != NULL;
}

size_t zoraya_refusal_line(const char *url, const char *host, unsigned port,
			   char *out, size_t size)
{
	struct out o = { out, size, 0 };
	char head[300];
	const unsigned char *u;
	int quote = 0;

	if (size == 0)
		return 0;
	out[0] = '\0';
	if (!zoraya_refusal_plain_host(host) || port == 0 || port > 65535)
		return 0;
	for (u = (const unsigned char *)url; *u != '\0'; u++)
		if (!safe_in_line(*u))
			quote = 1;
	snprintf(head, sizeof head, "+net %s:%u netsurf ", host, port);
	puts_(&o, head);
	if (quote)
		puts_(&o, "'");
	for (u = (const unsigned char *)url; *u != '\0'; u++) {
		if (*u == '\'' || *u <= 0x20 || *u >= 0x7f) {
			char hex[4];
			snprintf(hex, sizeof hex, "%%%02X", *u);
			puts_(&o, hex);
		} else {
			put(&o, (const char *)u, 1);
		}
	}
	if (quote)
		puts_(&o, "'");
	if (o.full) {
		out[0] = '\0';
		return 0;
	}
	return (size_t)(o.at - out);
}

/* The first page of the URL's host: SCHEME://HOST[:PORT]/, the port left
 * out when it is the scheme's own. For the line of a URL too long to repeat. */
static void first_page(const char *url, const char *host, unsigned port, char *out, size_t size)
{
	int https = strncmp(url, "https://", 8) == 0;
	if (port == (https ? 443u : 80u))
		snprintf(out, size, "%s://%s/", https ? "https" : "http", host);
	else
		snprintf(out, size, "%s://%s:%u/", https ? "https" : "http", host, port);
}

size_t zoraya_refusal_page(const char *url, const char *host, unsigned port,
			   char *out, size_t size)
{
	struct out o = { out, size, 0 };
	/* Its longest: the head (5 + 253 + 6 + 9), quotes and 3 bytes for each
	 * of ZORAYA_REFUSAL_URL_MAX. */
	char line[300 + 3 * ZORAYA_REFUSAL_URL_MAX];
	char number[16];
	size_t has_line, url_len = strlen(url);
	int too_long = url_len > ZORAYA_REFUSAL_URL_MAX;

	if (size == 0)
		return 0;
	out[0] = '\0';
	if (too_long) {
		char first[300];
		first_page(url, host, port, first, sizeof first);
		has_line = zoraya_refusal_line(first, host, port, line, sizeof line);
	} else {
		has_line = zoraya_refusal_line(url, host, port, line, sizeof line);
	}
	snprintf(number, sizeof number, "%u", port);
	puts_(&o, "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Not granted: ");
	put_html_cut(&o, host, ZORAYA_REFUSAL_HOST_MAX, "...");
	puts_(&o, ":");
	puts_(&o, number);
	puts_(&o, "</title>\n<style>body{margin:0;background:#ffe4b5;color:#000000}"
		  "div{padding:16px}pre{background:#ffffff;padding:8px;white-space:pre-wrap}</style>\n"
		  "</head><body><div>\n<h1>NetSurf may not reach ");
	put_html_cut(&o, host, ZORAYA_REFUSAL_HOST_MAX, "...");
	puts_(&o, ":");
	puts_(&o, number);
	puts_(&o, "</h1>\n<p>The network grant of this window does not cover ");
	put_html_cut(&o, host, ZORAYA_REFUSAL_HOST_MAX, "...");
	puts_(&o, " on port ");
	puts_(&o, number);
	puts_(&o, ", so Zoraya refused it: no connection was made to it.</p>\n");
	if (has_line && too_long) {
		puts_(&o, "<p>The address is too long to repeat in a line. This line, typed in a terminal, "
			  "starts NetSurf with that host and port too, at the host's first page:</p>\n<pre>");
		put_html(&o, line);
		puts_(&o, "</pre>\n");
	} else if (has_line) {
		puts_(&o, "<p>This line, typed in a terminal, starts NetSurf with that host and port too:</p>\n<pre>");
		put_html(&o, line);
		puts_(&o, "</pre>\n");
	} else {
		puts_(&o, "<p>That host cannot be written as a <code>+net</code> word, so no line can allow it.</p>\n");
	}
	puts_(&o, "<p>This window's own grant, <code>web</code>, reaches public hosts on ports 80 and 443. "
		  "This machine, its local network, loopback and other ports need a <code>+net</code> word.</p>\n"
		  "<p>Address: ");
	put_html_cut(&o, url, ZORAYA_REFUSAL_URL_MAX, " ... (cut: the address is longer)");
	puts_(&o, "</p>\n</div></body></html>\n");
	if (o.full) {
		out[0] = '\0';
		return 0;
	}
	return (size_t)(o.at - out);
}
