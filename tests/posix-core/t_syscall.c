/*
 * INV-0451-1 (ADR-0451 item 1): every libposix definition returns the width its
 * C prototype declares. `syscall()` was `int` where <unistd.h> says `long`, as
 * `tee` and `sendmsg` were `int` for `ssize_t`: on AArch64 the upper half of
 * the result register was garbage, so -1 read as 4294967295 (a success).
 *
 *   t_syscall syscall   syscall() is a long; a number it does not answer gives
 *                       -1 and ENOSYS, as a 64-bit -1 (also with a number
 *                       whose low 32 bits are a valid number)
 *   t_syscall stubs     tee returns ssize_t: -1 and ENOSYS (sendmsg is outside the
 *                       tier list: libc.a hides it as __zoraya_sendmsg, so only its
 *                       prototype is checked here, and its definition by
 *                       make check-libposix-widths)
 *   t_syscall wide      a result above 2^32 survives: lseek's off_t past 4 GiB
 *
 * ADR-0451 items 1 to 3, built by part work/rust-std (Linux AArch64's numbers,
 * which the sysroot's <sys/syscall.h> does not define: futex 98, gettid 178,
 * getrandom 278):
 *
 *   t_syscall futex     FUTEX_WAIT (relative) and FUTEX_WAIT_BITSET (an absolute
 *                       CLOCK_MONOTONIC deadline, the full mask) with and without
 *                       FUTEX_PRIVATE_FLAG: EAGAIN on a changed word, ETIMEDOUT
 *                       after the deadline and not before, a wake by a thread; a
 *                       mask other than the full one, FUTEX_CLOCK_REALTIME and
 *                       the requeues are ENOSYS
 *   t_syscall random    syscall(278) as getrandom (under zosh, which gives the
 *                       random service)
 *   t_syscall tid       syscall(178) is gettid(); a thread's differs from the
 *                       main thread's
 *   t_syscall fionbio   ioctl(FIONBIO) as fcntl(F_SETFL, O_NONBLOCK): a read of
 *                       an empty pipe is EAGAIN, and the flag is read back
 *   t_syscall names     sched_yield, clock_nanosleep (relative and absolute, a
 *                       bad clock), dirfd, getauxval, bcmp, pipe2
 *   t_syscall chdir     posix_spawn_file_actions_addchdir (under zosh, which
 *                       gives a broker): the child starts in /tmp, then in
 *                       /tmp/sub; a missing one fails the spawn
 *   t_syscall cwd DIR   (the child of chdir) exits 0 if getcwd is DIR
 *   t_syscall report    an unanswered number is reported to the shell once
 *                       (the shell prints the note after the line)
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/auxv.h>
#include <sys/ioctl.h>
#include <sys/random.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* Linux AArch64's numbers (ADR-0451 item 1). */
#define NR_FUTEX 98L
#define NR_GETTID 178L
#define NR_GETRANDOM 278L
#define FUTEX_WAIT 0L
#define FUTEX_WAKE 1L
#define FUTEX_REQUEUE 3L
#define FUTEX_WAIT_BITSET 9L
#define FUTEX_PRIVATE 128L
#define FUTEX_CLOCK_REALTIME 256L
#define MATCH_ANY 0xffffffffL

/* The declared types, at compile time (the call's value is what is checked at
 * run time: a narrow definition passes this and fails below). */
_Static_assert(sizeof(syscall(1L)) == 8, "syscall() returns long");
_Static_assert(sizeof(tee(0, 0, 0, 0)) == 8, "tee() returns ssize_t");
_Static_assert(sizeof(sendmsg(0, 0, 0)) == 8, "sendmsg() returns ssize_t");

static int checks;

#define CHECK(cond)                                                             \
	do {                                                                    \
		checks++;                                                       \
		if (!(cond)) {                                                  \
			printf("t_syscall: FAIL line %d: %s (errno %d: %s)\n",  \
			       __LINE__, #cond, errno, strerror(errno));        \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

/* A result that must be the 64-bit -1 with errno ENOSYS, not 0xffffffff. */
#define ENOSYS64(expr)                                                          \
	do {                                                                    \
		errno = 0;                                                      \
		long long r_ = (long long)(expr);                               \
		CHECK(r_ == -1LL);                                              \
		CHECK((unsigned long long)r_ == 0xffffffffffffffffULL);         \
		CHECK(errno == ENOSYS);                                         \
	} while (0)

static void numbers(void)
{
	/* Numbers no table holds, in the range, far above it, negative, and with
	 * a listed number in the low 32 bits only. */
	ENOSYS64(syscall(9999L));
	ENOSYS64(syscall(4096L, 1L, 2L, 3L, 4L, 5L, 6L));
	ENOSYS64(syscall(-1L));
	ENOSYS64(syscall(0x7fffffffL));
	ENOSYS64(syscall((1L << 32) | 98L));
	ENOSYS64(syscall((1L << 40) + 1L));
	printf("t_syscall: syscall passed (%d checks)\n", checks);
}

static void stubs(void)
{
	ENOSYS64(tee(-1, -1, 4096, 0));
	printf("t_syscall: stubs passed (%d checks)\n", checks);
}

static void wide(void)
{
	const char *name = "/work/t_syscall.wide";
	int fd = open(name, O_RDWR | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	/* off_t is 64 bits: the position past 4 GiB comes back whole. */
	off_t big = (off_t)5 << 30;
	CHECK(lseek(fd, big, SEEK_SET) == big);
	CHECK(lseek(fd, 0, SEEK_CUR) == big);
	CHECK(lseek(fd, 1, SEEK_CUR) == big + 1);
	CHECK(close(fd) == 0);
	CHECK(unlink(name) == 0);
	printf("t_syscall: wide passed (%d checks)\n", checks);
}

static long long now_ns(clockid_t c)
{
	struct timespec t;
	clock_gettime(c, &t);
	return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

static volatile unsigned word;
static volatile int woke;

static void *waiter(void *arg)
{
	(void)arg;
	/* No timeout: sleeps while the word is 0, until the main thread wakes it. */
	long r = syscall(NR_FUTEX, &word, FUTEX_WAIT_BITSET | FUTEX_PRIVATE, 0L, 0L, 0L, MATCH_ANY);
	woke = (r == 0) ? 1 : -1;
	return NULL;
}

static void futex(void)
{
	unsigned w = 1;
	struct timespec rel = { 0, 20 * 1000 * 1000 };
	struct timespec abs;
	long long t0, dt;

	/* The word is not what the caller expected: EAGAIN at once. */
	errno = 0;
	CHECK(syscall(NR_FUTEX, &w, FUTEX_WAIT, 0L, 0L, 0L, 0L) == -1L && errno == EAGAIN);
	errno = 0;
	CHECK(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET | FUTEX_PRIVATE, 0L, 0L, 0L, MATCH_ANY) == -1L
	      && errno == EAGAIN);
	/* A relative wait times out, after its time and not far after. */
	t0 = now_ns(CLOCK_MONOTONIC);
	errno = 0;
	CHECK(syscall(NR_FUTEX, &w, FUTEX_WAIT | FUTEX_PRIVATE, 1L, &rel, 0L, 0L) == -1L
	      && errno == ETIMEDOUT);
	dt = now_ns(CLOCK_MONOTONIC) - t0;
	CHECK(dt >= 20LL * 1000 * 1000 && dt < 2000LL * 1000 * 1000);
	/* An absolute wait on CLOCK_MONOTONIC, with and without the private flag. */
	for (int i = 0; i < 2; i++) {
		long long deadline = now_ns(CLOCK_MONOTONIC) + 30LL * 1000 * 1000;
		abs.tv_sec = deadline / 1000000000LL;
		abs.tv_nsec = deadline % 1000000000LL;
		errno = 0;
		CHECK(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET | (i ? FUTEX_PRIVATE : 0L), 1L, &abs, 0L,
			      MATCH_ANY) == -1L && errno == ETIMEDOUT);
		CHECK(now_ns(CLOCK_MONOTONIC) >= deadline);
		CHECK(now_ns(CLOCK_MONOTONIC) < deadline + 2000LL * 1000 * 1000);
	}
	/* A deadline in the past is a time-out at once. */
	abs.tv_sec = 0;
	abs.tv_nsec = 1;
	errno = 0;
	CHECK(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET | FUTEX_PRIVATE, 1L, &abs, 0L, MATCH_ANY) == -1L
	      && errno == ETIMEDOUT);
	/* A wake with nobody waiting wakes nobody. */
	CHECK(syscall(NR_FUTEX, &w, FUTEX_WAKE | FUTEX_PRIVATE, 1L, 0L, 0L, 0L) == 0L);
	/* Forms the table does not hold. */
	ENOSYS64(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET, 1L, 0L, 0L, 1L));
	ENOSYS64(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET, 1L, 0L, 0L, 0L));
	ENOSYS64(syscall(NR_FUTEX, &w, FUTEX_WAIT_BITSET | FUTEX_CLOCK_REALTIME, 1L, 0L, 0L, MATCH_ANY));
	ENOSYS64(syscall(NR_FUTEX, &w, FUTEX_REQUEUE, 1L, 1L, &w, 0L));
	/* A thread asleep on the word is woken by a wake on it. */
	pthread_t t;
	word = 0;
	woke = 0;
	CHECK(pthread_create(&t, NULL, waiter, NULL) == 0);
	for (int i = 0; i < 2000 && !woke; i++) {
		word = 0;
		/* The waiter may not be asleep yet: wake until it has gone. */
		long n = syscall(NR_FUTEX, &word, FUTEX_WAKE | FUTEX_PRIVATE, 1L, 0L, 0L, 0L);
		if (n == 1)
			break;
		usleep(1000);
	}
	CHECK(pthread_join(t, NULL) == 0);
	CHECK(woke == 1);
	printf("t_syscall: futex passed (%d checks)\n", checks);
}

static void random_(void)
{
	unsigned char a[32] = { 0 }, b[32] = { 0 }, z[32] = { 0 };
	CHECK(syscall(NR_GETRANDOM, a, (long)sizeof a, 0L) == (long)sizeof a);
	CHECK(syscall(NR_GETRANDOM, b, (long)sizeof b, 0L) == (long)sizeof b);
	CHECK(memcmp(a, z, sizeof a) != 0);
	CHECK(memcmp(a, b, sizeof a) != 0);
	/* As getrandom itself. */
	CHECK(getrandom(b, sizeof b, 0) == (ssize_t)sizeof b);
	/* A bad buffer is EFAULT as for getrandom, a 64-bit -1 either way. */
	errno = 0;
	CHECK(syscall(NR_GETRANDOM, (void *)8, 16L, 0L) == -1L && errno == EFAULT);
	printf("t_syscall: random passed (%d checks)\n", checks);
}

static volatile long thread_tid;

static void *tid_thread(void *arg)
{
	(void)arg;
	thread_tid = syscall(NR_GETTID);
	return (void *)(long)gettid();
}

static void tid(void)
{
	long main_tid = syscall(NR_GETTID);
	CHECK(main_tid > 0 && main_tid == (long)gettid());
	pthread_t t;
	void *ret;
	CHECK(pthread_create(&t, NULL, tid_thread, NULL) == 0);
	CHECK(pthread_join(t, &ret) == 0);
	CHECK(thread_tid > 0 && thread_tid != main_tid);
	CHECK((long)ret == thread_tid);
	/* Still the main thread's afterwards. */
	CHECK(syscall(NR_GETTID) == main_tid);
	printf("t_syscall: tid passed (%d checks)\n", checks);
}

static void fionbio(void)
{
	int fds[2], on = 1, off = 0;
	char c;
	CHECK(pipe2(fds, O_CLOEXEC) == 0);
	CHECK((fcntl(fds[0], F_GETFL) & O_NONBLOCK) == 0);
	CHECK(ioctl(fds[0], FIONBIO, &on) == 0);
	CHECK((fcntl(fds[0], F_GETFL) & O_NONBLOCK) != 0);
	errno = 0;
	CHECK(read(fds[0], &c, 1) == -1 && errno == EAGAIN);
	CHECK(ioctl(fds[0], FIONBIO, &off) == 0);
	CHECK((fcntl(fds[0], F_GETFL) & O_NONBLOCK) == 0);
	/* The descriptor's other flags stay. */
	CHECK((fcntl(fds[0], F_GETFD) & FD_CLOEXEC) != 0);
	/* A bad descriptor is EBADF; another request is as before. */
	errno = 0;
	CHECK(ioctl(-1, FIONBIO, &on) == -1 && errno == EBADF);
	errno = 0;
	CHECK(ioctl(fds[0], FIONREAD, &on) == -1 && errno == ENOTTY);
	CHECK(close(fds[0]) == 0 && close(fds[1]) == 0);
	printf("t_syscall: fionbio passed (%d checks)\n", checks);
}

static void names(void)
{
	struct timespec t = { 0, 20 * 1000 * 1000 }, abs;
	long long t0;
	CHECK(sched_yield() == 0);
	/* clock_nanosleep returns the error number and sets no errno. */
	t0 = now_ns(CLOCK_MONOTONIC);
	CHECK(clock_nanosleep(CLOCK_MONOTONIC, 0, &t, NULL) == 0);
	CHECK(now_ns(CLOCK_MONOTONIC) - t0 >= 20LL * 1000 * 1000);
	t0 = now_ns(CLOCK_REALTIME) + 20LL * 1000 * 1000;
	abs.tv_sec = t0 / 1000000000LL;
	abs.tv_nsec = t0 % 1000000000LL;
	CHECK(clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, &abs, NULL) == 0);
	CHECK(now_ns(CLOCK_REALTIME) >= t0);
	t0 = now_ns(CLOCK_MONOTONIC) + 20LL * 1000 * 1000;
	abs.tv_sec = t0 / 1000000000LL;
	abs.tv_nsec = t0 % 1000000000LL;
	CHECK(clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &abs, NULL) == 0);
	CHECK(now_ns(CLOCK_MONOTONIC) >= t0);
	CHECK(clock_nanosleep(CLOCK_PROCESS_CPUTIME_ID, 0, &t, NULL) == EINVAL);
	CHECK(clock_nanosleep(99, 0, &t, NULL) == EINVAL);
	CHECK(clock_nanosleep(CLOCK_MONOTONIC, 8, &t, NULL) == EINVAL);
	t.tv_nsec = 1000000000L;
	CHECK(clock_nanosleep(CLOCK_MONOTONIC, 0, &t, NULL) == EINVAL);
	/* dirfd is the descriptor of the stream. */
	DIR *d = opendir("/");
	CHECK(d != NULL);
	int fd = dirfd(d);
	CHECK(fd >= 0 && fcntl(fd, F_GETFD) >= 0);
	CHECK(closedir(d) == 0);
	errno = 0;
	CHECK(dirfd(NULL) == -1 && errno == EINVAL);
	CHECK(getauxval(AT_PAGESZ) == 4096);
	errno = 0;
	CHECK(getauxval(0x7777) == 0 && errno == ENOENT);
	CHECK(bcmp("abc", "abc", 3) == 0 && bcmp("abc", "abd", 3) != 0);
	int p[2];
	CHECK(pipe2(p, O_NONBLOCK | O_CLOEXEC) == 0);
	CHECK((fcntl(p[0], F_GETFL) & O_NONBLOCK) != 0 && (fcntl(p[1], F_GETFD) & FD_CLOEXEC) != 0);
	printf("t_syscall: names passed (%d checks)\n", checks);
}

/* The child of chdir. */
static void cwd(const char *want)
{
	char buf[512];
	CHECK(getcwd(buf, sizeof buf) != NULL);
	printf("t_syscall: child cwd %s\n", buf);
	CHECK(strcmp(buf, want) == 0);
}

static void chdir_(void)
{
	posix_spawn_file_actions_t fa;
	pid_t pid;
	int status;
	char *argv[] = { "t_syscall", "cwd", "/tmp", NULL };
	char *argv2[] = { "t_syscall", "cwd", "/tmp/sub", NULL };
	char before[512], after[512];
	CHECK(getcwd(before, sizeof before) != NULL);
	CHECK(mkdir("/tmp/sub", 0755) == 0 || errno == EEXIST);
	CHECK(posix_spawn_file_actions_init(&fa) == 0);
	CHECK(posix_spawn_file_actions_addchdir(&fa, "/tmp") == 0);
	CHECK(posix_spawn(&pid, "/bin/t_syscall", &fa, NULL, argv, environ) == 0);
	CHECK(waitpid(pid, &status, 0) == pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	CHECK(posix_spawn_file_actions_destroy(&fa) == 0);
	/* A relative path follows the one before it; the _np name is the same. */
	CHECK(posix_spawn_file_actions_init(&fa) == 0);
	CHECK(posix_spawn_file_actions_addchdir_np(&fa, "/tmp") == 0);
	CHECK(posix_spawn_file_actions_addchdir_np(&fa, "sub") == 0);
	CHECK(posix_spawn(&pid, "/bin/t_syscall", &fa, NULL, argv2, environ) == 0);
	CHECK(waitpid(pid, &status, 0) == pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	CHECK(posix_spawn_file_actions_destroy(&fa) == 0);
	/* A directory that is not there fails the spawn, as chdir would. */
	CHECK(posix_spawn_file_actions_init(&fa) == 0);
	CHECK(posix_spawn_file_actions_addchdir(&fa, "/tmp/missing") == 0);
	CHECK(posix_spawn(&pid, "/bin/t_syscall", &fa, NULL, argv, environ) == ENOENT);
	CHECK(posix_spawn_file_actions_destroy(&fa) == 0);
	/* The parent stayed where it was. */
	CHECK(getcwd(after, sizeof after) != NULL && strcmp(before, after) == 0);
	printf("t_syscall: chdir passed (%d checks)\n", checks);
}

static void report(void)
{
	/* A number no table holds, twice: the shell prints one note after the line. */
	ENOSYS64(syscall(279L));
	ENOSYS64(syscall(279L));
	/* A number std falls back from is not reported. */
	ENOSYS64(syscall(291L));
	printf("t_syscall: report passed (%d checks)\n", checks);
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "syscall";
	if (strcmp(mode, "syscall") == 0)
		numbers();
	else if (strcmp(mode, "stubs") == 0)
		stubs();
	else if (strcmp(mode, "wide") == 0)
		wide();
	else if (strcmp(mode, "futex") == 0)
		futex();
	else if (strcmp(mode, "random") == 0)
		random_();
	else if (strcmp(mode, "tid") == 0)
		tid();
	else if (strcmp(mode, "fionbio") == 0)
		fionbio();
	else if (strcmp(mode, "names") == 0)
		names();
	else if (strcmp(mode, "chdir") == 0)
		chdir_();
	else if (strcmp(mode, "cwd") == 0)
		cwd(argc > 2 ? argv[2] : "/");
	else if (strcmp(mode, "report") == 0)
		report();
	else {
		printf("t_syscall: unknown mode %s\n", mode);
		return 2;
	}
	return 0;
}
