/*
 * posix_spawn with what each child is given, in QEMU (ADR-0411 item 1, plan
 * part F4, ADR-0433): run by posixrun on make test-libposix's image, and once
 * by zosh for the case that needs the shell's connection to the random
 * service. One binary with named cases; each prints "t_spawn: NAME passed" or
 * a FAIL line and exits 1. The programs it spawns are itself: "t_spawn child
 * MODE" writes what it finds on fd 1, which the parent reads.
 *
 *   table     an empty namespace with one read-only entry: the child has the
 *             fd the actions gave and no other, its table is that entry
 *             (read-only at the service), no cwd, no network, and a random
 *             connection of its own from the broker (ADR-0453 item 3)
 *   table2    two entries, one read-write: the child writes in it, and its
 *             cwd is the parent's, since a listed directory covers it
 *   default   no attribute: the child has the parent's table and fds, and no
 *             network connection (INV-0411-4); its random connection is the broker's
 *   refuse    an entry outside the parent's table, or wider than the parent
 *             holds it, is EACCES; no child is left; bad attributes are EINVAL
 *   grants    a connection the parent lacks is EACCES; with the shell's random
 *             connection (t_spawn random under zosh) the child has its own
 *   unix      a socketpair end of each type crosses a spawn: the child talks
 *             over it, and a descriptor it sends arrives here
 *   memfd     a 4 MiB memory descriptor crosses a spawn, and crosses an exec
 *             in the child; a read-only one is read-only there
 *   addopen   the child gets the file addopen opened, and the parent's table
 *             is as it was
 *   spawnp    posix_spawnp looks in PATH and reports a missing program
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <signal.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static const char *test = "";

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_spawn: %s: FAIL line %d: %s (errno %d: %s)\n", \
			       test, __LINE__, #cond, errno, strerror(errno));  \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define PASSED()                                                                \
	do {                                                                    \
		printf("t_spawn: %s passed\n", test);                           \
		fflush(stdout);                                                 \
	} while (0)

#define SELF "/bin/t_spawn"
#define MIB (1024 * 1024)
#define BIG (4 * MIB)
/* Where the memory descriptor is in the child: not a number a pipe of the parent may have. */
#define MEMFD_FD 20

/* ---------------------------------------------------------------- the child */

/* One line of the report on fd 1: a write, not stdio, so that it works with
 * what the table has. */
static void say(const char *fmt, ...)
{
	char line[256];
	va_list ap;
	va_start(ap, fmt);
	int n = vsnprintf(line, sizeof line, fmt, ap);
	va_end(ap);
	if (n > 0)
		(void)!write(1, line, (size_t)n);
}

static void probe(const char *path)
{
	int fd = open(path, O_RDONLY);
	say("open %s=%d\n", path, fd >= 0 ? 0 : errno);
	if (fd >= 0)
		close(fd);
}

static void create_probe(const char *path)
{
	int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0644);
	say("create %s=%d\n", path, fd >= 0 ? 0 : errno);
	if (fd >= 0) {
		close(fd);
		unlink(path);
	}
}

static void fds_line(void)
{
	char list[256] = "";
	size_t at = 0;
	for (int fd = 0; fd < 64 && at + 8 < sizeof list; fd++)
		if (fcntl(fd, F_GETFD) != -1)
			at += (size_t)snprintf(list + at, sizeof list - at, "%d,", fd);
	say("fds=%s\n", list);
}

/* What the child finds: its fds, its table, its connections. */
static int child_report(void)
{
	char cwd[128];
	fds_line();
	say("isatty0=%d\n", isatty(0));
	say("cwd=%s\n", getcwd(cwd, sizeof cwd) ? cwd : "none");
	probe(SELF);
	probe("/tmp");
	probe("/etc");
	probe("/dev/null");
	create_probe("/bin/_spawn_probe");
	create_probe("/tmp/_spawn_probe");
	char r[16];
	say("getrandom=%d\n", getrandom(r, sizeof r, 0) == (ssize_t)sizeof r ? 0 : errno);
	int s = socket(AF_INET, SOCK_STREAM, 0);
	struct sockaddr_in a = {0};
	a.sin_family = AF_INET;
	a.sin_port = htons(80);
	a.sin_addr.s_addr = htonl(0x0a000202);
	int e = 0;
	if (s >= 0 && connect(s, (struct sockaddr *)&a, sizeof a) != 0)
		e = errno;
	say("connect=%d\n", s < 0 ? errno : e == EACCES ? EACCES : 0);
	return 0;
}

static unsigned char pattern(size_t i)
{
	return (unsigned char)((i * 31 + (i >> 12)) & 0xff);
}

/* Sums what a memory descriptor at `fd` holds through a new mapping. */
static uint64_t sum_of(int fd, size_t size, int writable_maps)
{
	unsigned char *p = mmap(0, size, PROT_READ, MAP_SHARED, fd, 0);
	if (p == MAP_FAILED)
		return 0;
	uint64_t sum = 0;
	for (size_t i = 0; i < size; i++)
		sum += p[i];
	munmap(p, size);
	if (writable_maps) {
		void *w = mmap(0, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
		say("writable=%d\n", w == MAP_FAILED ? errno : 0);
	}
	return sum;
}

static uint64_t expected_sum(size_t size)
{
	uint64_t sum = 0;
	for (size_t i = 0; i < size; i++)
		sum += pattern(i);
	return sum;
}

/* The child of `unix`: fd 3 is a SOCK_SEQPACKET end, fd 4 a SOCK_STREAM end. */
static int child_unix(void)
{
	char b[64];
	CHECK(recv(3, b, sizeof b, 0) == 5 && memcmp(b, "hello", 5) == 0);
	CHECK(send(3, "answer", 6, 0) == 6);
	/* The stream: bytes both ways, and a descriptor the parent will read. */
	CHECK(recv(4, b, sizeof b, 0) == 6 && memcmp(b, "stream", 6) == 0);
	int p[2];
	CHECK(pipe(p) == 0);
	CHECK(write(p[1], "from the child", 14) == 14);
	char data = '!';
	struct iovec iov = {&data, 1};
	union {
		struct cmsghdr h;
		char buf[CMSG_SPACE(sizeof(int))];
	} u;
	memset(&u, 0, sizeof u);
	struct msghdr m = {0};
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	m.msg_control = u.buf;
	m.msg_controllen = sizeof u.buf;
	struct cmsghdr *c = CMSG_FIRSTHDR(&m);
	c->cmsg_level = SOL_SOCKET;
	c->cmsg_type = SCM_RIGHTS;
	c->cmsg_len = CMSG_LEN(sizeof(int));
	memcpy(CMSG_DATA(c), &p[0], sizeof(int));
	CHECK(sendmsg(4, &m, 0) == 1);
	say("unix=ok\n");
	return 0;
}

/* The child of `memfd`: fd 20 is the descriptor, `arg` names what to do. */
static int child_memfd(const char *arg)
{
	if (strcmp(arg, "exec") == 0) {
		/* Cross an exec: the new image finds the descriptor too. */
		char *argv[] = {SELF, "child", "memfd-after-exec", NULL};
		execv(SELF, argv);
		say("exec failed %d\n", errno);
		return 1;
	}
	struct stat st;
	CHECK(fstat(MEMFD_FD, &st) == 0 && st.st_size == BIG);
	say("sum=%llu\n", (unsigned long long)sum_of(MEMFD_FD, BIG, strcmp(arg, "ro") == 0));
	return 0;
}

static int child(int argc, char **argv)
{
	const char *mode = argc > 2 ? argv[2] : "";
	if (strcmp(mode, "report") == 0)
		return child_report();
	if (strcmp(mode, "unix") == 0)
		return child_unix();
	if (strcmp(mode, "memfd") == 0)
		return child_memfd(argc > 3 ? argv[3] : "");
	if (strcmp(mode, "memfd-after-exec") == 0)
		return child_memfd("");
	if (strcmp(mode, "read7") == 0) {
		char b[32] = "";
		ssize_t n = read(7, b, sizeof b - 1);
		say("fd7=%s\n", n > 0 ? b : "none");
		fds_line();
		return 0;
	}
	if (strcmp(mode, "hello") == 0) {
		say("hello\n");
		return 0;
	}
	return 2;
}

/* --------------------------------------------------------------- the parent */

/* The text of what the child wrote, and whether it exited 0. */
struct ran {
	char out[4096];
	int status;
	int spawn_error;
};

/* A pipe whose ends close on exec. */
static int cpipe(int p[2])
{
	if (pipe(p) != 0)
		return -1;
	fcntl(p[0], F_SETFD, FD_CLOEXEC);
	fcntl(p[1], F_SETFD, FD_CLOEXEC);
	return 0;
}

static int count_fds(void)
{
	int n = 0;
	for (int fd = 0; fd < 64; fd++)
		if (fcntl(fd, F_GETFD) != -1)
			n++;
	return n;
}

/*
 * Spawns SELF with `mode` (and `arg`), the write end of a pipe as its fd 1,
 * and `extra` actions already in `fa`; reads the pipe to its end and reaps.
 */
static void run(struct ran *r, posix_spawnattr_t *attr, posix_spawn_file_actions_t *fa,
		const char *mode, const char *arg)
{
	int p[2];
	CHECK(cpipe(p) == 0);
	CHECK(posix_spawn_file_actions_adddup2(fa, p[1], 1) == 0);
	char *argv[] = {SELF, "child", (char *)mode, (char *)arg, NULL};
	pid_t pid = 0;
	r->spawn_error = posix_spawn(&pid, SELF, fa, attr, argv, environ);
	close(p[1]);
	size_t at = 0;
	r->out[0] = 0;
	r->status = -1;
	if (r->spawn_error == 0) {
		ssize_t n;
		while (at < sizeof r->out - 1 && (n = read(p[0], r->out + at, sizeof r->out - 1 - at)) > 0)
			at += (size_t)n;
		r->out[at] = 0;
			CHECK(waitpid(pid, &r->status, 0) == pid);
	}
	close(p[0]);
}

#define HAS(r, text) (strstr((r).out, (text)) != NULL)

#define NEEDS(r, text)                                                          \
	do {                                                                    \
		if (!HAS(r, text)) {                                            \
			printf("t_spawn: %s: FAIL line %d: no \"%s\" in:\n%s\n", \
			       test, __LINE__, text, (r).out);                  \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define LACKS(r, text)                                                          \
	do {                                                                    \
		if (HAS(r, text)) {                                             \
			printf("t_spawn: %s: FAIL line %d: \"%s\" in:\n%s\n",   \
			       test, __LINE__, text, (r).out);                  \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

static void fresh(posix_spawnattr_t *attr, posix_spawn_file_actions_t *fa)
{
	CHECK(posix_spawnattr_init(attr) == 0);
	CHECK(posix_spawn_file_actions_init(fa) == 0);
}

static int ok_exit(const struct ran *r)
{
	return r->spawn_error == 0 && WIFEXITED(r->status) && WEXITSTATUS(r->status) == 0;
}

#define OKX(r)                                                                  \
	do {                                                                    \
		if (!ok_exit(&(r))) {                                           \
			printf("t_spawn: %s: FAIL line %d: child: spawn %d status %d\n%s\n", \
			       test, __LINE__, (r).spawn_error, (r).status, (r).out); \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

static void done(posix_spawnattr_t *attr, posix_spawn_file_actions_t *fa)
{
	posix_spawnattr_destroy(attr);
	posix_spawn_file_actions_destroy(fa);
}

/* ---------------------------------------------------------------- the cases */

static void t_table(void)
{
	static const char *const list[] = {"/bin:ro", NULL};
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, list) == 0);
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	/* The fd the actions named, and nothing else. */
	NEEDS(r, "fds=1,\n");
	NEEDS(r, "isatty0=0\n");
	/* The entry, read-only; everything else is outside the table. */
	NEEDS(r, "open " SELF "=0\n");
	NEEDS(r, "open /tmp=2\n");
	NEEDS(r, "open /etc=2\n");
	NEEDS(r, "open /dev/null=2\n");
	NEEDS(r, "create /bin/_spawn_probe=13\n");
	NEEDS(r, "create /tmp/_spawn_probe=2\n");
	/* The parent's cwd, /work, is in no entry. */
	NEEDS(r, "cwd=/\n");
	/* No network: a connect is EACCES. Random: the broker makes every child one, whatever its table
	 * (ADR-0453 item 3, which supersedes INV-0411-4 for randomness; under zosh the broker has one). */
	NEEDS(r, "getrandom=0\n");
	NEEDS(r, "connect=13\n");
	done(&attr, &fa);
	/* An empty table: nothing at all. */
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	NEEDS(r, "fds=1,\n");
	NEEDS(r, "open " SELF "=2\n");
	NEEDS(r, "create /bin/_spawn_probe=2\n");
	NEEDS(r, "cwd=/\n");
	done(&attr, &fa);
	PASSED();
}

static void t_table2(void)
{
	static const char *const list[] = {"/bin:ro", "/tmp", NULL};
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	CHECK(chdir("/tmp") == 0);
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, list) == 0);
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	NEEDS(r, "open /tmp=0\n");
	NEEDS(r, "open " SELF "=0\n");
	NEEDS(r, "create /bin/_spawn_probe=13\n");
	NEEDS(r, "create /tmp/_spawn_probe=0\n");
	/* The parent's cwd is inside /work, which is listed. */
	NEEDS(r, "cwd=/tmp\n");
	/* The attributes are reusable. */
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	done(&attr, &fa);
	PASSED();
}

static void t_default(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	int before = count_fds();
	fresh(&attr, &fa);
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	/* The parent's table, and its non-close-on-exec fds. */
	NEEDS(r, "open " SELF "=0\n");
	NEEDS(r, "open /tmp=0\n");
	NEEDS(r, "create /tmp/_spawn_probe=0\n");
	NEEDS(r, "create /bin/_spawn_probe=13\n");
	NEEDS(r, "fds=");
	/* No network, as a spawn always had (INV-0411-4); but a random connection of its own, which the broker makes
	 * for every child (ADR-0453 item 3). */
	NEEDS(r, "getrandom=0\n");
	NEEDS(r, "connect=13\n");
	done(&attr, &fa);
	CHECK(count_fds() == before);
	PASSED();
}

static void t_refuse(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	/* A path outside the parent's table; wider than the parent holds it; a
	 * file where a directory is wanted; every one a refusal of the spawn. */
	static const char *const outside[] = {"/etc", NULL};
	static const char *const wider[] = {"/bin", NULL};
	static const char *const file[] = {SELF ":ro", NULL};
	static const char *const relative[] = {"bin", NULL};
	static const char *const twice[] = {"/bin:ro", "/bin:ro", NULL};
	static const char *const *const bad[] = {outside, wider, file};
	static const int want[] = {EACCES, EACCES, ENOTDIR};
	for (unsigned i = 0; i < 3; i++) {
		fresh(&attr, &fa);
		CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
		CHECK(posix_spawnattr_setnamespace_np(&attr, bad[i]) == 0);
		run(&r, &attr, &fa, "hello", "");
		CHECK(r.spawn_error == want[i]);
		done(&attr, &fa);
	}
	/* No child is left behind by a refusal. */
	errno = 0;
	CHECK(waitpid(-1, NULL, WNOHANG) == -1 && errno == ECHILD);
	/* The attribute calls refuse what no spawn could do. */
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setnamespace_np(&attr, relative) == EINVAL);
	CHECK(posix_spawnattr_setgrants_np(&attr, 64) == EINVAL);
	CHECK(posix_spawnattr_setgrants_np(&attr, POSIX_SPAWN_GRANT_NET | POSIX_SPAWN_MOVE_NET) == EINVAL);
	/* A list without the flag, and a path twice. */
	CHECK(posix_spawnattr_setnamespace_np(&attr, wider) == 0);
	run(&r, &attr, &fa, "hello", "");
	CHECK(r.spawn_error == EINVAL);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, twice) == 0);
	run(&r, &attr, &fa, "hello", "");
	CHECK(r.spawn_error == EINVAL);
	/* An unknown flag. */
	CHECK(posix_spawnattr_setflags(&attr, 0x1000) == EINVAL);
	done(&attr, &fa);
	PASSED();
}

/* Whether this process has the shell's random connection. */
static int has_random(void)
{
	char b[8];
	return getrandom(b, sizeof b, 0) == (ssize_t)sizeof b;
}

static void t_grants(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	int random = has_random();
	printf("t_spawn: parent_random=%d\n", random);
	/* Without the connection the parent lacks, a grant is EACCES. */
	unsigned lacking[] = {POSIX_SPAWN_GRANT_NET, POSIX_SPAWN_MOVE_NET,
			      POSIX_SPAWN_GRANT_WINDOW,
			      random ? 0 : POSIX_SPAWN_GRANT_RANDOM};
	for (unsigned i = 0; i < 4; i++) {
		if (!lacking[i])
			continue;
		fresh(&attr, &fa);
		CHECK(posix_spawnattr_setgrants_np(&attr, lacking[i]) == 0);
		run(&r, &attr, &fa, "report", "");
		CHECK(r.spawn_error == EACCES);
		done(&attr, &fa);
	}
	errno = 0;
	CHECK(waitpid(-1, NULL, WNOHANG) == -1 && errno == ECHILD);
	if (random) {
		/* With the grant the child has the parent's kind, and without it the broker's (ADR-0453 item 3). */
		fresh(&attr, &fa);
		CHECK(posix_spawnattr_setgrants_np(&attr, POSIX_SPAWN_GRANT_RANDOM) == 0);
		run(&r, &attr, &fa, "report", "");
		OKX(r);
		NEEDS(r, "getrandom=0\n");
		NEEDS(r, "connect=13\n");
		done(&attr, &fa);
		fresh(&attr, &fa);
		run(&r, &attr, &fa, "report", "");
		OKX(r);
		NEEDS(r, "getrandom=0\n");
		NEEDS(r, "connect=13\n");
		done(&attr, &fa);
		/* And an empty table with the grant: random, and nothing else. */
		fresh(&attr, &fa);
		CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
		CHECK(posix_spawnattr_setgrants_np(&attr, POSIX_SPAWN_GRANT_RANDOM) == 0);
		run(&r, &attr, &fa, "report", "");
		OKX(r);
		NEEDS(r, "getrandom=0\n");
		NEEDS(r, "open " SELF "=2\n");
		NEEDS(r, "fds=1,\n");
		done(&attr, &fa);
		/* The parent still has its own. */
		CHECK(has_random());
	}
	PASSED();
}

static void t_unix(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	int pk[2], st[2];
	/* Made without close-on-exec, as a program that hands them down makes them. */
	CHECK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, pk) == 0);
	CHECK(socketpair(AF_UNIX, SOCK_STREAM, 0, st) == 0);
	fresh(&attr, &fa);
	/* An empty table whose only fds are the two ends, at 3 and 4. */
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, pk[1], 3) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, st[1], 4) == 0);
	int p[2];
	CHECK(cpipe(p) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, p[1], 1) == 0);
	char *argv[] = {SELF, "child", "unix", NULL};
	pid_t pid;
	CHECK(posix_spawn(&pid, SELF, &fa, &attr, argv, environ) == 0);
	close(p[1]);
	/* The packet socket moved: the parent's copy is dead, the child's is not. */
	CHECK(send(pk[1], "x", 1, 0) == -1);
	close(pk[1]);
	CHECK(send(pk[0], "hello", 5, 0) == 5);
	char b[64];
	CHECK(recv(pk[0], b, sizeof b, 0) == 6 && memcmp(b, "answer", 6) == 0);
	/* The stream: the parent keeps its end of the pair; the child has the
	 * other, whole, with the ability to send descriptors. */
	close(st[1]);
	CHECK(send(st[0], "stream", 6, 0) == 6);
	struct iovec iov = {b, sizeof b};
	union {
		struct cmsghdr h;
		char buf[CMSG_SPACE(sizeof(int))];
	} u;
	memset(&u, 0, sizeof u);
	struct msghdr m = {0};
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	m.msg_control = u.buf;
	m.msg_controllen = sizeof u.buf;
	CHECK(recvmsg(st[0], &m, 0) == 1 && b[0] == '!');
	struct cmsghdr *c = CMSG_FIRSTHDR(&m);
	CHECK(c && c->cmsg_type == SCM_RIGHTS);
	int got;
	memcpy(&got, CMSG_DATA(c), sizeof got);
	CHECK(read(got, b, sizeof b) == 14 && memcmp(b, "from the child", 14) == 0);
	ssize_t n = read(p[0], b, sizeof b);
	CHECK(n > 0);
	b[n] = 0;
	CHECK(strstr(b, "unix=ok") != NULL);
	int status;
	CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
	/* The child going away closes its ends: the parent reads the end. */
	CHECK(recv(pk[0], b, sizeof b, 0) == 0);
	close(p[0]);
	close(pk[0]);
	close(st[0]);
	close(got);
	done(&attr, &fa);
	PASSED();
}

static int memfd_of(size_t size, unsigned seals)
{
	int fd = memfd_create("spawn", MFD_ALLOW_SEALING);
	CHECK(fd >= 0 && ftruncate(fd, (off_t)size) == 0);
	unsigned char *w = mmap(0, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(w != MAP_FAILED);
	for (size_t i = 0; i < size; i++)
		w[i] = pattern(i);
	munmap(w, size);
	if (seals)
		CHECK(fcntl(fd, F_ADD_SEALS, seals) == 0);
	return fd;
}

static const char *const bin_only[] = {"/bin:ro", NULL};

static void t_memfd(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	char want[64];
	snprintf(want, sizeof want, "sum=%llu\n", (unsigned long long)expected_sum(BIG));
	/* The plan's criterion: a 4 MiB descriptor to a child that reads it. */
	int fd = memfd_of(BIG, 0);
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, bin_only) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, fd, MEMFD_FD) == 0);
	run(&r, &attr, &fa, "memfd", "rw");
	OKX(r);
	NEEDS(r, want);
	/* The same object: what the parent writes now is what a reader sees. */
	done(&attr, &fa);
	/* Across an exec in the child. */
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, bin_only) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, fd, MEMFD_FD) == 0);
	run(&r, &attr, &fa, "memfd", "exec");
	OKX(r);
	NEEDS(r, want);
	done(&attr, &fa);
	close(fd);
	/* A close-on-exec descriptor is no obstacle to a default spawn, and is not
	 * inherited: the child's fd list has the pipe and the parent's others only. */
	int c = memfd_create("c", MFD_CLOEXEC);
	CHECK(c >= 0 && ftruncate(c, 4096) == 0);
	char number[16];
	snprintf(number, sizeof number, "%d,", c);
	fresh(&attr, &fa);
	run(&r, &attr, &fa, "report", "");
	OKX(r);
	NEEDS(r, "fds=");
	CHECK(strstr(strstr(r.out, "fds="), number) == NULL || c < 3);
	done(&attr, &fa);
	close(c);
	/* A read-only descriptor stays read-only in the child, as the kernel sees it. */
	int ro = memfd_of(BIG, F_SEAL_FUTURE_WRITE);
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawnattr_setnamespace_np(&attr, bin_only) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, ro, MEMFD_FD) == 0);
	run(&r, &attr, &fa, "memfd", "ro");
	OKX(r);
	NEEDS(r, want);
	NEEDS(r, "writable=13\n");
	done(&attr, &fa);
	close(ro);
	PASSED();
}

static void t_addopen(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	int fd = open("/tmp/in.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0 && write(fd, "hello", 5) == 5);
	close(fd);
	int before = count_fds();
	/* An empty table: the file is outside it, and is the child's only fd but 1. */
	fresh(&attr, &fa);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawn_file_actions_addopen(&fa, 7, "/tmp/in.txt", O_RDONLY, 0) == 0);
	run(&r, &attr, &fa, "read7", "");
	OKX(r);
	NEEDS(r, "fd7=hello\n");
	NEEDS(r, "fds=1,7,\n");
	done(&attr, &fa);
	CHECK(count_fds() == before);
	/* A file that is not there is the spawn's error, and no fd is left. */
	fresh(&attr, &fa);
	CHECK(posix_spawn_file_actions_addopen(&fa, 7, "/tmp/none", O_RDONLY, 0) == 0);
	run(&r, &attr, &fa, "read7", "");
	CHECK(r.spawn_error == ENOENT);
	done(&attr, &fa);
	CHECK(count_fds() == before);
	unlink("/tmp/in.txt");
	PASSED();
}

static void t_spawnp(void)
{
	posix_spawn_file_actions_t fa;
	posix_spawnattr_t attr;
	fresh(&attr, &fa);
	int p[2];
	CHECK(cpipe(p) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, p[1], 1) == 0);
	char *argv[] = {"t_spawn", "child", "hello", NULL};
	pid_t pid;
	CHECK(setenv("PATH", "/nonexistent:/bin", 1) == 0);
	CHECK(posix_spawnp(&pid, "t_spawn", &fa, &attr, argv, environ) == 0);
	close(p[1]);
	char b[32] = "";
	CHECK(read(p[0], b, sizeof b - 1) == 6 && strcmp(b, "hello\n") == 0);
	int status;
	CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
	close(p[0]);
	/* A program that is in no directory of PATH. */
	CHECK(posix_spawnp(&pid, "t_spawn_nothing", &fa, &attr, argv, environ) == ENOENT);
	CHECK(posix_spawnp(&pid, "/bin/t_spawn_nothing", &fa, &attr, argv, environ) == ENOENT);
	done(&attr, &fa);
	PASSED();
}

/* Review of F4, finding 2: a socket the actions do not list stays with the
 * parent, as it did before; the spawn is EBADF (what system() of a program
 * that holds one sees) and the socket still works. */
static void t_inherited(void)
{
	static const int types[] = {SOCK_SEQPACKET, SOCK_DGRAM, SOCK_STREAM};
	for (int i = 0; i < 3; i++) {
		posix_spawnattr_t attr;
		posix_spawn_file_actions_t fa;
		struct ran r;
		int sv[2];
		CHECK(socketpair(AF_UNIX, types[i], 0, sv) == 0);
		fresh(&attr, &fa);
		run(&r, &attr, &fa, "hello", "");
		CHECK(r.spawn_error == EBADF);
		done(&attr, &fa);
		char b[8];
		CHECK(send(sv[1], "ok", 2, 0) == 2);
		CHECK(recv(sv[0], b, sizeof b, 0) == 2 && memcmp(b, "ok", 2) == 0);
		close(sv[0]);
		close(sv[1]);
	}
	PASSED();
}

/* Review of F4, finding 5: a socket the kernel will not move (its end holds a
 * descriptor nobody has read) makes the spawn fail, and it leaves the
 * parent's socket and its connections as they were. */
static void t_queued(void)
{
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	struct ran r;
	int sv[2], p[2];
	CHECK(socketpair(AF_UNIX, SOCK_SEQPACKET, 0, sv) == 0);
	CHECK(pipe(p) == 0);
	struct iovec iov = {"x", 1};
	union {
		struct cmsghdr h;
		char buf[CMSG_SPACE(sizeof(int))];
	} u;
	memset(&u, 0, sizeof u);
	struct msghdr m = {0};
	m.msg_iov = &iov;
	m.msg_iovlen = 1;
	m.msg_control = u.buf;
	m.msg_controllen = sizeof u.buf;
	struct cmsghdr *c = CMSG_FIRSTHDR(&m);
	c->cmsg_level = SOL_SOCKET;
	c->cmsg_type = SCM_RIGHTS;
	c->cmsg_len = CMSG_LEN(sizeof(int));
	memcpy(CMSG_DATA(c), &p[0], sizeof(int));
	CHECK(sendmsg(sv[0], &m, 0) == 1);
	/* sv[1] now has a descriptor queued: the kernel does not move it. */
	fresh(&attr, &fa);
	fcntl(sv[0], F_SETFD, FD_CLOEXEC);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_EMPTY_NAMESPACE) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, sv[1], 9) == 0);
	run(&r, &attr, &fa, "hello", "");
	CHECK(r.spawn_error != 0);
	done(&attr, &fa);
	/* The parent's socket is whole: it reads what was sent, descriptor and all. */
	char b[8];
	struct iovec in = {b, sizeof b};
	struct msghdr rm = {0};
	union {
		struct cmsghdr h;
		char buf[CMSG_SPACE(sizeof(int))];
	} ru;
	rm.msg_iov = &in;
	rm.msg_iovlen = 1;
	rm.msg_control = ru.buf;
	rm.msg_controllen = sizeof ru.buf;
	CHECK(recvmsg(sv[1], &rm, 0) == 1);
	CHECK(CMSG_FIRSTHDR(&rm) != NULL);
	PASSED();
}

int main(int argc, char **argv)
{
	if (argc > 1 && strcmp(argv[1], "child") == 0)
		return child(argc, argv);
	if (argc < 2) {
		printf("usage: t_spawn table|table2|default|refuse|grants|unix|memfd|addopen|spawnp|random\n");
		return 2;
	}
	test = argv[1];
	/* A send to a socket that moved away is EPIPE, not a signal. */
	signal(SIGPIPE, SIG_IGN);
	if (strcmp(test, "table") == 0)
		t_table();
	else if (strcmp(test, "table2") == 0)
		t_table2();
	else if (strcmp(test, "default") == 0)
		t_default();
	else if (strcmp(test, "refuse") == 0)
		t_refuse();
	else if (strcmp(test, "grants") == 0)
		t_grants();
	else if (strcmp(test, "random") == 0) {
		/* Under zosh, which gives each program the random service. */
		CHECK(has_random());
		test = "grants";
		t_grants();
		test = "random";
		PASSED();
	} else if (strcmp(test, "unix") == 0)
		t_unix();
	else if (strcmp(test, "memfd") == 0)
		t_memfd();

	else if (strcmp(test, "addopen") == 0)
		t_addopen();
	else if (strcmp(test, "inherited") == 0)
		t_inherited();
	else if (strcmp(test, "queued") == 0)
		t_queued();
	else if (strcmp(test, "spawnp") == 0)
		t_spawnp();
	else {
		printf("t_spawn: unknown case %s\n", test);
		return 2;
	}
	return 0;
}
