/*
 * INV-0325-1 (the page's text) and INV-0325-2 (escaping and quoting): the
 * refusal page of ADR-0325 on the host (`make check-netsurf-refusal`).
 */
#include "zoraya_refusal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;

static void line_is(const char *url, const char *host, unsigned port, const char *want)
{
	char out[2400];
	size_t n = zoraya_refusal_line(url, host, port, out, sizeof out);
	if (strcmp(out, want) != 0 || n != strlen(want)) {
		printf("FAIL line(%s, %s, %u): got [%s] (%zu), want [%s]\n", url, host, port, out, n, want);
		failures++;
	}
}

static void page_has(const char *page, const char *part)
{
	if (strstr(page, part) == NULL) {
		printf("FAIL page lacks [%s]\n", part);
		failures++;
	}
}

static void page_lacks(const char *page, const char *part)
{
	if (strstr(page, part) != NULL) {
		printf("FAIL page holds [%s]\n", part);
		failures++;
	}
}

/* 9f-net review, finding 4: a long URL and a long host. The page always
 * fits ZORAYA_REFUSAL_PAGE_MAX: the address is cut with a mark, and a URL
 * too long to repeat gets the line of the host's first page, never a claim
 * that the host cannot be written. */
static void long_urls(void)
{
	static char page[ZORAYA_REFUSAL_PAGE_MAX];
	size_t total = 20000, n, i;
	char *url = malloc(total + 1);
	char *host = malloc(1001);
	const char fill[] = { 'a', '&', '"', '\'', '<' };
	char prefix[300];
	if (url == NULL || host == NULL) {
		printf("FAIL out of memory\n");
		failures++;
		return;
	}
	/* Just under, at and over the bound, with the widest bytes. */
	for (i = 0; i < sizeof fill; i++) {
		size_t lens[] = { ZORAYA_REFUSAL_URL_MAX, ZORAYA_REFUSAL_URL_MAX + 1, 2400, total };
		size_t k;
		for (k = 0; k < sizeof lens / sizeof lens[0]; k++) {
			size_t len = lens[k], p;
			/* The longest host the shell takes: 253 bytes. */
			memset(host, 'h', 253);
			host[63] = host[127] = host[191] = '.';
			host[253] = '\0';
			p = (size_t)snprintf(url, total + 1, "https://%s:65535/", host);
			memset(url + p, fill[i], len - p);
			url[len] = '\0';
			n = zoraya_refusal_page(url, host, 65535, page, sizeof page);
			if (n == 0 || n != strlen(page)) {
				printf("FAIL a URL of %zu bytes of %c: no page\n", len, fill[i]);
				failures++;
				continue;
			}
			page_has(page, "background:#ffe4b5");
			page_lacks(page, "cannot be written");
			snprintf(prefix, sizeof prefix, "<pre>+net %s:65535 netsurf ", host);
			page_has(page, prefix);
			if (len > ZORAYA_REFUSAL_URL_MAX) {
				snprintf(prefix, sizeof prefix, "netsurf https://%s:65535/</pre>", host);
				page_has(page, prefix);
				page_has(page, "too long to repeat");
				page_has(page, "(cut: the address is longer)</p>");
			} else {
				page_lacks(page, "too long to repeat");
				page_lacks(page, "(cut:");
			}
		}
	}
	/* The default port is left out of the first page's URL. */
	memset(url, 'a', total);
	memcpy(url, "http://h.example/", 17);
	url[total] = '\0';
	n = zoraya_refusal_page(url, "h.example", 80, page, sizeof page);
	page_has(page, "<pre>+net h.example:80 netsurf http://h.example/</pre>");
	/* A long host that is no +net word: cut with a mark, no line. */
	memset(host, '&', 1000);
	host[1000] = '\0';
	memset(url, '"', total);
	url[total] = '\0';
	n = zoraya_refusal_page(url, host, 80, page, sizeof page);
	if (n == 0) {
		printf("FAIL a long host: no page\n");
		failures++;
	}
	page_has(page, "&amp;...:80</title>");
	page_has(page, "cannot be written");
	page_lacks(page, "<pre>");
	/* A cut never splits a UTF-8 character: \xc3\xa9 across the bound. */
	memset(url, 'a', total);
	memcpy(url, "http://h.example/", 17);
	url[ZORAYA_REFUSAL_URL_MAX - 1] = '\xc3';
	url[ZORAYA_REFUSAL_URL_MAX] = '\xa9';
	url[ZORAYA_REFUSAL_URL_MAX + 10] = '\0';
	n = zoraya_refusal_page(url, "h.example", 80, page, sizeof page);
	page_lacks(page, "\xc3 ...");
	page_has(page, "a ... (cut");
	free(url);
	free(host);
}

int main(void)
{
	char page[8192];
	size_t n;

	/* The smallest grant: the host and the port the URL names. */
	line_is("http://10.0.2.2:8080/ns/page", "10.0.2.2", 8080,
		"+net 10.0.2.2:8080 netsurf http://10.0.2.2:8080/ns/page");
	line_is("http://127.0.0.1/", "127.0.0.1", 80, "+net 127.0.0.1:80 netsurf http://127.0.0.1/");
	line_is("https://intranet.example/a_b-c~d", "intranet.example", 443,
		"+net intranet.example:443 netsurf https://intranet.example/a_b-c~d");
	/* Characters the shell reads are quoted; a ' becomes %27; bytes outside
	 * printable ASCII become %XX. */
	line_is("http://h.example/?a=1&b=2", "h.example", 80,
		"+net h.example:80 netsurf 'http://h.example/?a=1&b=2'");
	line_is("http://h.example/it's", "h.example", 80,
		"+net h.example:80 netsurf 'http://h.example/it%27s'");
	line_is("http://h.example/a b;$(x)", "h.example", 80,
		"+net h.example:80 netsurf 'http://h.example/a%20b;$(x)'");
	line_is("http://h.example/\x01\xc3\xa9", "h.example", 80,
		"+net h.example:80 netsurf 'http://h.example/%01%C3%A9'");
	/* A host the shell cannot take gets no line. */
	line_is("http://[::1]/", "[::1]", 80, "");
	line_is("http://A.example/", "A.example", 80, "");
	line_is("http://a..b/", "a..b", 80, "");
	line_is("http://.a/", ".a", 80, "");
	line_is("http://a./", "a.", 80, "");
	line_is("http://x/", "", 80, "");
	line_is("http://x/", "x", 0, "");
	line_is("http://x/", "x", 65536, "");
	line_is("http://x/", "x", 65535, "+net x:65535 netsurf http://x/");
	/* Words the shell reads otherwise are no host's word (9f-net review,
	 * finding 1): `+net any:80` would reach every host on port 80, and
	 * `+net web` reads as the web grant. Hosts literally named so get no
	 * line; names that only hold the words keep theirs. */
	line_is("http://any/", "any", 80, "");
	line_is("http://any:8080/x", "any", 8080, "");
	line_is("http://ANY/", "ANY", 80, "");
	line_is("http://web/", "web", 80, "");
	line_is("https://web/", "web", 443, "");
	line_is("http://any.example/", "any.example", 80, "+net any.example:80 netsurf http://any.example/");
	line_is("http://web.example/", "web.example", 80, "+net web.example:80 netsurf http://web.example/");
	line_is("http://company/", "company", 80, "+net company:80 netsurf http://company/");
	/* A wildcard, a prefix or a port in the host are the shell's other
	 * readings: never in a host's word. */
	line_is("http://x/", "*.example", 80, "");
	line_is("http://x/", "10.0.0.0/8", 80, "");
	line_is("http://x/", "x:1", 80, "");
	/* A host whose last label is all digits is a dotted address or nothing
	 * the shell takes. */
	line_is("http://1.2.3/", "1.2.3", 80, "");
	line_is("http://a.1/", "a.1", 80, "");
	line_is("http://256.1.1.1/", "256.1.1.1", 80, "");
	line_is("http://1.2.3.4.5/", "1.2.3.4.5", 80, "");
	line_is("http://1.2.3.0004/", "1.2.3.0004", 80, "");
	line_is("http://255.255.255.255/", "255.255.255.255", 80,
		"+net 255.255.255.255:80 netsurf http://255.255.255.255/");
	line_is("http://1.2.3.x4/", "1.2.3.x4", 80, "+net 1.2.3.x4:80 netsurf http://1.2.3.x4/");
	if (!zoraya_refusal_plain_host("a.example") || zoraya_refusal_plain_host("any")
	    || zoraya_refusal_plain_host("web")) {
		printf("FAIL zoraya_refusal_plain_host\n");
		failures++;
	}
	/* Too small a buffer: nothing. */
	{
		char small[10];
		if (zoraya_refusal_line("http://x/", "x", 80, small, sizeof small) != 0 || small[0] != '\0') {
			printf("FAIL a line that does not fit\n");
			failures++;
		}
	}

	/* The page: title, heading, the line, the colour, everything escaped. */
	n = zoraya_refusal_page("http://10.0.2.2:8080/ns/x?q=<b>&r='\"", "10.0.2.2", 8080, page, sizeof page);
	if (n == 0 || n != strlen(page)) {
		printf("FAIL the page was not made (%zu)\n", n);
		failures++;
	}
	page_has(page, "<title>Not granted: 10.0.2.2:8080</title>");
	page_has(page, "<h1>NetSurf may not reach 10.0.2.2:8080</h1>");
	page_has(page, "background:#ffe4b5");
	page_has(page, "<pre>+net 10.0.2.2:8080 netsurf &#39;http://10.0.2.2:8080/ns/x?q=&lt;b&gt;&amp;r=%27&quot;&#39;</pre>");
	page_has(page, "Address: http://10.0.2.2:8080/ns/x?q=&lt;b&gt;&amp;r=&#39;&quot;</p>");
	page_lacks(page, "<b>");
	page_lacks(page, "'\"");
	/* A host that would be markup is escaped and gets no line. */
	n = zoraya_refusal_page("http://x/", "<script>", 80, page, sizeof page);
	page_has(page, "Not granted: &lt;script&gt;:80");
	page_lacks(page, "<script>");
	page_lacks(page, "<pre>");
	page_has(page, "no line can allow it");
	/* A host named any gets the page with no line. */
	n = zoraya_refusal_page("http://any:8080/x", "any", 8080, page, sizeof page);
	page_has(page, "<h1>NetSurf may not reach any:8080</h1>");
	page_has(page, "cannot be written as a <code>+net</code> word");
	page_lacks(page, "<pre>");
	page_lacks(page, "+net any");
	/* A page that does not fit is nothing. */
	if (zoraya_refusal_page("http://x/", "x", 80, page, 100) != 0 || page[0] != '\0') {
		printf("FAIL a page that does not fit\n");
		failures++;
	}
	(void)n;
	long_urls();
	if (failures) {
		printf("check-netsurf-refusal: %d failures\n", failures);
		return 1;
	}
	printf("check-netsurf-refusal: ok\n");
	return 0;
}
