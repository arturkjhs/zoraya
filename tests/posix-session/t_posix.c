/*
 * The process half of libposix in QEMU (TASK-21): run from dash in the
 * POSIX session of make test-posix, through the broker, ttyd and tmpd. Each
 * subcommand checks one invariant of docs/adr/invariants.md and prints
 * "t_posix: NAME passed", or a FAIL line and exits 1.
 *
 *   t_posix forkpos    INV-0103-2: after fork an inherited fd's position is
 *                      shared
 *   t_posix cloexec    INV-0103-4: FD_CLOEXEC is absent in a spawned child,
 *                      an fd without it shares its description
 *   t_posix execwait   INV-0104-2: a child that execs is waited on once,
 *                      with the new image's exit
 *   t_posix groups     INV-0104-3: kill(pid) and kill(-pgid) reach exactly
 *                      their job
 *   t_posix tcsetpgrp  INV-0105-2: dash's two tcsetpgrp calls
 *   t_posix statuses   INV-0106-1: SIGINT, SIGSEGV, SIGKILL, exit 130
 *   t_posix blocked    INV-0106-4: a blocked signal with a handler, with
 *                      SIG_IGN, with SIG_DFL
 *   t_posix raise      INV-0106-5: raise and kill of oneself with SIG_DFL
 *   t_posix execmask   ADR-0190 item 18: SIG_IGN and the mask survive
 *                      execve, and posix_spawn's SETSIGMASK applies: SIGINT
 *                      ignored survives kill, SIGTERM blocked stays held
 *                      until the new image unblocks it
 *   t_posix eintr      INV-0106-3, INV-0006-7: pselect, sigsuspend and
 *                      nanosleep give EINTR only after a handler; an
 *                      ignored or blocked signal does not wake pselect; a
 *                      read with SA_RESTART resumes
 *   t_posix files      a file workload (INV-0104-5, INV-0106-2 with
 *                      ZORAYA_POSIX_STATS: broker=0 takes=0)
 *   t_posix time       INV-0191-1: a file in /tmp after one on the root volume is
 *                      later, and CLOCK_REALTIME is not below either
 *   t_posix intcount NAME
 *                      counts SIGINT until SIGTERM or one SIGINT
 *                      (INV-0105-1, run from the script)
 *   t_posix hostile    ignores every catchable signal, then sleeps
 *   t_posix forkbomb N forks until refused, N at most; reports the count
 *   t_posix exit7      exits 7
 *   t_posix bench      the measurements of TASK-21: posix_spawn and wait of
 *                      a program exiting at once, fork and wait, fork and
 *                      exec and wait (run with ZORAYA_POSIX_STATS for the
 *                      broker's round trips)
 */
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <zoraya/posix.h>

extern char **environ;

/* argv[0] is what the shell found on $PATH: the path is this one. */
#define SELF "/bin/t_posix"

static const char *test = "t_posix";

#define CHECK(cond)                                                          \
	do {                                                                 \
		if (!(cond)) {                                               \
			printf("t_posix: %s: FAIL line %d: %s (errno %d: %s)\n", \
			       test, __LINE__, #cond, errno, strerror(errno));   \
			fflush(stdout);                                      \
			exit(1);                                             \
		}                                                            \
	} while (0)

static void passed(void)
{
	printf("t_posix: %s passed\n", test);
	fflush(stdout);
}

static void pause_ms(long ms)
{
	struct timespec t = { ms / 1000, (ms % 1000) * 1000000 };
	while (nanosleep(&t, &t) == -1 && errno == EINTR)
		;
}

/* Waits for `pid`, and gives its status. */
static int reap(pid_t pid)
{
	int status = -1;
	pid_t got;
	do
		got = waitpid(pid, &status, 0);
	while (got == -1 && errno == EINTR);
	CHECK(got == pid);
	return status;
}

static volatile sig_atomic_t counted;
static volatile sig_atomic_t done;

static void count(int sig)
{
	(void)sig;
	counted++;
}

static void finish(int sig)
{
	(void)sig;
	done = 1;
}

static void handle(int sig, void (*f)(int), int flags)
{
	struct sigaction a;
	memset(&a, 0, sizeof a);
	a.sa_handler = f;
	a.sa_flags = flags;
	sigemptyset(&a.sa_mask);
	CHECK(sigaction(sig, &a, NULL) == 0);
}

static void forkpos(void)
{
	const char *name = "/tmp/forkpos";
	int fd = open(name, O_RDWR | O_CREAT | O_TRUNC, 0600);
	CHECK(fd >= 0);
	CHECK(write(fd, "abcdef", 6) == 6);
	CHECK(lseek(fd, 0, SEEK_SET) == 0);
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		char b[3];
		_exit(read(fd, b, 3) == 3 && memcmp(b, "abc", 3) == 0 ? 0 : 1);
	}
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	char b[3];
	CHECK(read(fd, b, 3) == 3);
	CHECK(memcmp(b, "def", 3) == 0);
	CHECK(lseek(fd, 0, SEEK_CUR) == 6);
	close(fd);
	unlink(name);
	passed();
}

/* The spawned half of cloexec: fd 3 must not exist, fd 4 reads one byte. */
static int cloexec_child(void)
{
	if (fcntl(3, F_GETFD) != -1 || errno != EBADF)
		return 2;
	char b;
	if (read(4, &b, 1) != 1 || b != 'x')
		return 3;
	return 0;
}

static void cloexec(char *self)
{
	int a = open("/tmp/cloexec-a", O_RDWR | O_CREAT | O_TRUNC, 0600);
	int b = open("/tmp/cloexec-b", O_RDWR | O_CREAT | O_TRUNC, 0600);
	CHECK(a >= 0 && b >= 0);
	CHECK(dup2(a, 3) == 3 && dup2(b, 4) == 4);
	CHECK(write(4, "xyz", 3) == 3 && lseek(4, 0, SEEK_SET) == 0);
	CHECK(fcntl(3, F_SETFD, FD_CLOEXEC) == 0);
	CHECK(fcntl(4, F_SETFD, 0) == 0);
	char *argv[] = { self, "cloexec-child", NULL };
	pid_t pid;
	int r = posix_spawn(&pid, self, NULL, NULL, argv, environ);
	errno = r;
	CHECK(r == 0);
	int status = reap(pid);
	CHECK(WIFEXITED(status));
	CHECK(WEXITSTATUS(status) == 0);
	/* The child's read moved the position of the one description. */
	CHECK(lseek(4, 0, SEEK_CUR) == 1);
	unlink("/tmp/cloexec-a");
	unlink("/tmp/cloexec-b");
	passed();
}

/* sigismember is outside the tier list (ADR-0141): musl's set, bit n - 1. */
static int in_set(const sigset_t *set, int n)
{
	return (int)((((const unsigned long *)set)[0] >> (n - 1)) & 1);
}

/* The image after exec (or posix_spawn): fd 3 reads "go", fd 4 writes. */
static int execmask_child(void)
{
	sigset_t now;
	CHECK(sigprocmask(SIG_BLOCK, NULL, &now) == 0);
	CHECK(in_set(&now, SIGTERM) == 1);
	CHECK(in_set(&now, SIGINT) == 0);
	struct sigaction a;
	CHECK(sigaction(SIGINT, NULL, &a) == 0);
	CHECK(a.sa_handler == SIG_IGN);
	/* A handler of this image, held by the mask it came with (a blocked
	 * SIG_DFL ends the process at once, ADR-0106 item 2). */
	counted = 0;
	handle(SIGTERM, count, 0);
	char c;
	CHECK(write(4, "r", 1) == 1);
	/* The parent sends SIGINT and SIGTERM, then "g". */
	CHECK(read(3, &c, 1) == 1);
	pause_ms(50);
	CHECK(counted == 0);
	sigset_t term;
	sigemptyset(&term);
	sigaddset(&term, SIGTERM);
	CHECK(sigprocmask(SIG_UNBLOCK, &term, NULL) == 0);
	CHECK(counted == 1);
	CHECK(write(4, "a", 1) == 1);
	return 0;
}

static void execmask_run(pid_t pid, int go, int back)
{
	char c;
	CHECK(read(back, &c, 1) == 1 && c == 'r');
	CHECK(kill(pid, SIGINT) == 0);
	CHECK(kill(pid, SIGTERM) == 0);
	CHECK(write(go, "g", 1) == 1);
	CHECK(read(back, &c, 1) == 1 && c == 'a');
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
}

static void execmask(char *self)
{
	char *argv[] = { self, "execmask-child", NULL };
	sigset_t term;
	sigemptyset(&term);
	sigaddset(&term, SIGTERM);
	int go[2], back[2];
	/* execve after fork: the child's own SIG_IGN and mask. */
	CHECK(pipe(go) == 0 && pipe(back) == 0);
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		signal(SIGINT, SIG_IGN);
		sigprocmask(SIG_BLOCK, &term, NULL);
		dup2(go[0], 3);
		dup2(back[1], 4);
		execve(self, argv, environ);
		_exit(99);
	}
	close(go[0]);
	close(back[1]);
	execmask_run(pid, go[1], back[0]);
	close(go[1]);
	close(back[0]);
	/* posix_spawn: the parent's SIG_IGN, SETSIGMASK's mask. */
	CHECK(pipe(go) == 0 && pipe(back) == 0);
	struct sigaction a, old;
	memset(&a, 0, sizeof a);
	a.sa_handler = SIG_IGN;
	CHECK(sigaction(SIGINT, &a, &old) == 0);
	posix_spawnattr_t attr;
	posix_spawn_file_actions_t fa;
	CHECK(posix_spawnattr_init(&attr) == 0);
	CHECK(posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGMASK) == 0);
	CHECK(posix_spawnattr_setsigmask(&attr, &term) == 0);
	CHECK(posix_spawn_file_actions_init(&fa) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, go[0], 3) == 0);
	CHECK(posix_spawn_file_actions_adddup2(&fa, back[1], 4) == 0);
	CHECK(posix_spawn(&pid, self, &fa, &attr, argv, environ) == 0);
	CHECK(sigaction(SIGINT, &old, NULL) == 0);
	close(go[0]);
	close(back[1]);
	execmask_run(pid, go[1], back[0]);
	close(go[1]);
	close(back[0]);
	/* The parent's own mask was never touched. */
	sigset_t now;
	CHECK(sigprocmask(SIG_BLOCK, NULL, &now) == 0);
	CHECK(in_set(&now, SIGTERM) == 0);
	passed();
}

static void execwait(char *self)
{
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		char *argv[] = { self, "exit7", NULL };
		execve(self, argv, environ);
		_exit(99);
	}
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 7);
	/* Once: the child is gone. */
	CHECK(waitpid(pid, &status, WNOHANG) == -1 && errno == ECHILD);
	/* A script through #!: the interpreter runs it. */
	int fd = open("/tmp/script.sh", O_WRONLY | O_CREAT | O_TRUNC, 0755);
	CHECK(fd >= 0);
	const char *text = "#!/bin/sh\nexit 5\n";
	CHECK(write(fd, text, strlen(text)) == (ssize_t)strlen(text));
	close(fd);
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		char *argv[] = { "/tmp/script.sh", NULL };
		execve("/tmp/script.sh", argv, environ);
		_exit(98);
	}
	status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 5);
	unlink("/tmp/script.sh");
	passed();
}

/* A member of a group for `groups`: counts SIGUSR1 until SIGTERM, then
 * exits with the count. `ready` gets a byte once the handlers are in. */
static void member(int ready)
{
	handle(SIGUSR1, count, 0);
	handle(SIGTERM, finish, 0);
	CHECK(write(ready, "r", 1) == 1);
	sigset_t none;
	sigemptyset(&none);
	while (!done)
		sigsuspend(&none);
	_exit(counted);
}

static pid_t start_member(int group, int ready)
{
	CHECK(zoraya_fork_group(group) == 0);
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0)
		member(ready);
	return pid;
}

static void groups(void)
{
	int p[2];
	CHECK(pipe(p) == 0);
	pid_t a1 = start_member(ZORAYA_FORK_NEW, p[1]);
	pid_t a2 = start_member(a1, p[1]);
	pid_t b1 = start_member(ZORAYA_FORK_NEW, p[1]);
	pid_t b2 = start_member(b1, p[1]);
	char r[4];
	for (int got = 0; got < 4;) {
		ssize_t n = read(p[0], r, 4 - got);
		CHECK(n > 0);
		got += n;
	}
	/* The groups are what the hints said (setpgid would only confirm). */
	CHECK(setpgid(a2, a1) == 0);
	CHECK(setpgid(b2, b1) == 0);
	CHECK(setpgid(b2, a1) == -1 && errno == EPERM);
	CHECK(kill(a1, SIGUSR1) == 0);
	pause_ms(100);
	CHECK(kill(-a1, SIGUSR1) == 0);
	pause_ms(100);
	CHECK(killpg(b1, SIGUSR1) == 0);
	pause_ms(100);
	CHECK(kill(b2, SIGUSR1) == 0);
	pause_ms(100);
	pid_t all[4] = { a1, a2, b1, b2 };
	int want[4] = { 2, 1, 1, 2 };
	for (int i = 0; i < 4; i++)
		CHECK(kill(all[i], SIGTERM) == 0);
	for (int i = 0; i < 4; i++) {
		int status = reap(all[i]);
		CHECK(WIFEXITED(status));
		if (WEXITSTATUS(status) != want[i]) {
			printf("t_posix: groups: member %d took %d, wanted %d\n", i,
			       WEXITSTATUS(status), want[i]);
			CHECK(0);
		}
	}
	/* An unknown pid. */
	CHECK(kill(99999, 0) == -1 && errno == ESRCH);
	passed();
}

static void tcsetpgrp_calls(void)
{
	pid_t shell = tcgetpgrp(0);
	CHECK(shell > 0);
	CHECK(zoraya_fork_group(ZORAYA_FORK_NEW) == 0);
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		/* dash's forkchild: setpgid, then the terminal for its group. */
		if (setpgid(0, 0) != 0)
			_exit(2);
		if (tcsetpgrp(0, getpgrp()) != 0)
			_exit(3);
		if (tcgetpgrp(0) != getpid())
			_exit(4);
		_exit(0);
	}
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	/* dash's waitforjob: the terminal back to the shell's group. */
	CHECK(tcsetpgrp(0, getpgrp()) == 0);
	CHECK(tcgetpgrp(0) == getpgrp());
	/* A group outside the session. */
	CHECK(tcsetpgrp(0, 99999) == -1 && errno == EPERM);
	passed();
}

static void statuses(void)
{
	int status;
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		raise(SIGINT);
		_exit(1);
	}
	status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGINT);
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		*(volatile int *)16 = 1;
		_exit(1);
	}
	status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV);
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		for (;;)
			pause_ms(1000);
	}
	CHECK(kill(pid, SIGKILL) == 0);
	status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGKILL);
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0)
		exit(130);
	status = reap(pid);
	CHECK(WIFEXITED(status) && !WIFSIGNALED(status) && WEXITSTATUS(status) == 130);
	passed();
}

static void blocked(void)
{
	sigset_t term, none;
	sigemptyset(&term);
	sigaddset(&term, SIGTERM);
	sigemptyset(&none);
	/* A handler: held while blocked, run once on unblock. */
	counted = 0;
	handle(SIGTERM, count, 0);
	CHECK(sigprocmask(SIG_BLOCK, &term, NULL) == 0);
	pid_t me = getpid();
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0)
		_exit(kill(me, SIGTERM) == 0 ? 0 : 1);
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	pause_ms(50);
	CHECK(counted == 0);
	CHECK(sigprocmask(SIG_SETMASK, &none, NULL) == 0);
	CHECK(counted == 1);
	pause_ms(50);
	CHECK(counted == 1);
	/* SIG_IGN: nothing, then or later. */
	handle(SIGTERM, SIG_IGN, 0);
	CHECK(sigprocmask(SIG_BLOCK, &term, NULL) == 0);
	CHECK(kill(me, SIGTERM) == 0);
	CHECK(sigprocmask(SIG_SETMASK, &none, NULL) == 0);
	CHECK(counted == 1);
	/* SIG_DFL, blocked: the process ends at once (the stated deviation). */
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		signal(SIGTERM, SIG_DFL);
		sigprocmask(SIG_BLOCK, &term, NULL);
		for (;;)
			pause_ms(1000);
	}
	pause_ms(100);
	CHECK(kill(pid, SIGTERM) == 0);
	status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGTERM);
	handle(SIGTERM, SIG_DFL, 0);
	passed();
}

static void raise_self(void)
{
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		raise(SIGUSR1);
		_exit(1);
	}
	int status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGUSR1);
	pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		kill(getpid(), SIGUSR2);
		_exit(1);
	}
	status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGUSR2);
	passed();
}

/* A child that sends `sig` to `to` after `ms`, then writes a byte to `fd`
 * if it is not -1. */
static pid_t later(pid_t to, int sig, long ms, int fd)
{
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		pause_ms(ms);
		kill(to, sig);
		if (fd >= 0) {
			pause_ms(ms);
			if (write(fd, "w", 1) != 1)
				_exit(2);
		}
		_exit(0);
	}
	return pid;
}

static void eintr(void)
{
	pid_t me = getpid();
	int p[2];
	CHECK(pipe(p) == 0);
	fd_set r;
	sigset_t none, usr1;
	sigemptyset(&none);
	sigemptyset(&usr1);
	sigaddset(&usr1, SIGUSR1);
	/* Ignored: pselect is not woken, it returns with the fd ready. */
	handle(SIGUSR1, SIG_IGN, 0);
	pid_t pid = later(me, SIGUSR1, 100, p[1]);
	FD_ZERO(&r);
	FD_SET(p[0], &r);
	CHECK(pselect(p[0] + 1, &r, NULL, NULL, NULL, &none) == 1);
	CHECK(FD_ISSET(p[0], &r));
	char b;
	CHECK(read(p[0], &b, 1) == 1);
	reap(pid);
	/* Blocked with a handler: not woken; the handler runs on unblock. */
	counted = 0;
	handle(SIGUSR1, count, 0);
	CHECK(sigprocmask(SIG_BLOCK, &usr1, NULL) == 0);
	pid = later(me, SIGUSR1, 100, p[1]);
	FD_ZERO(&r);
	FD_SET(p[0], &r);
	CHECK(pselect(p[0] + 1, &r, NULL, NULL, NULL, &usr1) == 1);
	CHECK(counted == 0);
	CHECK(read(p[0], &b, 1) == 1);
	reap(pid);
	CHECK(sigprocmask(SIG_SETMASK, &none, NULL) == 0);
	CHECK(counted == 1);
	/* A handler, unblocked: EINTR, after it ran. */
	counted = 0;
	pid = later(me, SIGUSR1, 100, -1);
	FD_ZERO(&r);
	FD_SET(p[0], &r);
	errno = 0;
	CHECK(pselect(p[0] + 1, &r, NULL, NULL, NULL, &none) == -1 && errno == EINTR);
	CHECK(counted == 1);
	reap(pid);
	/* sigsuspend: EINTR after the handler. */
	counted = 0;
	CHECK(sigprocmask(SIG_BLOCK, &usr1, NULL) == 0);
	pid = later(me, SIGUSR1, 100, -1);
	CHECK(sigsuspend(&none) == -1 && errno == EINTR);
	CHECK(counted == 1);
	CHECK(sigprocmask(SIG_SETMASK, &none, NULL) == 0);
	reap(pid);
	/* nanosleep: EINTR with time left. */
	counted = 0;
	pid = later(me, SIGUSR1, 100, -1);
	struct timespec t = { 5, 0 };
	CHECK(nanosleep(&t, &t) == -1 && errno == EINTR);
	CHECK(counted == 1 && t.tv_sec >= 3);
	reap(pid);
	/* A read with SA_RESTART resumes and completes. */
	counted = 0;
	handle(SIGUSR1, count, SA_RESTART);
	pid = later(me, SIGUSR1, 100, p[1]);
	CHECK(read(p[0], &b, 1) == 1 && b == 'w');
	CHECK(counted == 1);
	reap(pid);
	/* Without it, EINTR. */
	counted = 0;
	handle(SIGUSR1, count, 0);
	pid = later(me, SIGUSR1, 100, p[1]);
	CHECK(read(p[0], &b, 1) == -1 && errno == EINTR);
	CHECK(counted == 1);
	CHECK(read(p[0], &b, 1) == 1);
	reap(pid);
	handle(SIGUSR1, SIG_DFL, 0);
	passed();
}

static void files(void)
{
	int p[2];
	char b[64];
	for (int i = 0; i < 20; i++) {
		int fd = open("/tmp/work", O_RDWR | O_CREAT | O_TRUNC, 0600);
		CHECK(fd >= 0);
		CHECK(write(fd, "data", 4) == 4);
		CHECK(lseek(fd, 0, SEEK_SET) == 0);
		CHECK(read(fd, b, 4) == 4);
		struct stat st;
		CHECK(fstat(fd, &st) == 0 && st.st_size == 4);
		CHECK(stat("/home", &st) == 0);
		CHECK(close(fd) == 0);
		CHECK(pipe(p) == 0);
		CHECK(write(p[1], "x", 1) == 1 && read(p[0], b, 1) == 1);
		close(p[0]);
		close(p[1]);
	}
	CHECK(malloc(1 << 20) != NULL);
	unlink("/tmp/work");
	passed();
}

/* The mtime of `path` in nanoseconds. */
static long long mtime_of(const char *path)
{
	struct stat st;
	CHECK(stat(path, &st) == 0);
	return (long long)st.st_mtim.tv_sec * 1000000000LL + st.st_mtim.tv_nsec;
}

static void make_file(const char *path)
{
	int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	CHECK(write(fd, "t", 1) == 1);
	CHECK(close(fd) == 0);
}

static void time_base(void)
{
	make_file("/src/time-a");
	make_file("/tmp/time-b");
	make_file("/src/time-c");
	long long a = mtime_of("/src/time-a");
	long long b = mtime_of("/tmp/time-b");
	long long c = mtime_of("/src/time-c");
	struct timespec now;
	CHECK(clock_gettime(CLOCK_REALTIME, &now) == 0);
	long long n = (long long)now.tv_sec * 1000000000LL + now.tv_nsec;
	printf("t_posix: time: home %lld tmp %lld home %lld now %lld\n", a, b, c, n);
	CHECK(b > a);
	CHECK(c > b);
	CHECK(n >= c && n >= b);
	/* A date after 2020: the root volume's, not the boot's uptime. */
	CHECK(now.tv_sec > 1577836800);
	unlink("/src/time-a");
	unlink("/src/time-c");
	unlink("/tmp/time-b");
	passed();
}

static void intcount(const char *name)
{
	handle(SIGINT, count, 0);
	handle(SIGTERM, finish, 0);
	fprintf(stderr, "t_posix: %s ready\n", name);
	sigset_t none;
	sigemptyset(&none);
	while (!done && counted == 0)
		sigsuspend(&none);
	fprintf(stderr, "t_posix: %s interrupts=%d\n", name, (int)counted);
	exit(0);
}

static void hostile(void)
{
	for (int s = 1; s < 32; s++)
		if (s != SIGKILL && s != SIGSTOP)
			signal(s, SIG_IGN);
	fprintf(stderr, "t_posix: hostile ignoring every signal\n");
	for (;;)
		pause_ms(1000);
}

static void forkbomb(int limit)
{
	int made = 0;
	pid_t kids[512];
	while (made < limit && made < 512) {
		pid_t pid = fork();
		if (pid == 0) {
			for (;;)
				pause_ms(1000);
		}
		if (pid < 0)
			break;
		kids[made++] = pid;
	}
	int e = errno;
	printf("t_posix: forkbomb made %d before %s\n", made, made < limit ? strerror(e) : "the limit");
	for (int i = 0; i < made; i++)
		kill(kids[i], SIGKILL);
	for (int i = 0; i < made; i++)
		reap(kids[i]);
	/* And the session goes on: one more fork works. */
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0)
		_exit(0);
	reap(pid);
	passed();
}

static double now_us(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec * 1e6 + t.tv_nsec / 1e3;
}

static void bench(void)
{
	const int n = 20;
	char *argv[] = { SELF, "exit7", NULL };
	double t0 = now_us();
	for (int i = 0; i < n; i++) {
		pid_t pid;
		CHECK(posix_spawn(&pid, SELF, NULL, NULL, argv, environ) == 0);
		reap(pid);
	}
	double spawn = (now_us() - t0) / n;
	t0 = now_us();
	for (int i = 0; i < n; i++) {
		pid_t pid = fork();
		CHECK(pid >= 0);
		if (pid == 0)
			_exit(0);
		reap(pid);
	}
	double forked = (now_us() - t0) / n;
	t0 = now_us();
	for (int i = 0; i < n; i++) {
		pid_t pid = fork();
		CHECK(pid >= 0);
		if (pid == 0) {
			execve(SELF, argv, environ);
			_exit(99);
		}
		CHECK(WEXITSTATUS(reap(pid)) == 7);
	}
	double execed = (now_us() - t0) / n;
	printf("t_posix: bench spawn+wait %.0f us, fork+wait %.0f us, fork+exec+wait %.0f us, exec %.0f us\n",
	       spawn, forked, execed, execed - forked);
	passed();
}

int main(int argc, char **argv)
{
	if (argc < 2)
		return 2;
	test = argv[1];
	if (!strcmp(argv[1], "forkpos"))
		forkpos();
	else if (!strcmp(argv[1], "cloexec"))
		cloexec(SELF);
	else if (!strcmp(argv[1], "cloexec-child"))
		return cloexec_child();
	else if (!strcmp(argv[1], "execwait"))
		execwait(SELF);
	else if (!strcmp(argv[1], "groups"))
		groups();
	else if (!strcmp(argv[1], "tcsetpgrp"))
		tcsetpgrp_calls();
	else if (!strcmp(argv[1], "statuses"))
		statuses();
	else if (!strcmp(argv[1], "blocked"))
		blocked();
	else if (!strcmp(argv[1], "execmask"))
		execmask(SELF);
	else if (!strcmp(argv[1], "execmask-child"))
		return execmask_child();
	else if (!strcmp(argv[1], "raise"))
		raise_self();
	else if (!strcmp(argv[1], "eintr"))
		eintr();
	else if (!strcmp(argv[1], "files"))
		files();
	else if (!strcmp(argv[1], "time"))
		time_base();
	else if (!strcmp(argv[1], "intcount") && argc > 2)
		intcount(argv[2]);
	else if (!strcmp(argv[1], "hostile"))
		hostile();
	else if (!strcmp(argv[1], "forkbomb") && argc > 2)
		forkbomb(atoi(argv[2]));
	else if (!strcmp(argv[1], "exit7"))
		return 7;
	else if (!strcmp(argv[1], "bench"))
		bench();
	else
		return 2;
	return 0;
}
