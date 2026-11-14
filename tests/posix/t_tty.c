/*
 * The terminal through libposix (ADR-0331, ADR-0342 items 4 and 5, ADR-0389):
 * a static C program built by zcc against libc.a, run from zosh (so fds 0 to 2
 * are a terminal, in the window or on the serial console). One binary with
 * named modes; each prints "t_tty: NAME passed" or a line ending in FAIL.
 *
 *   t_tty names   ttyname, ttyname_r, ctermid, open("/dev/tty"), tcgetwinsize,
 *                 tcsetwinsize and TIOCSWINSZ (EPERM), TIOCSTI (ENOTTY),
 *                 tcsetattr's three actions and a bad one, tcflush, tcdrain,
 *                 tcflow, tcsendbreak, cfmakeraw and the speeds; the same
 *                 calls on a pipe say ENOTTY
 *   t_tty poll    raw mode, then poll and select with a 200 ms timeout on an
 *                 idle terminal (INV-0331-3: the elapsed time), then poll on
 *                 the terminal and a pipe nobody writes: a key wakes it with
 *                 the terminal only
 *   t_tty box     a full-screen program for the window (ADR-0331 items 7 and
 *                 9, ADR-0332): the alternate screen, a hidden cursor, a box
 *                 on the window's border that follows its size (SIGWINCH,
 *                 TIOCGWINSZ), the bytes of each key read, bracketed paste;
 *                 Ctrl-Z and Ctrl-C are the shell's (ISIG stays on); q ends
 *   t_tty bench bytes MIB | repaint N | latency N
 *                 what the shell's copy of the output costs (ADR-0331 item
 *                 14); the result goes to fd 2
 *   t_tty lines   canonical reads, each line printed with its length, until
 *                 end of file or "q": what a paste to a program without
 *                 mode 2004 shows
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/select.h>
#include <sys/time.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

static int bad;

#define CHECK(cond)                                                          \
	do {                                                                 \
		if (!(cond)) {                                               \
			printf("t_tty: %s:%d: %s FAIL (errno %d)\n", __func__, \
			       __LINE__, #cond, errno);                      \
			bad++;                                               \
		}                                                            \
	} while (0)

/* A call that must fail with `e`. */
#define FAILS(call, e)                                                       \
	do {                                                                 \
		errno = 0;                                                   \
		int r_ = (call);                                             \
		CHECK(r_ == -1 && errno == (e));                             \
	} while (0)

static long now_ms(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static int names(void)
{
	struct termios t;
	struct winsize ws;
	char small[4], buf[64];
	int p[2];
	const char *n;

	/* fds 0 to 2 are the terminal: the shell made them so (ADR-0331 item 2). */
	CHECK(isatty(0) == 1);
	n = ttyname(0);
	CHECK(n && !strcmp(n, "/dev/tty"));
	CHECK(ttyname_r(0, small, sizeof small) == ERANGE);
	CHECK(ttyname_r(0, buf, sizeof buf) == 0 && !strcmp(buf, "/dev/tty"));
	CHECK(!strcmp(ctermid(NULL), "/dev/tty"));
	CHECK(ctermid(buf) == buf && !strcmp(buf, "/dev/tty"));
	CHECK(ttyname_r(99, buf, sizeof buf) == EBADF);

	/* /dev/tty is a terminal of its own. */
	int fd = open("/dev/tty", O_RDWR);
	CHECK(fd >= 0);
	if (fd >= 0) {
		CHECK(isatty(fd) == 1);
		n = ttyname(fd);
		CHECK(n && !strcmp(n, "/dev/tty"));
		CHECK(tcgetattr(fd, &t) == 0);
		CHECK(tcgetwinsize(fd, &ws) == 0 && ws.ws_row > 0 && ws.ws_col > 0);
		/* Only the window sets the size (ADR-0331 item 7). */
		FAILS(tcsetwinsize(fd, &ws), EPERM);
		FAILS(ioctl(fd, TIOCSWINSZ, &ws), EPERM);
		/* Input is never injected (ADR-0338 item 2). */
		char c = 'x';
		FAILS(ioctl(fd, TIOCSTI, &c), ENOTTY);
		FAILS(ioctl(0, TIOCSTI, &c), ENOTTY);
		/* tcsetattr's three actions and a bad one. */
		CHECK(tcsetattr(fd, TCSANOW, &t) == 0);
		CHECK(tcsetattr(fd, TCSADRAIN, &t) == 0);
		CHECK(tcsetattr(fd, TCSAFLUSH, &t) == 0);
		FAILS(tcsetattr(fd, 99, &t), EINVAL);
		CHECK(tcflush(fd, TCIFLUSH) == 0);
		CHECK(tcflush(fd, TCOFLUSH) == 0);
		CHECK(tcflush(fd, TCIOFLUSH) == 0);
		FAILS(tcflush(fd, 9), EINVAL);
		CHECK(tcdrain(fd) == 0);
		CHECK(tcflow(fd, TCOON) == 0);
		FAILS(tcflow(fd, 9), EINVAL);
		CHECK(tcsendbreak(fd, 0) == 0);
		/* It writes where the terminal's output goes. */
		static const char msg[] = "t_tty: written through /dev/tty\n";
		CHECK(write(fd, msg, sizeof msg - 1) == (ssize_t)sizeof msg - 1);
		close(fd);
	}

	/* Nothing of that is a pipe's. */
	CHECK(pipe(p) == 0);
	errno = 0;
	CHECK(isatty(p[0]) == 0 && errno == ENOTTY);
	errno = 0;
	CHECK(ttyname(p[0]) == NULL && errno == ENOTTY);
	CHECK(ttyname_r(p[0], buf, sizeof buf) == ENOTTY);
	FAILS(tcgetattr(p[0], &t), ENOTTY);
	FAILS(tcsetattr(p[0], TCSANOW, &t), ENOTTY);
	FAILS(tcflush(p[0], TCIFLUSH), ENOTTY);
	FAILS(tcdrain(p[0]), ENOTTY);
	FAILS(tcgetwinsize(p[0], &ws), ENOTTY);
	FAILS(tcgetpgrp(p[0]), ENOTTY);
	close(p[0]);
	close(p[1]);

	/* The speeds are stored; cfmakeraw is musl's. */
	memset(&t, 0, sizeof t);
	CHECK(cfsetospeed(&t, B9600) == 0 && cfgetospeed(&t) == B9600);
	CHECK(cfsetispeed(&t, B9600) == 0 && cfgetispeed(&t) == B9600);
	FAILS(cfsetospeed(&t, 0x20000), EINVAL);
	CHECK(cfgetospeed(&t) == B9600);
	t.c_lflag = ICANON | ECHO | ISIG | IEXTEN;
	t.c_iflag = ICRNL | IXON;
	t.c_oflag = OPOST;
	cfmakeraw(&t);
	CHECK(!(t.c_lflag & (ICANON | ECHO | ISIG | IEXTEN)));
	CHECK(!(t.c_iflag & (ICRNL | IXON)) && !(t.c_oflag & OPOST));
	CHECK((t.c_cflag & CSIZE) == CS8);
	CHECK(t.c_cc[VMIN] == 1 && t.c_cc[VTIME] == 0);

	if (bad)
		return 1;
	printf("t_tty: names passed\n");
	return 0;
}

/* A raw mode that keeps ISIG: what `less` and `vi` ask for. */
static void raw_keep_signals(struct termios *t)
{
	t->c_lflag &= ~(ICANON | ECHO | IEXTEN);
	t->c_iflag &= ~(ICRNL | IXON);
	t->c_cc[VMIN] = 1;
	t->c_cc[VTIME] = 0;
}

static int poll_mode(void)
{
	struct termios was, t;
	struct pollfd pf[2];
	int p[2];
	char c;

	CHECK(isatty(0));
	CHECK(tcgetattr(0, &was) == 0);
	t = was;
	cfmakeraw(&t);
	CHECK(tcsetattr(0, TCSAFLUSH, &t) == 0);

	/* INV-0331-3: an idle terminal is not ready, and the wait is the timeout. */
	pf[0].fd = 0;
	pf[0].events = POLLIN;
	long t0 = now_ms();
	int r = poll(pf, 1, 200);
	long ms = now_ms() - t0;
	if (r == 0 && ms >= 150 && ms < 3000)
		printf("poll: idle ok\n");
	else
		printf("poll: idle FAIL r=%d ms=%ld revents=%x\n", r, ms, pf[0].revents);
	bad += !(r == 0 && ms >= 150 && ms < 3000);

	fd_set rs;
	struct timeval tv = {0, 200000};
	FD_ZERO(&rs);
	FD_SET(0, &rs);
	t0 = now_ms();
	r = select(1, &rs, NULL, NULL, &tv);
	ms = now_ms() - t0;
	if (r == 0 && ms >= 150 && ms < 3000)
		printf("select: idle ok\n");
	else
		printf("select: idle FAIL r=%d ms=%ld\n", r, ms);
	bad += !(r == 0 && ms >= 150 && ms < 3000);

	/* The terminal and a pipe nobody writes: a key wakes the call with the
	 * terminal only, and the read does not block. */
	CHECK(pipe(p) == 0);
	pf[0].fd = 0;
	pf[0].events = POLLIN;
	pf[1].fd = p[0];
	pf[1].events = POLLIN;
	printf("poll: type a key\n");
	fflush(stdout);
	r = poll(pf, 2, 30000);
	if (r == 1 && (pf[0].revents & POLLIN) && !pf[1].revents) {
		CHECK(read(0, &c, 1) == 1);
		printf("poll: key %02x\n", (unsigned char)c);
	} else {
		printf("poll: wake FAIL r=%d revents %x %x\n", r, pf[0].revents,
		       pf[1].revents);
		bad++;
	}
	/* The same through select. */
	printf("select: type a key\n");
	fflush(stdout);
	FD_ZERO(&rs);
	FD_SET(0, &rs);
	FD_SET(p[0], &rs);
	tv.tv_sec = 30;
	tv.tv_usec = 0;
	r = select(p[0] + 1, &rs, NULL, NULL, &tv);
	if (r == 1 && FD_ISSET(0, &rs) && !FD_ISSET(p[0], &rs)) {
		CHECK(read(0, &c, 1) == 1);
		printf("select: key %02x\n", (unsigned char)c);
	} else {
		printf("select: wake FAIL r=%d\n", r);
		bad++;
	}
	CHECK(tcsetattr(0, TCSAFLUSH, &was) == 0);
	printf(bad ? "t_tty: poll FAIL\n" : "t_tty: poll passed\n");
	return bad != 0;
}

/* ------------------------------------------------------------ box */

static volatile sig_atomic_t winch_seen, winches;

static void on_winch(int sig)
{
	(void)sig;
	winch_seen = 1;
	winches++;
}

static void put(const char *s)
{
	size_t n = strlen(s);
	while (n) {
		ssize_t w = write(1, s, n);
		if (w <= 0)
			return;
		s += w;
		n -= (size_t)w;
	}
}

static int rows, cols;
static char lastkeys[128] = "-";
static char lastpaste[96] = "-";

static void getsize(void)
{
	struct winsize w;
	if (ioctl(1, TIOCGWINSZ, &w) == 0 && w.ws_row && w.ws_col) {
		rows = w.ws_row;
		cols = w.ws_col;
	}
}

/* The whole screen in one write: a box on the border, text inside. */
static void draw(void)
{
	static char out[96 * 1024];
	size_t n = 0;
	int r, c;

	getsize();
	if (rows < 4 || cols < 20 || (size_t)rows * (size_t)(cols + 16) > sizeof out - 4096) {
		put("\x1b[2Jwindow too small or large\n");
		return;
	}
	n += (size_t)snprintf(out + n, sizeof out - n, "\x1b[H\x1b[2J");
	for (r = 1; r <= rows; r++) {
		n += (size_t)snprintf(out + n, sizeof out - n, "\x1b[%d;1H", r);
		for (c = 1; c <= cols; c++) {
			char ch = ' ';
			if (r == 1 || r == rows)
				ch = (c == 1 || c == cols) ? '+' : '-';
			else if (c == 1 || c == cols)
				ch = '|';
			out[n++] = ch;
		}
	}
	/* The text, with the numbers a script can read: no digit of them is a
	 * guess about the display (a big one has fewer rows than 24). */
	n += (size_t)snprintf(out + n, sizeof out - n,
			      "\x1b[2;3Hbox cols %d rows %d", cols, rows);
	n += (size_t)snprintf(out + n, sizeof out - n, "\x1b[3;3Hwinch %d", (int)winches);
	n += (size_t)snprintf(out + n, sizeof out - n, "\x1b[4;3Hkeys %s", lastkeys);
	n += (size_t)snprintf(out + n, sizeof out - n, "\x1b[5;3Hpaste %s", lastpaste);
	write(1, out, n);
}

static void hex(char *to, size_t size, const unsigned char *b, ssize_t n)
{
	size_t at = 0;
	for (ssize_t i = 0; i < n && at + 4 < size; i++)
		at += (size_t)snprintf(to + at, size - at, i ? " %02x" : "%02x", b[i]);
	if (!n)
		snprintf(to, size, "-");
}

/* The end of a bracketed paste, `ESC [ 2 0 1 ~`, in the bytes of one read. */
static int has_end(const unsigned char *b, size_t n)
{
	for (size_t i = 0; i + 6 <= n; i++)
		if (!memcmp(b + i, "\x1b[201~", 6))
			return 1;
	return 0;
}

static int box(void)
{
	struct termios was, t;
	unsigned char buf[512];
	int in_paste = 0, paste_bytes = 0;

	if (!isatty(0) || !isatty(1)) {
		printf("t_tty: box needs a terminal\n");
		return 1;
	}
	tcgetattr(0, &was);
	t = was;
	raw_keep_signals(&t);
	struct sigaction sa;
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = on_winch;
	sigaction(SIGWINCH, &sa, NULL);
	/* The alternate screen, no cursor, bracketed paste. */
	put("\x1b[?1049h\x1b[?25l\x1b[?2004h");
	tcsetattr(0, TCSADRAIN, &t);
	draw();
	for (;;) {
		struct pollfd pf = {0, POLLIN, 0};
		int r = poll(&pf, 1, -1);
		if (r < 0 && errno == EINTR) {
			if (winch_seen) {
				winch_seen = 0;
				draw();
			}
			continue;
		}
		if (r <= 0)
			continue;
		ssize_t n = read(0, buf, sizeof buf);
		if (n <= 0)
			break;
		if (n == 1 && buf[0] == 'q')
			break;
		/* Bracketed paste: 200~ ... 201~ around text (ADR-0332). */
		if ((size_t)n >= 6 && !memcmp(buf, "\x1b[200~", 6)) {
			in_paste = 1;
			paste_bytes = 0;
		}
		if (in_paste) {
			paste_bytes += (int)n;
			if (has_end(buf, (size_t)n)) {
				in_paste = 0;
				snprintf(lastpaste, sizeof lastpaste, "bracketed %d bytes", paste_bytes - 12);
			}
		} else {
			hex(lastkeys, sizeof lastkeys, buf, n);
		}
		if (winch_seen) {
			winch_seen = 0;
		}
		draw();
	}
	put("\x1b[?2004l\x1b[?25h\x1b[?1049l");
	tcsetattr(0, TCSADRAIN, &was);
	printf("t_tty: box done (winch %d)\n", (int)winches);
	return 0;
}

static int lines(void)
{
	char line[512];
	printf("lines: ready\n");
	fflush(stdout);
	while (fgets(line, sizeof line, stdin)) {
		size_t n = strlen(line);
		if (n && line[n - 1] == '\n')
			line[--n] = 0;
		if (!strcmp(line, "q"))
			break;
		printf("lines: got %zu bytes\n", n);
		fflush(stdout);
	}
	printf("t_tty: lines done\n");
	return 0;
}


/* ------------------------------------------------------------ bench
 * What the shell's copy of a program's output costs (ADR-0331 item 14). The
 * result goes to fd 2, the program's output to fd 1: with fd 1 a pipe that
 * `cat` empties into /dev/null there is no terminal and no relay in the path
 * (the baseline); with fd 1 the terminal, every byte is read from the
 * program's stream by the shell and written on (to the window's terminal, or
 * to the serial console). */
static long cpu_ms(void)
{
	struct timespec ts;
	if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &ts) != 0)
		return -1;
	return ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static void report(const char *what, long bytes, long ms, long cpu)
{
	long rate = ms > 0 ? (long)((long long)bytes * 1000 / ms) : 0;
	fprintf(stderr, "bench: %s %ld bytes %ld ms %ld bytes/s cpu %ld ms\n",
		what, bytes, ms, rate, cpu);
}

/* MiB of 79-column lines, in writes of 4096 bytes. */
static int bench_bytes(long mib)
{
	static char line[4096];
	size_t n = 0;
	long total = mib * 1024 * 1024, done = 0;
	while (n + 80 <= sizeof line) {
		memset(line + n, 'a' + (int)(n / 80) % 26, 79);
		line[n + 79] = '\n';
		n += 80;
	}
	long t0 = now_ms(), c0 = cpu_ms();
	while (done < total) {
		ssize_t w = write(1, line, n);
		if (w <= 0)
			return 1;
		done += w;
	}
	report("bytes", done, now_ms() - t0, cpu_ms() - c0);
	return 0;
}

/* Full repaints of an 80 by 24 screen: home, then each row placed and
 * written, as curses does without its optimiser. */
static int bench_repaint(long count)
{
	static char screen[24 * 96 + 16];
	size_t n = 0;
	long t0, c0;
	for (int r = 1; r <= 24; r++) {
		n += (size_t)snprintf(screen + n, sizeof screen - n, "\x1b[%d;1H", r);
		memset(screen + n, 'A' + r, 80);
		n += 80;
	}
	t0 = now_ms();
	c0 = cpu_ms();
	for (long i = 0; i < count; i++)
		if (write(1, screen, n) != (ssize_t)n)
			return 1;
	report("repaint", (long)n * count, now_ms() - t0, cpu_ms() - c0);
	return 0;
}

/* One byte written and `tcdrain`ed: the time until the shell has shown it
 * (a terminal only); the mean and the worst in microseconds. */
static int bench_latency(long count)
{
	struct timespec a, b;
	long long sum = 0, worst = 0;
	if (!isatty(1)) {
		fprintf(stderr, "bench: latency needs the terminal on fd 1\n");
		return 1;
	}
	for (long i = 0; i < count; i++) {
		clock_gettime(CLOCK_MONOTONIC, &a);
		if (write(1, " ", 1) != 1 || tcdrain(1) != 0)
			return 1;
		clock_gettime(CLOCK_MONOTONIC, &b);
		long long us = (long long)(b.tv_sec - a.tv_sec) * 1000000 + (b.tv_nsec - a.tv_nsec) / 1000;
		sum += us;
		if (us > worst)
			worst = us;
	}
	fprintf(stderr, "bench: latency %ld writes mean %lld us worst %lld us\n", count,
		sum / count, worst);
	return 0;
}

int main(int argc, char **argv)
{
	if (argc < 2)
		return 2;
	if (!strcmp(argv[1], "names"))
		return names();
	if (!strcmp(argv[1], "poll"))
		return poll_mode();
	if (!strcmp(argv[1], "box"))
		return box();
	if (!strcmp(argv[1], "lines"))
		return lines();
	if (!strcmp(argv[1], "bench") && argc > 3) {
		long n = atol(argv[3]);
		if (!strcmp(argv[2], "bytes"))
			return bench_bytes(n);
		if (!strcmp(argv[2], "repaint"))
			return bench_repaint(n);
		if (!strcmp(argv[2], "latency"))
			return bench_latency(n);
	}
	return 2;
}
