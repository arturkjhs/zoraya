/*
 * tcptest: TCP listen, accept and connect over netd's loopback (plan part F7b,
 * proposed ADR-0441), with the refusals. Run by `make test-net-posix`:
 *
 *   +listen 9000-9009 +listen 50000-50010 +net 127.0.0.1 tcptest
 *
 * One program is the server and the client: the loopback needs a listen
 * rule for the port and a reach rule for 127.0.0.1, both on the line. Every
 * wait is a poll with a limit, so that a hang is a failure that says where.
 */
#include <errno.h>
#include <fcntl.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <pthread.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define LOW 9000
/* netd's queue per listener: the listener channel's messages (ADR-0441 item 5). */
#define PENDING 64
#define WAIT_MS 60000

static int failures;

#define CHECK(cond)                                                              \
	do {                                                                     \
		if (!(cond)) {                                                   \
			printf("tcptest: FAIL line %d: %s (errno %d)\n", __LINE__, #cond, errno); \
			failures++;                                              \
			return;                                                  \
		}                                                                \
	} while (0)

static struct sockaddr_in addr(const char *ip, int port)
{
	struct sockaddr_in a;
	memset(&a, 0, sizeof a);
	a.sin_family = AF_INET;
	a.sin_port = htons(port);
	a.sin_addr.s_addr = ip ? inet_addr(ip) : htonl(INADDR_ANY);
	return a;
}

static int listener(int port, int *err)
{
	int fd = socket(AF_INET, SOCK_STREAM, 0);
	struct sockaddr_in a = addr(NULL, port);
	int one = 1;
	if (fd < 0)
		return -1;
	setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
	if (bind(fd, (struct sockaddr *)&a, sizeof a) < 0 || listen(fd, 8) < 0) {
		*err = errno;
		close(fd);
		return -1;
	}
	return fd;
}

static int client(int port)
{
	int fd = socket(AF_INET, SOCK_STREAM, 0);
	struct sockaddr_in a = addr("127.0.0.1", port);
	if (fd < 0)
		return -1;
	if (connect(fd, (struct sockaddr *)&a, sizeof a) < 0) {
		int e = errno;
		close(fd);
		errno = e;
		return -1;
	}
	return fd;
}

/* Whether `fd` is readable within WAIT_MS. */
static int ready(int fd)
{
	struct pollfd p = {.fd = fd, .events = POLLIN};
	return poll(&p, 1, WAIT_MS) == 1 && (p.revents & (POLLIN | POLLHUP));
}

static int readall(int fd, unsigned char *buf, size_t n)
{
	size_t got = 0;
	while (got < n) {
		ssize_t r;
		if (!ready(fd))
			return -2;
		r = read(fd, buf + got, n - got);
		if (r < 0)
			return -1;
		if (r == 0)
			break;
		got += (size_t)r;
	}
	return (int)got;
}

static void test_basics(void)
{
	int err = 0, port = LOW;
	int l = listener(port, &err);
	struct sockaddr_in a, peer;
	socklen_t n = sizeof a;
	int c, s, v = 0;
	char buf[64];
	CHECK(l >= 0);
	CHECK(getsockname(l, (struct sockaddr *)&a, &n) == 0 && ntohs(a.sin_port) == port);
	n = sizeof v;
	CHECK(getsockopt(l, SOL_SOCKET, SO_ACCEPTCONN, &v, &n) == 0 && v == 1);
	/* Nothing pending: a non-blocking accept says so, poll says not readable. */
	fcntl(l, F_SETFL, fcntl(l, F_GETFL) | O_NONBLOCK);
	CHECK(accept(l, NULL, NULL) < 0 && errno == EAGAIN);
	{
		struct pollfd p = {.fd = l, .events = POLLIN};
		CHECK(poll(&p, 1, 0) == 0);
	}
	c = client(port);
	CHECK(c >= 0);
	CHECK(ready(l));
	n = sizeof peer;
	s = accept(l, (struct sockaddr *)&peer, &n);
	CHECK(s >= 0);
	CHECK(peer.sin_addr.s_addr == inet_addr("127.0.0.1"));
	CHECK(write(c, "ping", 4) == 4);
	CHECK(readall(s, (unsigned char *)buf, 4) == 4 && memcmp(buf, "ping", 4) == 0);
	CHECK(write(s, "pong!", 5) == 5);
	CHECK(readall(c, (unsigned char *)buf, 5) == 5 && memcmp(buf, "pong!", 5) == 0);
	n = sizeof a;
	CHECK(getsockname(s, (struct sockaddr *)&a, &n) == 0 && ntohs(a.sin_port) == port);
	n = sizeof a;
	CHECK(getpeername(c, (struct sockaddr *)&a, &n) == 0 && ntohs(a.sin_port) == port);
	/* shutdown(SHUT_WR) of the client is the server's end of file; the server
	 * can still answer, then closes: the client reads to the end. */
	CHECK(shutdown(c, SHUT_WR) == 0);
	CHECK(readall(s, (unsigned char *)buf, 4) == 0);
	CHECK(write(s, "bye", 3) == 3);
	close(s);
	CHECK(readall(c, (unsigned char *)buf, 8) == 3 && memcmp(buf, "bye", 3) == 0);
	close(c);
	close(l);
	printf("tcptest: basics ok\n");
}

static void test_refusals(void)
{
	int err = 0, l, c;
	/* No listen rule for this port: EACCES, and bind alone is nothing. */
	CHECK(listener(9100, &err) < 0 && err == EACCES);
	/* Nobody listens on a port the reach rule covers. */
	CHECK(client(LOW + 5) < 0 && errno == ECONNREFUSED);
	/* A loopback address the rule does not name. */
	{
		int fd = socket(AF_INET, SOCK_STREAM, 0);
		struct sockaddr_in a = addr("127.0.0.2", LOW);
		CHECK(connect(fd, (struct sockaddr *)&a, sizeof a) < 0 && errno == EACCES);
		close(fd);
	}
	/* The port in use, by this program; and the account's one listener. */
	l = listener(LOW + 1, &err);
	CHECK(l >= 0);
	{
		int l2 = listener(LOW + 1, &err);
		CHECK(l2 < 0 && err == EADDRINUSE);
	}
	{
		int l3 = listener(LOW + 2, &err);
		CHECK(l3 < 0 && err == ENOBUFS);
	}
	/* Closing frees the place for another port, and the port for another listener. */
	close(l);
	l = listener(LOW + 2, &err);
	CHECK(l >= 0);
	close(l);
	l = listener(LOW + 1, &err);
	CHECK(l >= 0);
	c = client(LOW + 1);
	CHECK(c >= 0);
	close(c);
	close(l);
	/* A bind of an address that is not the machine's, and of IPv6. */
	{
		int fd = socket(AF_INET, SOCK_STREAM, 0);
		struct sockaddr_in a = addr("10.0.2.15", LOW);
		CHECK(bind(fd, (struct sockaddr *)&a, sizeof a) < 0 && errno == EADDRNOTAVAIL);
		close(fd);
		CHECK(socket(AF_INET6, SOCK_STREAM, 0) < 0 && errno == EAFNOSUPPORT);
	}
	printf("tcptest: refusals ok\n");
}

static void test_port_zero(void)
{
	int err = 0;
	int l = listener(0, &err);
	struct sockaddr_in a;
	socklen_t n = sizeof a;
	int c, s;
	CHECK(l >= 0);
	CHECK(getsockname(l, (struct sockaddr *)&a, &n) == 0);
	CHECK(ntohs(a.sin_port) >= 50000 && ntohs(a.sin_port) <= 50010);
	c = client(ntohs(a.sin_port));
	CHECK(c >= 0);
	s = accept(l, NULL, NULL);
	CHECK(s >= 0);
	close(s);
	close(c);
	close(l);
	printf("tcptest: port zero ok\n");
}

/* The server thread echoes what it reads until the end of the stream, then
 * shuts its write side; the writer thread sends N bytes then shuts its own. */
struct bulk {
	int fd;
	const unsigned char *data;
	size_t n;
	int ok;
};

static void *echo_thread(void *arg)
{
	struct bulk *b = arg;
	unsigned char tmp[8192];
	b->ok = 1;
	for (;;) {
		ssize_t r = read(b->fd, tmp, sizeof tmp), off = 0;
		if (r < 0) {
			b->ok = 0;
			break;
		}
		if (r == 0)
			break;
		while (off < r) {
			ssize_t w = write(b->fd, tmp + off, (size_t)(r - off));
			if (w <= 0) {
				b->ok = 0;
				return NULL;
			}
			off += w;
		}
	}
	shutdown(b->fd, SHUT_WR);
	return NULL;
}

static void *write_thread(void *arg)
{
	struct bulk *b = arg;
	size_t sent = 0;
	b->ok = 1;
	while (sent < b->n) {
		ssize_t w = write(b->fd, b->data + sent, b->n - sent > 5000 ? 5000 : b->n - sent);
		if (w <= 0) {
			b->ok = 0;
			return NULL;
		}
		sent += (size_t)w;
	}
	shutdown(b->fd, SHUT_WR);
	return NULL;
}

static void test_bulk(void)
{
	enum { N = 700000 };
	int err = 0, port = LOW + 3;
	int l = listener(port, &err);
	int c, s;
	unsigned char *out = malloc(N), *in = malloc(N);
	struct bulk echo, writer;
	pthread_t te, tw;
	int got;
	CHECK(l >= 0 && out && in);
	for (size_t i = 0; i < N; i++)
		out[i] = (unsigned char)(i * 7 + i / 251);
	c = client(port);
	CHECK(c >= 0);
	s = accept(l, NULL, NULL);
	CHECK(s >= 0);
	echo = (struct bulk){.fd = s};
	writer = (struct bulk){.fd = c, .data = out, .n = N};
	CHECK(pthread_create(&te, NULL, echo_thread, &echo) == 0);
	CHECK(pthread_create(&tw, NULL, write_thread, &writer) == 0);
	got = readall(c, in, N);
	pthread_join(tw, NULL);
	pthread_join(te, NULL);
	CHECK(echo.ok && writer.ok);
	CHECK(got == N);
	CHECK(memcmp(in, out, N) == 0);
	close(s);
	close(c);
	close(l);
	free(out);
	free(in);
	printf("tcptest: bulk ok\n");
}

/* The queue of a listener is netd's 64 messages: the 65th connect is refused,
 * and one accept makes room for one more. */
static void test_backlog_bound(void)
{
	int err = 0, port = LOW + 4;
	int l = listener(port, &err);
	int fds[PENDING + 1], made = 0, c, s;
	CHECK(l >= 0);
	for (int i = 0; i < PENDING; i++) {
		fds[i] = client(port);
		CHECK(fds[i] >= 0);
		made++;
	}
	/* Exactly at the bound. */
	c = client(port);
	CHECK(c < 0 && errno == ECONNREFUSED);
	s = accept(l, NULL, NULL);
	CHECK(s >= 0);
	close(s);
	fds[made] = client(port);
	CHECK(fds[made] >= 0);
	made++;
	/* Closing the listener with PENDING connections still queued: every
	 * client sees the end of its stream, not a hang. */
	close(l);
	for (int i = 0; i < made; i++) {
		char b[4];
		if (i == 0)
			continue; /* the accepted one's peer was closed above: also an end */
		CHECK(ready(fds[i]));
		CHECK(read(fds[i], b, sizeof b) == 0);
	}
	for (int i = 0; i < made; i++)
		close(fds[i]);
	printf("tcptest: backlog bound and a closed listener with pending connections ok\n");
}

/* The refusal itself, for the lines that give the program less: with no
 * listen rule `listen` must be EACCES (INV-0440-1); with no reach rule for
 * 127.0.0.1 a connect must be EACCES even though the listener works
 * (INV-0440-2). Each prints its own ok line, which the script waits for. */
static void test_expect_no_listen(void)
{
	int err = 0;
	CHECK(listener(LOW, &err) < 0 && err == EACCES);
	/* A connect to a port nobody listens on is refused by the loopback
	 * (ECONNREFUSED, the line has the reach rule), not by the grant: it
	 * fails if the line had no netd connection at all (EACCES). */
	CHECK(client(LOW) < 0 && errno == ECONNREFUSED);
	printf("tcptest: no listen grant ok\n");
}

static void test_expect_no_reach(void)
{
	int err = 0, l = listener(LOW, &err);
	int fd = socket(AF_INET, SOCK_STREAM, 0);
	struct sockaddr_in a = addr("127.0.0.1", LOW);
	CHECK(l >= 0);
	CHECK(connect(fd, (struct sockaddr *)&a, sizeof a) < 0 && errno == EACCES);
	close(fd);
	close(l);
	printf("tcptest: no reach grant ok\n");
}

int main(int argc, char **argv)
{
	if (argc > 1 && (strcmp(argv[1], "--no-listen") == 0 || strcmp(argv[1], "-L") == 0)) {
		test_expect_no_listen();
		return failures ? 1 : 0;
	}
	if (argc > 1 && (strcmp(argv[1], "--no-reach") == 0 || strcmp(argv[1], "-R") == 0)) {
		test_expect_no_reach();
		return failures ? 1 : 0;
	}
	test_basics();
	test_refusals();
	test_port_zero();
	test_bulk();
	test_backlog_bound();
	if (failures) {
		printf("tcptest: %d FAILED\n", failures);
		return 1;
	}
	printf("tcptest: all ok\n");
	return 0;
}
