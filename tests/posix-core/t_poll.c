/*
 * poll and ppoll through libposix (ADR-0342 item 5, ADR-0414 item 1; backlog
 * "Three requests from outside work", item 2): a static C program built by zcc
 * against libc.a, run by posixrun. One binary with named cases; each prints
 * "t_poll: NAME passed" or a line ending in FAIL and exits 1. posixrun has no
 * broker and no network daemon, so the cases are pipes, files, /dev/null,
 * a socket with no connection and, from threads, the deferred wait of
 * ADR-0315. Sockets with a connection are in crates/zoraya-posix/tests/net.rs
 * (the fake netd); a socket pair or a loopback does not exist yet (ADR-0412).
 *
 *   t_poll pipe      empty, with bytes, at end of file, the write end of a
 *                    pipe whose reader closed; POLLIN at EOF; "norm" bits
 *   t_poll select    pselect says what poll says for the same pipe states
 *                    (INV-0342-2)
 *   t_poll files     a file, a directory, /dev/null: ready at once
 *   t_poll socket    a socket with no connection: ready at once, as select
 *   t_poll nval      a closed fd is POLLNVAL, a negative one is skipped,
 *                    the return counts both kinds as the standard says
 *   t_poll timeout   0, a few milliseconds, no descriptors, ppoll's timespec,
 *                    its invalid values
 *   t_poll hostile   nfds too large, NULL arrays, events nobody defined
 *   t_poll mask      ppoll's signal mask: a blocked signal does not wake the
 *                    wait and stays pending; an unblocked one is EINTR
 *   t_poll threads   from a thread: a sibling's write wakes poll and ppoll,
 *                    the timeout, a signal is EINTR, and 300 ping-pong rounds
 *                    lose no wake-up (the deferred wait, ADR-0315)
 *   t_poll all       every case but threads
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_poll: line %d: %s (errno %d) FAIL\n",         \
			       __LINE__, #cond, errno);                         \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define OK(call) CHECK((call) == 0)

/* A call that must fail with `e`. */
#define FAILS(call, e) CHECK((call) == -1 && errno == (e))

static double now(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static void nap(int ms)
{
	struct timespec t = {ms / 1000, (ms % 1000) * 1000000L};
	nanosleep(&t, NULL);
}

/* revents of one descriptor, looked at without waiting. */
static int rev(int fd, short events)
{
	struct pollfd f = {fd, events, 0x7000};
	int n = poll(&f, 1, 0);
	CHECK(n == (f.revents != 0 ? 1 : 0));
	return f.revents;
}

static void pipe_case(void)
{
	int p[2];
	char b[8];
	OK(pipe(p));
	CHECK(rev(p[0], POLLIN) == 0);
	CHECK(rev(p[1], POLLOUT) == POLLOUT);
	CHECK(rev(p[1], POLLOUT | POLLWRNORM) == (POLLOUT | POLLWRNORM));
	CHECK(rev(p[0], POLLPRI) == 0);
	CHECK(rev(p[0], 0) == 0);
	CHECK(write(p[1], "ab", 2) == 2);
	CHECK(rev(p[0], POLLIN) == POLLIN);
	CHECK(rev(p[0], POLLRDNORM) == POLLRDNORM);
	CHECK(read(p[0], b, 1) == 1);
	CHECK(rev(p[0], POLLIN) == POLLIN); /* one byte left */
	CHECK(read(p[0], b, 8) == 1);
	CHECK(rev(p[0], POLLIN) == 0);
	/* The writer goes: the bytes first, then the end of file, both with the
	 * hang-up; a read at the end of file does not block, so POLLIN. */
	CHECK(write(p[1], "z", 1) == 1);
	OK(close(p[1]));
	CHECK(rev(p[0], POLLIN) == (POLLIN | POLLHUP));
	CHECK(read(p[0], b, 8) == 1);
	CHECK(rev(p[0], POLLIN) == (POLLIN | POLLHUP));
	CHECK(read(p[0], b, 8) == 0);
	CHECK(rev(p[0], 0) == POLLHUP);
	OK(close(p[0]));

	/* A write end whose reader has gone: an error, and a write fails at once. */
	OK(pipe(p));
	OK(close(p[0]));
	CHECK(rev(p[1], POLLOUT) == (POLLOUT | POLLERR));
	CHECK(rev(p[1], 0) == POLLERR);
	OK(close(p[1]));

	/* A full pipe is not writable until it is read from (non-blocking). */
	OK(pipe(p));
	OK(fcntl(p[1], F_SETFL, O_NONBLOCK));
	OK(fcntl(p[0], F_SETFL, O_NONBLOCK));
	char *big = malloc(1 << 16);
	CHECK(big);
	memset(big, 1, 1 << 16);
	long total = 0;
	for (;;) {
		ssize_t w = write(p[1], big, 1 << 16);
		if (w < 0) {
			CHECK(errno == EAGAIN);
			break;
		}
		total += w;
		CHECK(total < (1 << 24));
	}
	CHECK(rev(p[1], POLLOUT) == 0);
	CHECK(rev(p[0], POLLIN) == POLLIN);
	while (read(p[0], big, 1 << 16) > 0)
		;
	CHECK(rev(p[1], POLLOUT) == POLLOUT);
	free(big);
	OK(close(p[0]));
	OK(close(p[1]));
	printf("t_poll: pipe passed\n");
}

/* The same state through pselect and through poll (INV-0342-2). */
static void same(int fd, int writing)
{
	fd_set s;
	FD_ZERO(&s);
	FD_SET(fd, &s);
	struct timespec z = {0, 0};
	int n = pselect(fd + 1, writing ? NULL : &s, writing ? &s : NULL, NULL, &z, NULL);
	CHECK(n >= 0);
	short bits = writing ? POLLOUT : POLLIN;
	CHECK((rev(fd, bits) & bits ? 1 : 0) == n);
}

static void select_case(void)
{
	int p[2];
	char b[4];
	OK(pipe(p));
	same(p[0], 0);
	same(p[1], 1);
	CHECK(write(p[1], "q", 1) == 1);
	same(p[0], 0);
	same(p[1], 1);
	CHECK(read(p[0], b, 4) == 1);
	same(p[0], 0);
	OK(close(p[1]));
	same(p[0], 0); /* end of file */
	CHECK(read(p[0], b, 4) == 0);
	same(p[0], 0);
	OK(close(p[0]));
	OK(pipe(p));
	OK(close(p[0]));
	same(p[1], 1); /* EPIPE at once */
	OK(close(p[1]));
	int f = open("/dev/null", O_RDWR);
	CHECK(f >= 0);
	same(f, 0);
	same(f, 1);
	OK(close(f));
	printf("t_poll: select passed\n");
}

static void files(void)
{
	int f = open("/work/poll.tmp", O_RDWR | O_CREAT | O_TRUNC, 0644);
	CHECK(f >= 0);
	int d = open("/work", O_RDONLY);
	CHECK(d >= 0);
	int n = open("/dev/null", O_RDWR);
	CHECK(n >= 0);
	int fds[3] = {f, d, n};
	for (int i = 0; i < 3; i++) {
		CHECK(rev(fds[i], POLLIN | POLLOUT) == (POLLIN | POLLOUT));
		CHECK(rev(fds[i], POLLIN) == POLLIN);
		CHECK(rev(fds[i], POLLOUT) == POLLOUT);
		CHECK(rev(fds[i], 0) == 0);
		/* Never waits, whatever the timeout. */
		struct pollfd p = {fds[i], POLLIN, 0};
		double t = now();
		CHECK(poll(&p, 1, -1) == 1);
		CHECK(now() - t < 2000);
	}
	OK(close(f));
	OK(close(d));
	OK(close(n));
	OK(unlink("/work/poll.tmp"));
	printf("t_poll: files passed\n");
}

static void socket_case(void)
{
	int s = socket(AF_INET, SOCK_STREAM, 0);
	CHECK(s >= 0);
	/* Nothing to wait for on a socket that has no connection: ready at
	 * once, as select has it. */
	CHECK((rev(s, POLLOUT) & POLLOUT) != 0);
	same(s, 1);
	OK(close(s));
	printf("t_poll: socket passed\n");
}

static void nval(void)
{
	int p[2];
	OK(pipe(p));
	int dead = p[1];
	OK(close(p[1]));
	struct pollfd f[4] = {
		{dead, POLLIN, 0x55},
		{-1, POLLIN, 0x55},
		{p[0], POLLIN, 0x55},
		{1000, 0, 0x55},
	};
	/* The pipe's write end is closed: p[0] has a hang-up and POLLIN. */
	f[0].revents = f[1].revents = f[2].revents = f[3].revents = 0x55;
	int n = poll(f, 4, 0);
	CHECK(n == 3);
	CHECK(f[0].revents == POLLNVAL);
	CHECK(f[1].revents == 0);
	CHECK(f[2].revents == (POLLIN | POLLHUP));
	CHECK(f[3].revents == POLLNVAL);
	/* POLLNVAL is not a wait, and is not an error of the call. */
	double t = now();
	struct pollfd one = {dead, POLLIN, 0};
	CHECK(poll(&one, 1, -1) == 1 && one.revents == POLLNVAL);
	CHECK(now() - t < 2000);
	/* Only negative entries: nothing is ready. */
	struct pollfd neg[2] = {{-5, POLLIN, 0x66}, {-1, POLLOUT, 0x66}};
	CHECK(poll(neg, 2, 0) == 0);
	CHECK(neg[0].revents == 0 && neg[1].revents == 0);
	OK(close(p[0]));
	printf("t_poll: nval passed\n");
}

static void timeout(void)
{
	int p[2];
	OK(pipe(p));
	struct pollfd f = {p[0], POLLIN, 0};
	double t = now();
	CHECK(poll(&f, 1, 0) == 0);
	CHECK(now() - t < 1000);
	t = now();
	CHECK(poll(&f, 1, 60) == 0);
	double e = now() - t;
	CHECK(e >= 50 && e < 3000);
	/* No descriptors: a sleep. */
	t = now();
	CHECK(poll(NULL, 0, 40) == 0);
	e = now() - t;
	CHECK(e >= 30 && e < 3000);
	/* ppoll's timespec. */
	struct timespec ts = {0, 50 * 1000000L};
	t = now();
	CHECK(ppoll(&f, 1, &ts, NULL) == 0);
	e = now() - t;
	CHECK(e >= 40 && e < 3000);
	ts.tv_sec = 0;
	ts.tv_nsec = 0;
	CHECK(ppoll(&f, 1, &ts, NULL) == 0);
	CHECK(write(p[1], "x", 1) == 1);
	ts.tv_sec = 5;
	CHECK(ppoll(&f, 1, &ts, NULL) == 1 && f.revents == POLLIN);
	CHECK(ppoll(&f, 1, NULL, NULL) == 1 && f.revents == POLLIN);
	/* Invalid values are EINVAL and touch nothing. */
	f.revents = 0x33;
	ts.tv_sec = -1;
	ts.tv_nsec = 0;
	FAILS(ppoll(&f, 1, &ts, NULL), EINVAL);
	ts.tv_sec = 0;
	ts.tv_nsec = 1000000000L;
	FAILS(ppoll(&f, 1, &ts, NULL), EINVAL);
	ts.tv_nsec = -1;
	FAILS(ppoll(&f, 1, &ts, NULL), EINVAL);
	CHECK(f.revents == 0x33);
	/* A huge timeout is a long wait, not an overflow into the past. */
	ts.tv_sec = 0x7fffffffffffffffL;
	ts.tv_nsec = 999999999L;
	CHECK(read(p[0], &(char){0}, 1) == 1);
	f.revents = 0;
	CHECK(write(p[1], "y", 1) == 1);
	CHECK(ppoll(&f, 1, &ts, NULL) == 1 && f.revents == POLLIN);
	CHECK(poll(&f, 1, 0x7fffffff) == 1);
	OK(close(p[0]));
	OK(close(p[1]));
	printf("t_poll: timeout passed\n");
}

static void hostile(void)
{
	struct pollfd one = {0, 0, 0};
	struct pollfd many[512];
	for (int i = 0; i < 512; i++) {
		many[i].fd = -1;
		many[i].events = POLLIN;
		many[i].revents = 0x44;
	}
	FAILS(poll(many, (nfds_t)-1, 0), EINVAL);
	FAILS(poll(many, (nfds_t)1 << 32, 0), EINVAL);
	FAILS(poll(many, (nfds_t)(1UL << 32) + 1, 0), EINVAL);
	FAILS(poll(many, 513, 0), EINVAL);
	FAILS(poll(many, 1 << 20, -1), EINVAL);
	FAILS(poll(NULL, (nfds_t)-1, 0), EINVAL);
	FAILS(ppoll(many, (nfds_t)-1, NULL, NULL), EINVAL);
	FAILS(poll(NULL, 1, 0), EFAULT);
	FAILS(ppoll(NULL, 3, NULL, NULL), EFAULT);
	CHECK(many[0].revents == 0x44); /* a refused call writes nothing */
	CHECK(poll(many, 512, 0) == 0);
	CHECK(many[0].revents == 0 && many[511].revents == 0);
	CHECK(poll(NULL, 0, 0) == 0);
	CHECK(ppoll(NULL, 0, &(struct timespec){0, 0}, NULL) == 0);
	/* Bits nobody defined in events are ignored, and revents is not read. */
	one.fd = open("/dev/null", O_RDWR);
	CHECK(one.fd >= 0);
	one.events = (short)0x7fff & ~(POLLIN | POLLOUT | POLLRDNORM | POLLWRNORM);
	one.revents = (short)0x7fff;
	CHECK(poll(&one, 1, 0) == 0 && one.revents == 0);
	one.events = (short)0xffff;
	CHECK(poll(&one, 1, 0) == 1);
	CHECK(one.revents == (POLLIN | POLLOUT | POLLRDNORM | POLLWRNORM));
	/* 512 entries of one ready descriptor: all are reported. */
	for (int i = 0; i < 512; i++) {
		many[i].fd = one.fd;
		many[i].events = POLLIN;
	}
	CHECK(poll(many, 512, 0) == 512);
	OK(close(one.fd));
	printf("t_poll: hostile passed\n");
}

static volatile sig_atomic_t usr1;

static void on_usr1(int s)
{
	(void)s;
	usr1++;
}

static void mask(void)
{
	struct sigaction sa;
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = on_usr1;
	OK(sigaction(SIGUSR1, &sa, NULL));
	int p[2];
	OK(pipe(p));
	struct pollfd f = {p[0], POLLIN, 0};
	sigset_t blocked, none, old;
	sigemptyset(&blocked);
	sigaddset(&blocked, SIGUSR1);
	sigemptyset(&none);

	/* A blocked signal is pending, and ppoll with the same mask in force
	 * does not take it. */
	OK(sigprocmask(SIG_BLOCK, &blocked, &old));
	usr1 = 0;
	OK(raise(SIGUSR1));
	CHECK(usr1 == 0);
	struct timespec ts = {0, 30 * 1000000L};
	CHECK(ppoll(&f, 1, &ts, &blocked) == 0);
	CHECK(usr1 == 0);
	/* With the signal unblocked for the wait it is delivered: EINTR. */
	FAILS(ppoll(&f, 1, &ts, &none), EINTR);
	CHECK(usr1 == 1);
	/* The caller's mask is back after the call, in every outcome. */
	sigset_t cur;
	OK(sigprocmask(SIG_BLOCK, NULL, &cur));
	CHECK(sigismember(&cur, SIGUSR1) == 1);
	/* Ready data wins over nothing, and the mask is still restored. */
	CHECK(write(p[1], "x", 1) == 1);
	CHECK(ppoll(&f, 1, &ts, &none) == 1 && f.revents == POLLIN);
	OK(sigprocmask(SIG_BLOCK, NULL, &cur));
	CHECK(sigismember(&cur, SIGUSR1) == 1);
	CHECK(read(p[0], &(char){0}, 1) == 1);
	/* Plain poll takes the caller's mask: blocked, pending, not delivered. */
	OK(raise(SIGUSR1));
	CHECK(poll(&f, 1, 20) == 0);
	CHECK(usr1 == 1);
	OK(sigprocmask(SIG_SETMASK, &old, NULL));
	CHECK(usr1 == 2);
	OK(close(p[0]));
	OK(close(p[1]));
	printf("t_poll: mask passed\n");
}

/* ----------------------------------------------------------- threads */

struct wake {
	int fd;
	int ms;
	pthread_t target;
	int sig;
};

static void *writer(void *arg)
{
	struct wake *w = arg;
	nap(w->ms);
	CHECK(write(w->fd, "w", 1) == 1);
	return NULL;
}

static void *killer(void *arg)
{
	struct wake *w = arg;
	nap(w->ms);
	OK(pthread_kill(w->target, w->sig));
	return NULL;
}

struct pingpong {
	int in, out;
	int rounds;
	int ppoll_it;
};

static void *echo(void *arg)
{
	struct pingpong *pp = arg;
	for (int i = 0; i < pp->rounds; i++) {
		struct pollfd f = {pp->in, POLLIN, 0};
		int n = pp->ppoll_it ? ppoll(&f, 1, NULL, NULL) : poll(&f, 1, -1);
		CHECK(n == 1 && f.revents == POLLIN);
		char c;
		CHECK(read(pp->in, &c, 1) == 1);
		CHECK(c == (char)i);
		CHECK(write(pp->out, &c, 1) == 1);
	}
	return NULL;
}

static void threads(void)
{
	struct sigaction sa;
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = on_usr1;
	OK(sigaction(SIGUSR1, &sa, NULL));
	int p[2], q[2];
	OK(pipe(p));
	OK(pipe(q));
	pthread_t t;
	char b[4];

	/* A sibling's write wakes poll; with -1 and with a timeout that is longer. */
	struct wake w = {p[1], 100, 0, 0};
	OK(pthread_create(&t, NULL, writer, &w));
	struct pollfd f = {p[0], POLLIN, 0};
	double t0 = now();
	CHECK(poll(&f, 1, -1) == 1 && f.revents == POLLIN);
	double e = now() - t0;
	CHECK(e >= 60 && e < 5000);
	OK(pthread_join(t, NULL));
	CHECK(read(p[0], b, 4) == 1);

	/* The same through ppoll, from the second of two descriptors. */
	struct pollfd two[2] = {{p[0], POLLIN, 0}, {q[0], POLLIN, 0}};
	struct wake w2 = {q[1], 80, 0, 0};
	OK(pthread_create(&t, NULL, writer, &w2));
	struct timespec ts = {10, 0};
	CHECK(ppoll(two, 2, &ts, NULL) == 1);
	CHECK(two[0].revents == 0 && two[1].revents == POLLIN);
	OK(pthread_join(t, NULL));
	CHECK(read(q[0], b, 4) == 1);

	/* The deadline passes with the lock released and the process threaded. */
	t0 = now();
	CHECK(poll(two, 2, 50) == 0);
	e = now() - t0;
	CHECK(e >= 40 && e < 3000);
	t0 = now();
	CHECK(poll(NULL, 0, 30) == 0);
	CHECK(now() - t0 >= 20);

	/* A signal to this thread ends the wait: EINTR, the handler has run. */
	usr1 = 0;
	struct wake k = {0, 100, pthread_self(), SIGUSR1};
	OK(pthread_create(&t, NULL, killer, &k));
	t0 = now();
	FAILS(poll(two, 2, -1), EINTR);
	CHECK(usr1 == 1);
	CHECK(now() - t0 < 5000);
	OK(pthread_join(t, NULL));

	/* With the signal blocked and the wait's mask open (ppoll's reason to
	 * be): the same. A write that comes first is the answer instead. */
	sigset_t blocked, none, old;
	sigemptyset(&blocked);
	sigaddset(&blocked, SIGUSR1);
	sigemptyset(&none);
	OK(pthread_sigmask(SIG_BLOCK, &blocked, &old));
	usr1 = 0;
	OK(pthread_create(&t, NULL, killer, &k));
	FAILS(ppoll(two, 2, NULL, &none), EINTR);
	CHECK(usr1 == 1);
	OK(pthread_join(t, NULL));
	/* Still blocked outside the call; a kill while blocked waits for the
	 * mask, and the wait times out instead. */
	OK(pthread_create(&t, NULL, killer, &k));
	ts.tv_sec = 0;
	ts.tv_nsec = 300 * 1000000L;
	CHECK(ppoll(two, 2, &ts, &blocked) == 0);
	CHECK(usr1 == 1);
	OK(pthread_join(t, NULL));
	OK(pthread_sigmask(SIG_SETMASK, &old, NULL));
	CHECK(usr1 == 2);

	/* 300 rounds between two threads: every write wakes the other side's
	 * wait, none is lost between the check and the wait. */
	struct pingpong pp = {p[0], q[1], 300, 0};
	OK(pthread_create(&t, NULL, echo, &pp));
	for (int i = 0; i < 300; i++) {
		char c = (char)i;
		CHECK(write(p[1], &c, 1) == 1);
		struct pollfd g = {q[0], POLLIN, 0};
		CHECK((i % 2 ? ppoll(&g, 1, NULL, NULL) : poll(&g, 1, 10000)) == 1);
		CHECK(g.revents == POLLIN);
		CHECK(read(q[0], &c, 1) == 1 && c == (char)i);
	}
	OK(pthread_join(t, NULL));
	pp.ppoll_it = 1;
	OK(pthread_create(&t, NULL, echo, &pp));
	for (int i = 0; i < 300; i++) {
		char c = (char)i;
		CHECK(write(p[1], &c, 1) == 1);
		struct pollfd g = {q[0], POLLIN, 0};
		CHECK(poll(&g, 1, 10000) == 1);
		CHECK(read(q[0], &c, 1) == 1);
	}
	OK(pthread_join(t, NULL));
	OK(close(p[0]));
	OK(close(p[1]));
	OK(close(q[0]));
	OK(close(q[1]));
	printf("t_poll: threads passed\n");
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "all";
	int all = strcmp(mode, "all") == 0, known = 0;
#define CASE(name, fn)                                                     \
	if (all || strcmp(mode, name) == 0) {                              \
		known = 1;                                                 \
		fn();                                                      \
	}
	CASE("pipe", pipe_case)
	CASE("select", select_case)
	CASE("files", files)
	CASE("socket", socket_case)
	CASE("nval", nval)
	CASE("timeout", timeout)
	CASE("hostile", hostile)
	CASE("mask", mask)
	if (strcmp(mode, "threads") == 0) {
		known = 1;
		threads();
	}
	if (!known) {
		printf("t_poll: unknown case %s FAIL\n", mode);
		return 2;
	}
	return 0;
}
