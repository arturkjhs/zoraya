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

/* Text into HTML: & < > " ' escaped. */
static void put_html(struct out *o, const char *s)
{
	for (; *s != '\0'; s++) {
		switch (*s) {
		case '&': puts_(o, "&amp;"); break;
		case '<': puts_(o, "&lt;"); break;
		case '>': puts_(o, "&gt;"); break;
		case '"': puts_(o, "&quot;"); break;
		case '\'': puts_(o, "&#39;"); break;
		default: put(o, s, 1);
		}
	}
}

/* A host the shell takes in +net HOST: labels of a-z 0-9 - _, separated by
 * single dots, at most 253 bytes (a dotted address is such a host too). */
static int plain_host(const char *h)
{
	size_t n = strlen(h), label = 0, i;
	if (n == 0 || n > 253)
		return 0;
	for (i = 0; i < n; i++) {
		char c = h[i];
		if (c == '.') {
			if (label == 0)
				return 0;
			label = 0;
		} else if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' || c == '_') {
			if (++label > 63)
				return 0;
		} else {
			return 0;
		}
	}
	return label > 0;
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
	if (!plain_host(host) || port == 0 || port > 65535)
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

size_t zoraya_refusal_page(const char *url, const char *host, unsigned port,
			   char *out, size_t size)
{
	struct out o = { out, size, 0 };
	char line[2400];
	char number[16];
	size_t has_line;

	if (size == 0)
		return 0;
	out[0] = '\0';
	has_line = zoraya_refusal_line(url, host, port, line, sizeof line);
	snprintf(number, sizeof number, "%u", port);
	puts_(&o, "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Not granted: ");
	put_html(&o, host);
	puts_(&o, ":");
	puts_(&o, number);
	puts_(&o, "</title>\n<style>body{margin:0;background:#ffe4b5;color:#000000}"
		  "div{padding:16px}pre{background:#ffffff;padding:8px;white-space:pre-wrap}</style>\n"
		  "</head><body><div>\n<h1>NetSurf may not reach ");
	put_html(&o, host);
	puts_(&o, ":");
	puts_(&o, number);
	puts_(&o, "</h1>\n<p>The network grant of this window does not cover ");
	put_html(&o, host);
	puts_(&o, " on port ");
	puts_(&o, number);
	puts_(&o, ", so Zoraya refused it: no connection was made to it.</p>\n");
	if (has_line) {
		puts_(&o, "<p>This line, typed in a terminal, starts NetSurf with that host and port too:</p>\n<pre>");
		put_html(&o, line);
		puts_(&o, "</pre>\n");
	} else {
		puts_(&o, "<p>That host cannot be written in a <code>+net</code> word, so no line can allow it.</p>\n");
	}
	puts_(&o, "<p>This window's own grant, <code>web</code>, reaches public hosts on ports 80 and 443. "
		  "This machine, its local network, loopback and other ports need a <code>+net</code> word.</p>\n"
		  "<p>Address: ");
	put_html(&o, url);
	puts_(&o, "</p>\n</div></body></html>\n");
	if (o.full) {
		out[0] = '\0';
		return 0;
	}
	return (size_t)(o.at - out);
}
