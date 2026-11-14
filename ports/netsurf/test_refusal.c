/*
 * INV-0325-1 (the page's text) and INV-0325-2 (escaping and quoting): the
 * refusal page of ADR-0325 on the host (`make check-netsurf-refusal`).
 */
#include "zoraya_refusal.h"

#include <stdio.h>
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
	/* A page that does not fit is nothing. */
	if (zoraya_refusal_page("http://x/", "x", 80, page, 100) != 0 || page[0] != '\0') {
		printf("FAIL a page that does not fit\n");
		failures++;
	}
	(void)n;
	if (failures) {
		printf("check-netsurf-refusal: %d failures\n", failures);
		return 1;
	}
	printf("check-netsurf-refusal: ok\n");
	return 0;
}
