/*
 * AF_UNIX sockets through libposix (ADR-0412, plan part F2), and the small
 * calls that came with them (pread, pwrite, usleep): a static C program built
 * by zcc against libc.a, run by posixrun. One binary with named cases; each
 * prints "t_unix: NAME passed" or a line ending in FAIL and exits 1. posixrun
 * has no broker, so there is no second process here: a descriptor is sent
 * from one socket of a pair and received from the other, in one process, on
 * the real kernel (the kernel half of INV-0412-1: duplicated stream ends,
 * moved channel ends, the all-or-nothing write). A second process needs a
 * spawn that hands a socket over (plan part F4); the host tests of
 * crates/zoraya-posix/tests/unix.rs cover two processes on a fake kernel.
 *
 *   t_unix pair      socketpair of each type: bytes, boundaries, the end,
 *                    EPIPE, truncation, the flags, getsockopt, fstat
 *   t_unix rights    SCM_RIGHTS with a file, a directory, both ends of a pipe,
 *                    a stream socket and a packet socket; each works when it
 *                    arrives; read-only stays read-only
 *   t_unix refused   what a send refuses, and that a refused send changes
 *                    nothing (fd count, the moved socket still ours)
 *   t_unix stream    descriptors come with the byte they were sent with
 *   t_unix poll      poll on pairs: POLLIN, POLLOUT, POLLHUP, a full queue
 *   t_unix block     a blocking recv wakes when the peer sends; a full queue
 *                    blocks the writer until the reader takes a message
 *   t_unix calls     pread, pwrite, usleep, mkdtemp
 *   t_unix memfd     a memory descriptor over SCM_RIGHTS (ADR-0413): the same
 *                    memory in the receiver, the size fixed by the send, a
 *                    sealed descriptor read-only in the receiver, no object
 *                    to send (INV-0413-1, INV-0413-2)
 *   t_unix file      socket files (plan part F7, proposed ADR-0439), run by zosh,
 *                    whose table has /tmp (the line's temporary directory) and
 *                    /bin read-only: bind, listen, connect and accept of a
 *                    stream and a packet socket, what each call refuses
 *                    (INV-0412-3: bind outside /tmp is EACCES), a second
 *                    program (this one, spawned) that connects through its own
 *                    table and talks, unlink, and that the fds come back
 *   t_unix all       every case but `file`, which needs the shell's table
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <spawn.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_unix: line %d: %s (errno %d) FAIL\n",         \
			       __LINE__, #cond, errno);                         \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define OK(call) CHECK((call) == 0)
#define FAILS(call, e) CHECK((call) == -1 && errno == (e))

/* How many descriptors are open: the first free number. */
static int first_free(void)
{
	int fd = dup(0);
	CHECK(fd >= 0);
	close(fd);
	return fd;
}

/* Sends `n` bytes of `data` with the descriptors `fds`, as one message. */
static ssize_t send_fds(int s, const void *data, size_t n, const int *fds, int count, int flags)
{
	struct iovec iov = {(void *)data, n};
	char control[CMSG_SPACE(sizeof(int) * 70)];
	struct msghdr m;
	memset(&m, 0, sizeof m);
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	if (count > 0) {
		memset(control, 0, sizeof control);
		m.msg_control = control;
		m.msg_controllen = CMSG_SPACE(sizeof(int) * count);
		struct cmsghdr *c = CMSG_FIRSTHDR(&m);
		c->cmsg_level = SOL_SOCKET;
		c->cmsg_type = SCM_RIGHTS;
		c->cmsg_len = CMSG_LEN(sizeof(int) * count);
		memcpy(CMSG_DATA(c), fds, sizeof(int) * count);
	}
	return sendmsg(s, &m, flags);
}

/* Receives up to `n` bytes and `room` descriptors: the count of descriptors in
 * `*got`, the message's flags in `*mflags`. */
static ssize_t recv_fds(int s, void *buf, size_t n, int *fds, int room, int *got, int *mflags, int flags)
{
	struct iovec iov = {buf, n};
	char control[CMSG_SPACE(sizeof(int) * 70)];
	struct msghdr m;
	memset(&m, 0, sizeof m);
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	m.msg_control = control;
	m.msg_controllen = CMSG_SPACE(sizeof(int) * room);
	ssize_t r = recvmsg(s, &m, flags);
	*got = 0;
	if (r < 0)
		return r;
	*mflags = m.msg_flags;
	/* At most one control message: the library gives all descriptors in one. */
	struct cmsghdr *c = CMSG_FIRSTHDR(&m);
	if (c && c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
		int k = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
		memcpy(fds + *got, CMSG_DATA(c), k * sizeof(int));
		*got += k;
	}
	return r;
}

static void pair_case(void)
{
	int sv[2], t;
	char b[64];
	int types[3] = {SOCK_STREAM, SOCK_SEQPACKET, SOCK_DGRAM};
	for (int i = 0; i < 3; i++) {
		OK(socketpair(AF_UNIX, types[i], 0, sv));
		socklen_t l = sizeof t;
		OK(getsockopt(sv[0], SOL_SOCKET, SO_TYPE, &t, &l));
		CHECK(t == types[i]);
		OK(getsockopt(sv[1], SOL_SOCKET, SO_DOMAIN, &t, &l));
		CHECK(t == AF_UNIX);
		struct stat st;
		OK(fstat(sv[0], &st));
		CHECK(S_ISSOCK(st.st_mode));
		CHECK(write(sv[0], "hello", 5) == 5);
		CHECK(send(sv[1], "back!", 5, 0) == 5);
		if (types[i] == SOCK_STREAM) {
			CHECK(read(sv[1], b, 3) == 3 && memcmp(b, "hel", 3) == 0);
			CHECK(read(sv[1], b, 9) == 2 && memcmp(b, "lo", 2) == 0);
		} else {
			/* One message each, and the rest of a long one is lost. */
			CHECK(read(sv[1], b, 3) == 3 && memcmp(b, "hel", 3) == 0);
			CHECK(send(sv[0], "x", 1, 0) == 1);
			CHECK(recv(sv[1], b, 9, MSG_DONTWAIT) == 1 && b[0] == 'x');
		}
		CHECK(recv(sv[0], b, 64, 0) == 5 && memcmp(b, "back!", 5) == 0);
		FAILS(recv(sv[0], b, 64, MSG_DONTWAIT), EAGAIN);
		/* The peer closes: the end, then EPIPE for a write. */
		OK(close(sv[1]));
		CHECK(read(sv[0], b, 64) == 0);
		signal(SIGPIPE, SIG_IGN);
		FAILS(write(sv[0], "x", 1), EPIPE);
		OK(close(sv[0]));
	}
	/* The flags and what is refused. */
	OK(socketpair(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0, sv));
	CHECK(fcntl(sv[0], F_GETFL) & O_NONBLOCK);
	CHECK(fcntl(sv[0], F_GETFD) & FD_CLOEXEC);
	FAILS(read(sv[0], b, 1), EAGAIN);
	OK(close(sv[0]));
	OK(close(sv[1]));
	FAILS(socketpair(AF_INET, SOCK_STREAM, 0, sv), EAFNOSUPPORT);
	FAILS(socketpair(AF_UNIX, SOCK_RAW, 0, sv), EPROTONOSUPPORT);
	/* Boundaries, and the size of one message. */
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv));
	CHECK(send(sv[0], "", 0, 0) == 0);
	CHECK(send(sv[0], "abc", 3, 0) == 3);
	CHECK(recv(sv[1], b, 10, 0) == 0);
	CHECK(recv(sv[1], b, 2, 0) == 2);
	FAILS(recv(sv[1], b, 2, MSG_DONTWAIT), EAGAIN);
	static char big[64 * 1024];
	FAILS(send(sv[0], big, sizeof big, 0), EMSGSIZE);
	CHECK(send(sv[0], big, sizeof big - 1, 0) == (ssize_t)sizeof big - 1);
	CHECK(recv(sv[1], big, sizeof big, 0) == (ssize_t)sizeof big - 1);
	/* A message longer than the buffer says so. */
	struct iovec iov = {b, 2};
	struct msghdr m = {0};
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	CHECK(send(sv[0], "truncated", 9, 0) == 9);
	CHECK(recvmsg(sv[1], &m, 0) == 2 && (m.msg_flags & MSG_TRUNC));
	OK(close(sv[0]));
	OK(close(sv[1]));
	printf("t_unix: pair passed\n");
}

static void rights_case(void)
{
	int sv[2], file, ro, dir, p[2], ss[2], ps[2], fds[8], got, mflags;
	char b[64];
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv));
	file = open("/work/unix.txt", O_CREAT | O_TRUNC | O_RDWR, 0644);
	CHECK(file >= 0);
	CHECK(write(file, "data", 4) == 4);
	ro = open("/work/unix.txt", O_RDONLY);
	CHECK(ro >= 0);
	dir = open("/work", O_RDONLY | O_DIRECTORY);
	CHECK(dir >= 0);
	OK(pipe(p));
	OK(socketpair(AF_UNIX, SOCK_STREAM, 0, ss));
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, ps));
	int send_list[7] = {file, ro, dir, p[0], p[1], ss[0], ps[0]};
	CHECK(send_fds(sv[0], "all", 3, send_list, 7, 0) == 3);
	/* The two sockets moved: their numbers are free. */
	FAILS(fcntl(ss[0], F_GETFD), EBADF);
	FAILS(fcntl(ps[0], F_GETFD), EBADF);
	CHECK(fcntl(file, F_GETFD) >= 0 && fcntl(p[1], F_GETFD) >= 0);
	CHECK(recv_fds(sv[1], b, sizeof b, fds, 7, &got, &mflags, MSG_CMSG_CLOEXEC) == 3);
	CHECK(got == 7 && !(mflags & MSG_CTRUNC));
	for (int i = 0; i < 7; i++)
		CHECK(fcntl(fds[i], F_GETFD) & FD_CLOEXEC);
	/* The file: the same bytes, writable; the read-only one is not. */
	CHECK(pread(fds[0], b, 4, 0) == 4 && memcmp(b, "data", 4) == 0);
	CHECK(pwrite(fds[0], "D", 1, 0) == 1);
	CHECK(pread(fds[1], b, 4, 0) == 4 && memcmp(b, "Data", 4) == 0);
	FAILS(write(fds[1], "x", 1), EBADF);
	/* The directory. */
	struct stat st;
	OK(fstat(fds[2], &st));
	CHECK(S_ISDIR(st.st_mode));
	/* The pipe ends are this pipe's. */
	CHECK(write(fds[4], "pipe", 4) == 4);
	CHECK(read(p[0], b, 8) == 4 && memcmp(b, "pipe", 4) == 0);
	CHECK(write(p[1], "back", 4) == 4);
	CHECK(read(fds[3], b, 8) == 4 && memcmp(b, "back", 4) == 0);
	/* The stream socket and the packet socket still talk to their peers. */
	CHECK(write(ss[1], "stream", 6) == 6);
	CHECK(read(fds[5], b, 8) == 6 && memcmp(b, "stream", 6) == 0);
	CHECK(write(fds[5], "ok", 2) == 2);
	CHECK(read(ss[1], b, 8) == 2);
	CHECK(send(ps[1], "packet", 6, 0) == 6);
	CHECK(recv(fds[6], b, 8, 0) == 6 && memcmp(b, "packet", 6) == 0);
	/* A received socket can carry descriptors again. */
	int again[1];
	CHECK(send_fds(fds[6], "again", 5, &p[0], 1, 0) == 5);
	CHECK(recv_fds(ps[1], b, sizeof b, again, 1, &got, &mflags, 0) == 5 && got == 1);
	CHECK(write(p[1], "q", 1) == 1);
	CHECK(read(again[0], b, 8) == 1 && b[0] == 'q');
	/* Close what is left. */
	OK(close(sv[0]));
	OK(close(sv[1]));
	OK(close(ps[1]));
	OK(close(file));
	OK(close(ro));
	OK(close(dir));
	OK(close(p[0]));
	OK(close(p[1]));
	OK(close(ss[1]));
	printf("t_unix: rights passed\n");
}

static void refused_case(void)
{
	int sv[2], p[2], fds[70], got, mflags;
	char b[16];
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv));
	OK(pipe(p));
	int tcp = socket(AF_INET, SOCK_STREAM, 0);
	int null = open("/dev/null", O_RDWR);
	CHECK(tcp >= 0 && null >= 0);
	int open_before = first_free();
	int bad[3];
	bad[0] = p[0];
	bad[1] = 99;
	FAILS(send_fds(sv[0], "x", 1, bad, 2, 0), EBADF);
	bad[1] = tcp;
	FAILS(send_fds(sv[0], "x", 1, bad, 2, 0), EINVAL);
	bad[1] = null;
	FAILS(send_fds(sv[0], "x", 1, bad, 2, 0), EINVAL);
	bad[1] = sv[1];
	FAILS(send_fds(sv[0], "x", 1, bad, 2, 0), EBUSY); /* its peer's channel */
	bad[1] = sv[0];
	FAILS(send_fds(sv[0], "x", 1, bad, 2, 0), EBUSY); /* its own */
	for (int i = 0; i < 70; i++)
		fds[i] = p[0];
	FAILS(send_fds(sv[0], "x", 1, fds, 65, 0), ETOOMANYREFS);
	/* Nothing was sent and no descriptor was made or lost. */
	FAILS(recv(sv[1], b, 16, MSG_DONTWAIT), EAGAIN);
	CHECK(first_free() == open_before);
	CHECK(fcntl(sv[0], F_GETFD) >= 0 && fcntl(sv[1], F_GETFD) >= 0);
	/* The same moving socket twice. */
	int two[3] = {sv[1], sv[1], 0};
	FAILS(send_fds(sv[0], "x", 1, two, 2, 0), EINVAL);
	/* A sent end with a handle queued at it cannot move (ADR-0050). */
	int q[2];
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, q));
	CHECK(send_fds(q[1], "m", 1, &p[0], 1, 0) == 1);
	FAILS(send_fds(sv[0], "x", 1, &q[0], 1, 0), EBUSY);
	CHECK(fcntl(q[0], F_GETFD) >= 0);
	CHECK(recv_fds(q[0], b, 16, fds, 1, &got, &mflags, 0) == 1 && got == 1);
	OK(close(fds[0]));
	CHECK(send_fds(sv[0], "x", 1, &q[0], 1, 0) == 1);
	CHECK(recv_fds(sv[1], b, 16, fds, 1, &got, &mflags, 0) == 1 && got == 1);
	OK(close(fds[0]));
	/* A peer that is gone: EPIPE, and the socket that would have moved stays. */
	OK(close(sv[1]));
	signal(SIGPIPE, SIG_IGN);
	int s[2];
	OK(socketpair(AF_UNIX, SOCK_STREAM, 0, s));
	FAILS(send_fds(sv[0], "x", 1, &s[0], 1, 0), EPIPE);
	CHECK(write(s[0], "alive", 5) == 5);
	CHECK(read(s[1], b, 16) == 5);
	/* A descriptor with no byte, and one past a fork's shared stream. */
	FAILS(send_fds(s[0], "", 0, &p[0], 1, 0), EINVAL);
	/* Control messages of another kind. */
	struct iovec iov = {"x", 1};
	char control[CMSG_SPACE(sizeof(int))];
	struct msghdr m = {0};
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	m.msg_control = control;
	m.msg_controllen = sizeof control;
	struct cmsghdr *c = CMSG_FIRSTHDR(&m);
	c->cmsg_level = SOL_SOCKET;
	c->cmsg_type = SCM_CREDENTIALS;
	c->cmsg_len = CMSG_LEN(sizeof(int));
	FAILS(sendmsg(s[0], &m, 0), EINVAL);
	FAILS(socket(AF_UNIX, SOCK_STREAM, 3), EPROTONOSUPPORT);
	/* A Unix socket with no peer. */
	int u = socket(AF_UNIX, SOCK_STREAM, 0);
	CHECK(u >= 0);
	FAILS(read(u, b, 1), ENOTCONN);
	OK(close(u));
	printf("t_unix: refused passed\n");
}

static void stream_case(void)
{
	int s[2], p[2], fds[4], got, mflags;
	char b[256];
	OK(socketpair(AF_UNIX, SOCK_STREAM, 0, s));
	OK(pipe(p));
	/* Three sends, the second and the third with a descriptor: a read of
	 * 100 bytes returns the first send alone, then the second with its
	 * descriptor, then the third with its own. */
	CHECK(send(s[0], "AAAA", 4, 0) == 4);
	int a = p[0];
	CHECK(send_fds(s[0], "BBBB", 4, &a, 1, 0) == 4);
	CHECK(send_fds(s[0], "CCCC", 4, &p[1], 1, 0) == 4);
	CHECK(recv_fds(s[1], b, 100, fds, 4, &got, &mflags, 0) == 4 && got == 0);
	CHECK(memcmp(b, "AAAA", 4) == 0);
	CHECK(recv_fds(s[1], b, 100, fds, 4, &got, &mflags, 0) == 4 && got == 1);
	CHECK(memcmp(b, "BBBB", 4) == 0);
	CHECK(write(p[1], "r", 1) == 1);
	CHECK(read(fds[0], b, 1) == 1 && b[0] == 'r');
	OK(close(fds[0]));
	CHECK(recv_fds(s[1], b, 100, fds, 4, &got, &mflags, 0) == 4 && got == 1);
	CHECK(memcmp(b, "CCCC", 4) == 0);
	OK(close(fds[0]));
	/* A plain read takes the descriptor's byte and closes the descriptor. */
	int before = first_free();
	CHECK(send_fds(s[0], "ZZ", 2, &p[0], 1, 0) == 2);
	CHECK(read(s[1], b, 2) == 2);
	CHECK(first_free() == before);
	/* A short control buffer: the descriptor is closed and the message says so. */
	CHECK(send_fds(s[0], "Y", 1, &p[0], 1, 0) == 1);
	CHECK(recv_fds(s[1], b, 1, fds, 0, &got, &mflags, 0) == 1);
	CHECK(got == 0 && (mflags & MSG_CTRUNC));
	CHECK(first_free() == before);
	/* A long stream, in uneven pieces, in order. */
	static char out[20000], in[20000];
	for (int i = 0; i < (int)sizeof out; i++)
		out[i] = (char)(i * 7 + i / 251);
	int sent = 0, taken = 0;
	while (taken < (int)sizeof out) {
		int n = 1 + (sent * 31) % 997;
		if (n > (int)sizeof out - sent)
			n = (int)sizeof out - sent;
		if (n > 0) {
			ssize_t w = send(s[0], out + sent, n, MSG_DONTWAIT);
			if (w > 0)
				sent += w;
		}
		ssize_t r = recv(s[1], in + taken, 1 + (taken * 17) % 613, MSG_DONTWAIT);
		if (r > 0)
			taken += r;
	}
	CHECK(memcmp(in, out, sizeof out) == 0);
	OK(close(s[0]));
	OK(close(s[1]));
	OK(close(p[0]));
	OK(close(p[1]));
	printf("t_unix: stream passed\n");
}

static int rev(int fd, short events)
{
	struct pollfd f = {fd, events, 0x7000};
	int n = poll(&f, 1, 0);
	CHECK(n == (f.revents != 0 ? 1 : 0));
	return f.revents;
}

static void poll_case(void)
{
	int types[2] = {SOCK_STREAM, SOCK_SEQPACKET};
	char b[8];
	for (int i = 0; i < 2; i++) {
		int s[2];
		OK(socketpair(AF_UNIX, types[i], 0, s));
		CHECK(rev(s[0], POLLIN) == 0);
		CHECK(rev(s[0], POLLOUT) == POLLOUT);
		CHECK(write(s[1], "x", 1) == 1);
		CHECK(rev(s[0], POLLIN) == POLLIN);
		CHECK(read(s[0], b, 8) == 1);
		CHECK(rev(s[0], POLLIN) == 0);
		OK(close(s[1]));
		CHECK((rev(s[0], POLLIN) & POLLHUP) == POLLHUP);
		CHECK(read(s[0], b, 8) == 0);
		OK(close(s[0]));
	}
	/* A full queue is not writable until a message is read. */
	int s[2];
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, s));
	int n = 0;
	while (send(s[0], "m", 1, MSG_DONTWAIT) == 1)
		n++;
	CHECK(errno == EAGAIN && n >= 1);
	CHECK(rev(s[0], POLLOUT) == 0);
	CHECK(recv(s[1], b, 8, 0) == 1);
	CHECK(rev(s[0], POLLOUT) == POLLOUT);
	OK(close(s[0]));
	OK(close(s[1]));
	printf("t_unix: poll passed\n");
}

static void *late_send(void *arg)
{
	int fd = *(int *)arg;
	struct timespec t = {0, 30 * 1000000L};
	nanosleep(&t, NULL);
	CHECK(send(fd, "late", 4, 0) == 4);
	return NULL;
}

static void *late_recv(void *arg)
{
	int fd = *(int *)arg;
	char b[8];
	struct timespec t = {0, 30 * 1000000L};
	nanosleep(&t, NULL);
	CHECK(recv(fd, b, 8, 0) == 1);
	return NULL;
}

static void block_case(void)
{
	int s[2];
	pthread_t t;
	char b[8];
	for (int i = 0; i < 2; i++) {
		OK(socketpair(AF_UNIX, i ? SOCK_SEQPACKET : SOCK_STREAM, 0, s));
		OK(pthread_create(&t, NULL, late_send, &s[1]));
		CHECK(recv(s[0], b, 8, 0) == 4 && memcmp(b, "late", 4) == 0);
		OK(pthread_join(t, NULL));
		OK(close(s[0]));
		OK(close(s[1]));
	}
	/* A writer blocked on a full queue goes on when a message is read. */
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, s));
	while (send(s[0], "m", 1, MSG_DONTWAIT) == 1)
		;
	CHECK(errno == EAGAIN);
	OK(pthread_create(&t, NULL, late_recv, &s[1]));
	CHECK(send(s[0], "m", 1, 0) == 1);
	OK(pthread_join(t, NULL));
	OK(close(s[0]));
	OK(close(s[1]));
	printf("t_unix: block passed\n");
}

static void calls_case(void)
{
	char b[16];
	int fd = open("/work/pread.txt", O_CREAT | O_TRUNC | O_RDWR, 0644);
	CHECK(fd >= 0);
	CHECK(write(fd, "0123456789", 10) == 10);
	CHECK(pread(fd, b, 4, 3) == 4 && memcmp(b, "3456", 4) == 0);
	CHECK(lseek(fd, 0, SEEK_CUR) == 10);
	CHECK(pwrite(fd, "XY", 2, 2) == 2);
	CHECK(lseek(fd, 0, SEEK_CUR) == 10);
	CHECK(pread(fd, b, 16, 0) == 10 && memcmp(b, "01XY456789", 10) == 0);
	CHECK(pread(fd, b, 16, 10) == 0);
	FAILS(pread(fd, b, 4, -1), EINVAL);
	int p[2];
	OK(pipe(p));
	FAILS(pread(p[0], b, 4, 0), ESPIPE);
	OK(close(p[0]));
	OK(close(p[1]));
	OK(close(fd));
	struct timespec a, c;
	clock_gettime(CLOCK_MONOTONIC, &a);
	OK(usleep(20000));
	clock_gettime(CLOCK_MONOTONIC, &c);
	long ms = (c.tv_sec - a.tv_sec) * 1000 + (c.tv_nsec - a.tv_nsec) / 1000000;
	CHECK(ms >= 19 && ms < 2000);
	OK(usleep(0));
	/* mkdtemp: a new directory of a random name, mode 0700. */
	char tmpl[] = "/work/dXXXXXX";
	char *d = mkdtemp(tmpl);
	CHECK(d == tmpl && strcmp(d, "/work/dXXXXXX") != 0);
	struct stat st;
	OK(stat(d, &st));
	CHECK(S_ISDIR(st.st_mode));
	OK(rmdir(d));
	char bad[] = "/work/dXXXXX";
	CHECK(mkdtemp(bad) == NULL && errno == EINVAL);
	printf("t_unix: calls passed\n");
}

static void memfd_case(void)
{
	int sv[2], fds[2], got, mflags, size = 1 << 20;
	char b[16];
	OK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv));
	int fd = memfd_create("t_unix", MFD_ALLOW_SEALING);
	CHECK(fd >= 0 && ftruncate(fd, size) == 0);
	char *w = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(w != MAP_FAILED);
	memcpy(w, "painted", 8);
	/* The sender keeps it, and its size is fixed by the send. */
	CHECK(send_fds(sv[0], "m", 1, &fd, 1, 0) == 1);
	FAILS(ftruncate(fd, 4096), EBUSY);
	CHECK(fcntl(fd, F_GETFD) >= 0);
	CHECK(recv_fds(sv[1], b, sizeof b, fds, 2, &got, &mflags, 0) == 1 && got == 1);
	struct stat st;
	CHECK(fstat(fds[0], &st) == 0 && st.st_size == size);
	char *rw = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fds[0], 0);
	CHECK(rw != MAP_FAILED && memcmp(rw, "painted", 8) == 0);
	/* The same memory: what the receiver writes, the sender sees. */
	memcpy(rw + 100, "reply", 6);
	CHECK(memcmp(w + 100, "reply", 6) == 0);
	FAILS(ftruncate(fds[0], 4096), EBUSY);
	/* Sealed against later writing, a descriptor arrives read-only. */
	CHECK(fcntl(fd, F_ADD_SEALS, F_SEAL_FUTURE_WRITE) == 0);
	CHECK(send_fds(sv[0], "m", 1, &fd, 1, 0) == 1);
	CHECK(recv_fds(sv[1], b, sizeof b, fds + 1, 1, &got, &mflags, 0) == 1 && got == 1);
	CHECK(mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fds[1], 0) == MAP_FAILED && errno == EACCES);
	char *ro = mmap(NULL, size, PROT_READ, MAP_SHARED, fds[1], 0);
	CHECK(ro != MAP_FAILED && memcmp(ro, "painted", 8) == 0 && memcmp(ro + 100, "reply", 6) == 0);
	FAILS(write(fds[1], "x", 1), EBADF);
	CHECK(mmap(NULL, size, PROT_READ | PROT_EXEC, MAP_SHARED, fds[1], 0) == MAP_FAILED && errno == EACCES);
	/* A descriptor with no object has nothing to send. */
	int empty = memfd_create("empty", 0);
	CHECK(empty >= 0);
	FAILS(send_fds(sv[0], "m", 1, &empty, 1, 0), EINVAL);
	CHECK(munmap(w, size) == 0 && munmap(rw, size) == 0 && munmap(ro, size) == 0);
	printf("t_unix: memfd passed\n");
}


/* ------------------------------------------------------------- socket files */

#define SOCK_PATH "/tmp/t_unix_sock"

static socklen_t un_addr(struct sockaddr_un *a, const char *path)
{
	memset(a, 0, sizeof *a);
	a->sun_family = AF_UNIX;
	strncpy(a->sun_path, path, sizeof a->sun_path - 1);
	return (socklen_t)(offsetof(struct sockaddr_un, sun_path) + strlen(path) + 1);
}

static int un_bind(int s, const char *path)
{
	struct sockaddr_un a;
	socklen_t n = un_addr(&a, path);
	return bind(s, (struct sockaddr *)&a, n);
}

static int un_connect(int s, const char *path)
{
	struct sockaddr_un a;
	socklen_t n = un_addr(&a, path);
	return connect(s, (struct sockaddr *)&a, n);
}

/* The second program: connects to argv[2] as a stream, says hello, reads the
 * answer, then does the same with a packet socket at argv[3]. */
static int file_child(const char *stream_path, const char *packet_path)
{
	char b[64];
	int c = socket(AF_UNIX, SOCK_STREAM, 0);
	CHECK(c >= 0);
	OK(un_connect(c, stream_path));
	CHECK(write(c, "hello from the child", 20) == 20);
	CHECK(read(c, b, sizeof b) == 9 && memcmp(b, "the reply", 9) == 0);
	CHECK(read(c, b, sizeof b) == 0);
	close(c);
	int p = socket(AF_UNIX, SOCK_SEQPACKET, 0);
	CHECK(p >= 0);
	OK(un_connect(p, packet_path));
	CHECK(send(p, "one", 3, 0) == 3 && send(p, "twotwo", 6, 0) == 6);
	close(p);
	return 0;
}

static void file_case(void)
{
	char b[64];
	struct sockaddr_un a;
	socklen_t len;
	int base = first_free();
	unlink(SOCK_PATH);
	unlink(SOCK_PATH "2");

	/* A stream socket bound in /tmp: a socket file, with its name. */
	int s = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	CHECK(s >= 0);
	OK(un_bind(s, SOCK_PATH));
	struct stat st;
	OK(stat(SOCK_PATH, &st));
	CHECK(S_ISSOCK(st.st_mode));
	len = sizeof a;
	OK(getsockname(s, (struct sockaddr *)&a, &len));
	CHECK(a.sun_family == AF_UNIX && strcmp(a.sun_path, SOCK_PATH) == 0);
	CHECK(len == offsetof(struct sockaddr_un, sun_path) + strlen(SOCK_PATH) + 1);

	/* What bind refuses (INV-0412-3), and a name taken once. */
	int t = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	FAILS(un_bind(t, SOCK_PATH), EADDRINUSE);
	FAILS(un_bind(s, SOCK_PATH "2"), EINVAL);
	FAILS(un_bind(t, "/bin/t_unix_sock"), EACCES);
	FAILS(un_bind(t, "/tmp"), EINVAL);
	FAILS(un_bind(t, "/nowhere/x"), ENOENT);
	FAILS(un_bind(t, "/tmp/missing/x"), ENOENT);
	CHECK(access("/bin/t_unix_sock", F_OK) == -1);
	/* An abstract name, a datagram socket: not offered. */
	struct sockaddr_un ab;
	memset(&ab, 0, sizeof ab);
	ab.sun_family = AF_UNIX;
	memcpy(ab.sun_path + 1, "abc", 3);
	FAILS(bind(t, (struct sockaddr *)&ab, offsetof(struct sockaddr_un, sun_path) + 4), EAFNOSUPPORT);
	int d = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
	FAILS(un_bind(d, SOCK_PATH "d"), EOPNOTSUPP);
	close(d);
	/* A socket file is connected to, not opened. */
	FAILS(open(SOCK_PATH, O_RDWR), ENXIO);

	/* Bound, not listening: refused; no listener to accept on. */
	int c = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	FAILS(un_connect(c, SOCK_PATH), ECONNREFUSED);
	FAILS(accept(s, NULL, NULL), EINVAL);
	FAILS(listen(t, 5), EINVAL);
	FAILS(un_connect(c, "/tmp/t_unix_none"), ENOENT);
	FAILS(un_connect(c, "/bin/t_unix"), EACCES);
	OK(listen(s, 5));
	int on;
	socklen_t onlen = sizeof on;
	OK(getsockopt(s, SOL_SOCKET, SO_ACCEPTCONN, &on, &onlen));
	CHECK(on == 1);

	/* Connect and accept, in one process: both ends talk. */
	OK(un_connect(c, SOCK_PATH));
	FAILS(un_connect(c, SOCK_PATH), EISCONN);
	struct pollfd pf = {s, POLLIN, 0};
	CHECK(poll(&pf, 1, 1000) == 1 && (pf.revents & POLLIN));
	struct sockaddr_un peer;
	len = sizeof peer;
	int acc = accept(s, (struct sockaddr *)&peer, &len);
	CHECK(acc >= 0 && peer.sun_family == AF_UNIX);
	len = sizeof a;
	OK(getsockname(acc, (struct sockaddr *)&a, &len));
	CHECK(strcmp(a.sun_path, SOCK_PATH) == 0);
	len = sizeof a;
	OK(getpeername(c, (struct sockaddr *)&a, &len));
	CHECK(strcmp(a.sun_path, SOCK_PATH) == 0);
	CHECK(write(c, "ping", 4) == 4 && read(acc, b, sizeof b) == 4 && memcmp(b, "ping", 4) == 0);
	CHECK(write(acc, "pong", 4) == 4 && read(c, b, sizeof b) == 4 && memcmp(b, "pong", 4) == 0);
	/* A descriptor crosses the connection. */
	int pp[2];
	OK(pipe(pp));
	CHECK(write(pp[1], "via pipe", 8) == 8);
	CHECK(send_fds(c, "!", 1, &pp[0], 1, 0) == 1);
	int got[1], ng = 0, mf = 0;
	errno = 0;
	ssize_t rr = recv_fds(acc, b, sizeof b, got, 1, &ng, &mf, 0);
	if (rr != 1 || ng != 1 || mf != 0)
		printf("t_unix: recv_fds gave %zd, %d descriptors, flags %d, errno %d\n", rr, ng, mf, errno);
	CHECK(rr == 1 && ng == 1 && mf == 0);
	CHECK(read(got[0], b, sizeof b) == 8 && memcmp(b, "via pipe", 8) == 0);
	close(got[0]);
	close(pp[0]);
	close(pp[1]);
	/* A non-blocking listener with nothing queued: EAGAIN, and back. */
	int fl = fcntl(s, F_GETFL);
	CHECK(fl >= 0 && fcntl(s, F_SETFL, fl | O_NONBLOCK) == 0);
	FAILS(accept4(s, NULL, NULL, SOCK_CLOEXEC), EAGAIN);
	CHECK(fcntl(s, F_SETFL, fl) == 0);
	close(c);
	CHECK(read(acc, b, sizeof b) == 0);
	close(acc);

	/* A packet listener, and a stream connect to it is the wrong type. */
	int ps = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
	OK(un_bind(ps, SOCK_PATH "2"));
	OK(listen(ps, 5));
	int wrong = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	FAILS(un_connect(wrong, SOCK_PATH "2"), EPROTOTYPE);
	close(wrong);

	/* A second program, in the same directory, through its own table. Every
	 * socket here is close-on-exec: an inherited Unix socket would fail the spawn. */
	pid_t pid;
	char *argv[] = {"/bin/t_unix", "filechild", SOCK_PATH, SOCK_PATH "2", NULL};
	CHECK(posix_spawn(&pid, "/bin/t_unix", NULL, NULL, argv, environ) == 0);
	int sa = accept(s, NULL, NULL);
	CHECK(sa >= 0);
	CHECK(read(sa, b, sizeof b) == 20 && memcmp(b, "hello from the child", 20) == 0);
	CHECK(write(sa, "the reply", 9) == 9);
	close(sa);
	int pa = accept(ps, NULL, NULL);
	CHECK(pa >= 0);
	CHECK(recv(pa, b, sizeof b, 0) == 3 && memcmp(b, "one", 3) == 0);
	CHECK(recv(pa, b, sizeof b, 0) == 6 && memcmp(b, "twotwo", 6) == 0);
	CHECK(recv(pa, b, sizeof b, 0) == 0);
	close(pa);
	int status = 0;
	CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);

	/* Closing the listener leaves an inert file; unlink removes it. */
	close(s);
	int c2 = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	FAILS(un_connect(c2, SOCK_PATH), ECONNREFUSED);
	OK(unlink(SOCK_PATH));
	FAILS(un_connect(c2, SOCK_PATH), ENOENT);
	OK(unlink(SOCK_PATH "2"));
	close(ps);
	close(c2);
	close(t);
	CHECK(first_free() == base);
	printf("t_unix: file passed\n");
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "all";
	if (argc > 3 && strcmp(mode, "filechild") == 0)
		return file_child(argv[2], argv[3]);
	int all = strcmp(mode, "all") == 0, known = 0;
#define CASE(name, fn)                                                     \
	if (all || strcmp(mode, name) == 0) {                              \
		known = 1;                                                 \
		fn();                                                      \
	}
	CASE("pair", pair_case)
	CASE("rights", rights_case)
	CASE("refused", refused_case)
	CASE("stream", stream_case)
	CASE("poll", poll_case)
	CASE("block", block_case)
	CASE("calls", calls_case)
	CASE("memfd", memfd_case)
	/* Not in `all`: it needs the shell's table (/tmp, /bin read-only). */
	if (strcmp(mode, "file") == 0) {
		known = 1;
		file_case();
	}
	if (!known) {
		printf("t_unix: unknown case %s FAIL\n", mode);
		return 2;
	}
	return 0;
}
