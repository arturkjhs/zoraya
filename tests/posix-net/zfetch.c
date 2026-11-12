/*
 * zfetch: `fetch` as a POSIX program (N4 of TASK-26, ADR-0225, ADR-0287).
 *
 *   zfetch [-s|--sum] http://HOST[:PORT]/PATH
 *
 * One HTTP/1.1 GET with `Connection: close`, over getaddrinfo, socket,
 * connect, send and recv, nothing else of the network. The body of a 200
 * response goes to standard output, or with -s only its size and FNV-1a
 * checksum are printed, in the line `fetch --sum` prints, so that one test
 * compares the two programs. The body is decoded as the three framings the
 * test server gives: a length, chunks, and the end of the connection.
 * Only the names of docs/adr/posix-surface-T1.txt and ADR-0287 are used.
 */
#include <errno.h>
#include <netdb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define MAX_HEAD 8192

static int sum_only;
static uint64_t size, hash = 0xcbf29ce484222325ULL;

static int fail(const char *what, const char *why)
{
	fprintf(stderr, "zfetch: %s: %s\n", what, why);
	return 1;
}

static void body(const unsigned char *p, size_t n)
{
	size += n;
	if (sum_only) {
		for (size_t i = 0; i < n; i++)
			hash = (hash ^ p[i]) * 0x100000001b3ULL;
	} else if (n) {
		fwrite(p, 1, n, stdout);
	}
}

static int lower(int c)
{
	return c >= 'A' && c <= 'Z' ? c + 32 : c;
}

/* Whether the header line starts with `name` (lower case, with its colon),
 * ignoring case; `*value` is then the text after the colon and spaces. */
static int header(const char *line, const char *name, const char **value)
{
	size_t i = 0;
	for (; name[i]; i++)
		if (lower((unsigned char)line[i]) != name[i])
			return 0;
	line += i;
	while (*line == ' ' || *line == '\t')
		line++;
	*value = line;
	return 1;
}

enum mode { LENGTH, CHUNKED, CLOSE };
enum chunk { C_SIZE, C_DATA, C_CRLF, C_TRAILER, C_DONE };

static enum mode mode = CLOSE;
static uint64_t left;
static enum chunk chunk = C_SIZE;
static char line[64];
static size_t line_len;
static int bad;

/* Feeds body bytes through the framing; returns 1 when the body is over. */
static int feed(const unsigned char *p, size_t n)
{
	if (mode == CLOSE) {
		body(p, n);
		return 0;
	}
	if (mode == LENGTH) {
		size_t take = n < left ? n : (size_t)left;
		body(p, take);
		left -= take;
		return left == 0;
	}
	while (n && chunk != C_DONE) {
		if (chunk == C_DATA) {
			size_t take = n < left ? n : (size_t)left;
			body(p, take);
			p += take;
			n -= take;
			left -= take;
			if (left == 0)
				chunk = C_CRLF, line_len = 0;
			continue;
		}
		/* A line state: C_SIZE, C_CRLF or C_TRAILER. */
		unsigned char c = *p++;
		n--;
		if (c != '\n') {
			if (line_len < sizeof line - 1)
				line[line_len++] = (char)c;
			else
				bad = 1;
			continue;
		}
		line[line_len] = 0;
		if (line_len && line[line_len - 1] == '\r')
			line[--line_len] = 0;
		if (chunk == C_CRLF) {
			if (line_len)
				bad = 1;
			chunk = C_SIZE;
		} else if (chunk == C_SIZE) {
			char *end;
			unsigned long long v = strtoull(line, &end, 16);
			if (end == line || (*end && *end != ';') || v > (1ULL << 40)) {
				bad = 1;
				return 1;
			}
			left = v;
			chunk = v ? C_DATA : C_TRAILER;
		} else if (line_len == 0) {
			chunk = C_DONE;
		}
		line_len = 0;
	}
	return chunk == C_DONE;
}

static uint64_t now_us(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (uint64_t)t.tv_sec * 1000000 + (uint64_t)t.tv_nsec / 1000;
}

int main(int argc, char **argv)
{
	const char *url = NULL;
	for (int i = 1; i < argc; i++) {
		if (strcmp(argv[i], "-s") == 0 || strcmp(argv[i], "--sum") == 0)
			sum_only = 1;
		else if (!url)
			url = argv[i];
		else
			return fail(argv[i], "one URL only");
	}
	if (!url)
		return fail("usage", "zfetch [--sum] URL");
	if (strncmp(url, "http://", 7) != 0)
		return fail(url, "only http:// is understood");
	char host[256], port[8] = "80", path[2048] = "/";
	const char *h = url + 7, *slash = strchr(h, '/');
	size_t hl = slash ? (size_t)(slash - h) : strlen(h);
	if (slash) {
		if (strlen(slash) >= sizeof path)
			return fail(url, "the path is too long");
		strcpy(path, slash);
	}
	const char *colon = memchr(h, ':', hl);
	size_t nl = colon ? (size_t)(colon - h) : hl;
	if (nl == 0 || nl >= sizeof host)
		return fail(url, "no host");
	memcpy(host, h, nl);
	host[nl] = 0;
	if (colon) {
		size_t pl = hl - nl - 1;
		if (pl == 0 || pl >= sizeof port)
			return fail(url, "bad port");
		memcpy(port, colon + 1, pl);
		port[pl] = 0;
	}

	uint64_t started = now_us();
	struct addrinfo hints, *res, *ai;
	memset(&hints, 0, sizeof hints);
	hints.ai_family = AF_INET;
	hints.ai_socktype = SOCK_STREAM;
	hints.ai_flags = AI_NUMERICSERV;
	int g = getaddrinfo(host, port, &hints, &res);
	if (g != 0)
		return fail(host, g == EAI_SYSTEM ? strerror(errno) : gai_strerror(g));
	int fd = -1, err = 0;
	for (ai = res; ai; ai = ai->ai_next) {
		fd = socket(ai->ai_family, ai->ai_socktype, ai->ai_protocol);
		if (fd < 0)
			return fail("socket", strerror(errno));
		if (connect(fd, ai->ai_addr, ai->ai_addrlen) == 0)
			break;
		err = errno;
		close(fd);
		fd = -1;
	}
	freeaddrinfo(res);
	if (fd < 0)
		return fail(host, strerror(err));

	char req[2400];
	int rl = snprintf(req, sizeof req,
			  "GET %s HTTP/1.1\r\nHost: %s:%s\r\nConnection: close\r\n"
			  "User-Agent: zfetch\r\n\r\n", path, host, port);
	for (int sent = 0; sent < rl;) {
		ssize_t n = send(fd, req + sent, (size_t)(rl - sent), MSG_NOSIGNAL);
		if (n < 0)
			return fail("send", strerror(errno));
		sent += (int)n;
	}

	static unsigned char buf[16384];
	char head[MAX_HEAD + 1];
	size_t hn = 0;
	int in_body = 0, done = 0, status = 0;
	for (;;) {
		ssize_t n = recv(fd, buf, sizeof buf, 0);
		if (n < 0)
			return fail("recv", strerror(errno));
		if (n == 0)
			break;
		const unsigned char *p = buf;
		size_t m = (size_t)n;
		if (!in_body) {
			size_t take = m < MAX_HEAD - hn ? m : MAX_HEAD - hn;
			memcpy(head + hn, p, take);
			size_t from = hn > 3 ? hn - 3 : 0;
			hn += take;
			head[hn] = 0;
			char *end = NULL;
			for (size_t i = from; i + 3 < hn; i++)
				if (memcmp(head + i, "\r\n\r\n", 4) == 0) {
					end = head + i;
					break;
				}
			if (!end) {
				if (hn >= MAX_HEAD)
					return fail(host, "the response head is too long");
				continue;
			}
			size_t used = (size_t)(end + 4 - head) - (hn - take);
			p += used;
			m -= used;
			*end = 0;
			if (strlen(head) < 12 || strncmp(head, "HTTP/1.", 7) != 0)
				return fail(host, "not an HTTP response");
			status = atoi(head + 9);
			for (char *l = strstr(head, "\r\n"); l; l = strstr(l + 2, "\r\n")) {
				const char *v;
				if (header(l + 2, "content-length:", &v)) {
					mode = LENGTH;
					left = strtoull(v, NULL, 10);
				} else if (header(l + 2, "transfer-encoding:", &v)) {
					mode = CHUNKED;
				}
			}
			in_body = 1;
			if (status < 200 || status > 299) {
				char text[16];
				snprintf(text, sizeof text, "HTTP %d", status);
				return fail(url, text);
			}
			if (mode == LENGTH && left == 0)
				done = 1;
		}
		if (!done && m)
			done = feed(p, m);
		if (done || bad)
			break;
	}
	close(fd);
	if (bad)
		return fail(url, "the response is malformed");
	if (!in_body)
		return fail(url, "the connection closed before a response");
	if ((mode == LENGTH && left) || (mode == CHUNKED && chunk != C_DONE))
		return fail(url, "the connection closed before the end of the page");
	if (sum_only) {
		uint64_t us = now_us() - started;
		printf("%llu bytes fnv1a %016llx in %llu.%03llu ms\n",
		       (unsigned long long)size, (unsigned long long)hash,
		       (unsigned long long)(us / 1000), (unsigned long long)(us % 1000));
	}
	return 0;
}
