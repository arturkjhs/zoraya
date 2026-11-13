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
 *   t_posix diskfill DIR OUT
 *                      what `dd` into a limited directory does (ADR-0265, Q5;
 *                      the ports have no dd): writes 4 KiB blocks to DIR/fill
 *                      until a write fails, which must be EDQUOT; unlink
 *                      works at the limit; the space is back; a directory
 *                      does not move out of DIR into OUT (EXDEV) and a file
 *                      does not fit back in
 *   t_posix products   ADR-0210's functions the workloads' own
 *                      programs need: chmod, rmdir, symlink, utimes, fsync,
 *                      gethostname, getrusage, mmap, popen and pclose, system,
 *                      and each of its names that is musl's own code
 *   t_posix live N     W9's "no process left" (ADR-0110 item 4): prints
 *                      W9-LEFT and the count of live pids from 3 to N other
 *                      than itself and its parent, by kill(pid, 0) (the
 *                      broker's table: ESRCH once a pid is reaped; pids are
 *                      never given twice)
 *   t_posix exit7      exits 7
 *   t_posix tstp       INV-0200-2: catches SIGTSTP; on it says that it
 *                      restores the terminal, then stops with the default
 *                      action; continued, it catches it again and says so
 *   t_posix notstp     INV-0200-2's other half: ignores SIGTSTP, so a
 *                      Ctrl-Z does not stop it
 *   t_posix tfork      INV-0306-1, -2, -3: fork with running siblings (threads
 *                      holding malloc, stdio and the library, and two
 *                      counters on different pages): the child has one thread,
 *                      mallocs, prints, opens, makes a thread of its own, and
 *                      sees the counters as of one instant
 *   t_posix tforkthread
 *                      fork in a non-main thread; pthread_atfork handlers in
 *                      their order; the child's pthread_self and join
 *   t_posix texec      INV-0306-4: exec from a non-main thread while a
 *                      sibling opens files; the new image has only the
 *                      inherited descriptors and is waited for once
 *   t_posix tspawn     posix_spawn and vfork from several threads at once
 *   t_posix tsig       sigwait in a thread woken by kill(getpid()); sigtimedwait
 *                      times out (EAGAIN); kill(getpid(), n) goes to a thread
 *                      that has n unblocked, or stays pending until one does
 *   t_posix treroute   INV-0304-7: 1000 process signals routed to the one thread
 *                      that has them unblocked while another sits in sem_wait
 *   t_posix twait      INV-0302-2: waitpid in a thread while another thread
 *                      makes library calls
 *   t_posix bench      the measurements of TASK-21: posix_spawn and wait of
 *                      a program exiting at once, fork and wait, fork and
 *                      exec and wait (run with ZORAYA_POSIX_STATS for the
 *                      broker's round trips)
 */
#include <ctype.h>
#include <errno.h>
#include <glob.h>
#include <locale.h>
#include <math.h>
#include <pthread.h>
#include <pwd.h>
#include <semaphore.h>
#include <fcntl.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/select.h>
#include <sys/time.h>
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

static void live(int last)
{
	int n = 0;
	for (int pid = 3; pid <= last; pid++) {
		if (pid == getpid() || pid == getppid())
			continue;
		if (kill(pid, 0) == 0)
			n++;
		else
			CHECK(errno == ESRCH);
	}
	printf("W9-LEFT %d\n", n);
	fflush(stdout);
}

/* ADR-0210's names that are musl's or the Rust half's own code: each called
 * once with a result checked (the architect's condition on ADR-0210). */
static int near(double a, double b)
{
	return fabs(a - b) < 1e-9;
}

static void products_musl(void)
{
	volatile double one = 1.0, two = 2.0, half = 0.5;
	int e;
	CHECK(abs(-3) == 3);
	CHECK(near(acos(one), 0.0) && near(asin(0.0 * one), 0.0));
	CHECK(near(atan2(one, one), M_PI / 4) && near(cos(0.0 * one), 1.0));
	CHECK(near(sin(M_PI / 2 * one), 1.0) && near(tan(0.0 * one), 0.0));
	CHECK(ceil(1.2 * one) == 2.0 && floor(1.8 * one) == 1.0 && fabs(-two) == 2.0);
	CHECK(near(exp(0.0 * one), 1.0) && near(log(exp(two)), 2.0));
	CHECK(near(log10(1000.0 * one), 3.0) && near(log2(8.0 * one), 3.0));
	CHECK(near(pow(two, 10.0), 1024.0) && near(sqrt(16.0 * one), 4.0));
	CHECK(near(fmod(7.5 * one, two), 1.5));
	CHECK(near(frexp(8.0 * one, &e), half) && e == 4 && ldexp(half, 4) == 8.0);
	CHECK(isblank(' ') && isblank('\t') && !isblank('x'));
	CHECK(difftime(10, 4) == 6.0);
	time_t t = 86400 + 3661;
	struct tm tm;
	CHECK(gmtime_r(&t, &tm) == &tm && tm.tm_mday == 2 && tm.tm_hour == 1 && tm.tm_min == 1);
	CHECK(clock() != (clock_t)-1);
	struct lconv *lc = localeconv();
	CHECK(lc != NULL && !strcmp(lc->decimal_point, "."));
	CHECK(setenv("P_VAR", "x", 1) == 0 && getenv("P_VAR") != NULL);
	CHECK(unsetenv("P_VAR") == 0 && getenv("P_VAR") == NULL);
	struct passwd *pw = getpwuid(getuid());
	CHECK(pw != NULL && !strcmp(pw->pw_dir, "/home"));
	FILE *f = fopen("/tmp/p-stdio", "w+");
	CHECK(f != NULL && fputs("ab", f) >= 0);
	rewind(f);
	flockfile(f);
	CHECK(getc(f) == 'a');
	funlockfile(f);
	CHECK(ungetc('z', f) == 'z' && getc(f) == 'z' && ftello(f) == 1);
	CHECK(getc(f) == 'b' && getc(f) == EOF && feof(f));
	clearerr(f);
	CHECK(!feof(f) && fclose(f) == 0);
	glob_t g;
	CHECK(glob("/tmp/p-std*", 0, NULL, &g) == 0 && g.gl_pathc == 1);
	CHECK(!strcmp(g.gl_pathv[0], "/tmp/p-stdio"));
	globfree(&g);
	CHECK(unlink("/tmp/p-stdio") == 0);
}

/* ADR-0210: what W2's and W3's programs call beyond the tier list. */
static void products(void)
{
	struct stat st;
	char b[64];
	unlink("/tmp/p-link");
	rmdir("/tmp/p-dir");
	int fd = open("/tmp/p-file", O_RDWR | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	CHECK(fsync(fd) == 0);
	CHECK(write(fd, "data", 4) == 4);
	CHECK(fsync(fd) == 0);
	CHECK(close(fd) == 0);
	int p[2];
	CHECK(pipe(p) == 0);
	CHECK(fsync(p[0]) == -1 && errno == EINVAL);
	close(p[0]);
	close(p[1]);
	CHECK(chmod("/tmp/p-file", 0600) == 0);
	CHECK(stat("/tmp/p-file", &st) == 0 && (st.st_mode & 07777) == 0600);
	struct timeval tv[2] = {{1000000, 0}, {1234567, 500000}};
	CHECK(utimes("/tmp/p-file", tv) == 0);
	CHECK(stat("/tmp/p-file", &st) == 0 && st.st_mtim.tv_sec == 1234567 &&
	      st.st_mtim.tv_nsec == 500000000);
	tv[1].tv_usec = 1000000;
	CHECK(utimes("/tmp/p-file", tv) == -1 && errno == EINVAL);
	CHECK(symlink("p-file", "/tmp/p-link") == 0);
	CHECK(readlink("/tmp/p-link", b, sizeof b) == 6 && !memcmp(b, "p-file", 6));
	CHECK(mkdir("/tmp/p-dir", 0755) == 0);
	CHECK(rmdir("/tmp/p-dir") == 0);
	CHECK(rmdir("/tmp/p-dir") == -1 && errno == ENOENT);
	CHECK(rmdir("/tmp/p-file") == -1 && errno == ENOTDIR);
	CHECK(unlink("/tmp/p-link") == 0 && unlink("/tmp/p-file") == 0);
	CHECK(gethostname(b, sizeof b) == 0 && !strcmp(b, "zoraya"));
	CHECK(gethostname(b, 3) == 0 && !strcmp(b, "zo"));
	struct rusage ru;
	memset(&ru, 0xff, sizeof ru);
	CHECK(getrusage(RUSAGE_SELF, &ru) == 0 && ru.ru_maxrss == 0 && ru.ru_utime.tv_sec == 0);
	CHECK(getrusage(1, &ru) == 0); /* RUSAGE_THREAD */
	CHECK(getrusage(5, &ru) == -1 && errno == EINVAL);
	char *m = mmap(0, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	CHECK(m != MAP_FAILED && m[0] == 0 && m[8191] == 0);
	m[100] = 7;
	CHECK(munmap(m, 8192) == 0);
	CHECK(mmap(0, 4096, PROT_READ, MAP_SHARED | MAP_ANONYMOUS, -1, 0) == MAP_FAILED);
	FILE *f = popen("echo through-popen; exit 3", "r");
	CHECK(f != NULL);
	CHECK(fgets(b, sizeof b, f) != NULL && !strcmp(b, "through-popen\n"));
	int st2 = pclose(f);
	CHECK(WIFEXITED(st2) && WEXITSTATUS(st2) == 3);
	st2 = system("exit 5");
	CHECK(WIFEXITED(st2) && WEXITSTATUS(st2) == 5);
	CHECK(system(NULL) == 1);
	products_musl();
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

static void diskfill(const char *dir, const char *out)
{
	char path[256], sub[256], dest[256];
	snprintf(path, sizeof path, "%s/fill", dir);
	snprintf(sub, sizeof sub, "%s/sub", dir);
	snprintf(dest, sizeof dest, "%s/diskfill-sub", out);
	static char block[4096];
	memset(block, 'z', sizeof block);
	int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	int blocks = 0, e = 0;
	for (;;) {
		ssize_t n = write(fd, block, sizeof block);
		if (n < 0) {
			e = errno;
			break;
		}
		if (n == 0 || blocks >= 100000)
			break;
		blocks++;
	}
	printf("t_posix: diskfill wrote %d blocks before %s\n", blocks, e ? strerror(e) : "the end");
	CHECK(e == EDQUOT);
	CHECK(blocks > 0);
	/* The file holds what was written, and the next write is refused again. */
	struct stat st;
	CHECK(fstat(fd, &st) == 0);
	CHECK(st.st_size == (off_t)blocks * 4096);
	CHECK(write(fd, block, sizeof block) < 0 && errno == EDQUOT);
	CHECK(close(fd) == 0);
	/* A new name is refused too: the limit counts the object's block. */
	char other[256];
	snprintf(other, sizeof other, "%s/other", dir);
	int fd2 = open(other, O_WRONLY | O_CREAT | O_EXCL, 0644);
	if (fd2 >= 0) {
		/* There was room for the object but none for its data. */
		CHECK(write(fd2, block, sizeof block) < 0 && errno == EDQUOT);
		close(fd2);
		CHECK(unlink(other) == 0);
	} else {
		CHECK(errno == EDQUOT);
	}
	/* rm works at the limit. */
	CHECK(unlink(path) == 0);
	/* A directory does not cross the boundary; mv would copy it instead. */
	CHECK(mkdir(sub, 0755) == 0);
	CHECK(rename(sub, dest) < 0 && errno == EXDEV);
	CHECK(rmdir(sub) == 0);
	/* And the space is back. */
	fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	CHECK(write(fd, block, sizeof block) == (ssize_t)sizeof block);
	CHECK(close(fd) == 0);
	CHECK(unlink(path) == 0);
	passed();
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

/* INV-0200-2: what an editor does on Ctrl-Z, put the terminal back, then
 * stop. Only async-signal-safe calls in the handler. */
static void on_tstp(int sig)
{
	static const char restoring[] = "tstp: restoring the terminal\n";
	static const char continued[] = "tstp: continued\n";
	(void)sig;
	sigset_t set;
	write(2, restoring, sizeof restoring - 1);
	/* The default action, unblocked: the stop happens here. */
	signal(SIGTSTP, SIG_DFL);
	sigemptyset(&set);
	sigaddset(&set, SIGTSTP);
	sigprocmask(SIG_UNBLOCK, &set, NULL);
	raise(SIGTSTP);
	signal(SIGTSTP, on_tstp);
	write(2, continued, sizeof continued - 1);
}

static void tstp(void)
{
	static const char ready[] = "tstp: ready\n";
	signal(SIGTSTP, on_tstp);
	write(2, ready, sizeof ready - 1);
	for (;;)
		pause_ms(1000);
}

/* INV-0200-2: a program Ctrl-Z does not stop. */
static void notstp(void)
{
	static const char ready[] = "notstp: ready\n";
	signal(SIGTSTP, SIG_IGN);
	write(2, ready, sizeof ready - 1);
	for (;;)
		pause_ms(1000);
}

/* ------------------------------------------------------------ threads
 * Part 9b: fork and exec in a process with threads (ADR-0306). These need
 * the broker, so they are here and not in t_threads. */

static volatile int stop_siblings;
/* Each on a page of its own, so that a copy in steps could tear them. */
static volatile unsigned long counter_a __attribute__((aligned(16384)));
static volatile unsigned long counter_b __attribute__((aligned(16384)));
static volatile unsigned long seq __attribute__((aligned(16384)));
/* The mapper's published region (INV-0306-6): the child may touch it only
 * if the copy has it, which it does only if the mapping existed at the
 * instant of the fork. */
static volatile unsigned char *volatile region;
static volatile unsigned char region_byte;

/* Takes in turn: the heap, the library (open, close), stdout's lock, and
 * counts on two pages. */
static void *sibling(void *arg)
{
	long which = (long)arg;
	unsigned long n = 0;
	while (!stop_siblings) {
		switch (which % 5) {
		case 0: {
			void *p = malloc(100 + n % 900);
			if (p)
				memset(p, 1, 50);
			free(p);
			break;
		}
		case 1: {
			int fd = open("/etc/zoraya/session", O_RDONLY | O_CLOEXEC);
			if (fd >= 0)
				close(fd);
			break;
		}
		case 2:
			flockfile(stdout);
			for (volatile int i = 0; i < 200; i++)
				;
			funlockfile(stdout);
			break;
		case 3:
			seq++;
			counter_a++;
			counter_b++;
			seq++;
			break;
		default: {
			size_t len = (n % 8 == 7) ? (1u << 20) : 65536;
			unsigned char *m = mmap(NULL, len, PROT_READ | PROT_WRITE,
						MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
			if (m == MAP_FAILED)
				break;
			unsigned char v = (unsigned char)(n | 1);
			memset(m, v, len);
			region_byte = v;
			region = m;
			for (volatile int i = 0; i < 300; i++)
				;
			region = NULL;
			munmap(m, len);
			break;
		}
		}
		n++;
		if (n % 64 == 0)
			pause_ms(1);
	}
	return NULL;
}

#define SIBLINGS 5

static void start_siblings(pthread_t *t)
{
	stop_siblings = 0;
	for (long i = 0; i < SIBLINGS; i++)
		CHECK(pthread_create(&t[i], NULL, sibling, (void *)i) == 0);
}

static void stop_and_join(pthread_t *t)
{
	stop_siblings = 1;
	for (int i = 0; i < SIBLINGS; i++)
		CHECK(pthread_join(t[i], NULL) == 0);
}

static void *noop_thread(void *a)
{
	return a;
}

/* The child's checks; returns an exit code, 0 for all well. */
static int fork_child_checks(void)
{
	/* The counters were as of one instant: b is a, or one behind it while
	 * the sibling was between the two increments. */
	unsigned long a = counter_a, b = counter_b;
	if (a != b && a != b + 1)
		return 10;
	/* A mapping the sibling had published is in the copy whole (a fault
	 * here ends the child with a signal). The 1 MiB form is read at
	 * its ends only. */
	volatile unsigned char *m = region;
	if (m && m[0] != region_byte && m[0] != 0)
		return 16;
	/* The heap, stdio and the library work without waiting. */
	char *p = malloc(4096);
	if (!p)
		return 11;
	snprintf(p, 4096, "child %d", (int)getpid());
	free(p);
	if (printf("") < 0)
		return 12;
	int fd = open("/etc/zoraya/session", O_RDONLY);
	if (fd < 0)
		return 13;
	close(fd);
	/* A thread of its own works in the child. */
	pthread_t t;
	void *r;
	if (pthread_create(&t, NULL, noop_thread, (void *)5) != 0)
		return 14;
	if (pthread_join(t, &r) != 0 || r != (void *)5)
		return 15;
	return 0;
}

static void tfork(void)
{
	pthread_t t[SIBLINGS];
	start_siblings(t);
	pause_ms(20);
	for (int i = 0; i < 500; i++) {
		pid_t pid = fork();
		CHECK(pid >= 0);
		if (pid == 0)
			_exit(fork_child_checks());
		int status = reap(pid);
		if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
			printf("t_posix: tfork: round %d status 0x%x\n", i, status);
			CHECK(0);
		}
	}
	stop_and_join(t);
	/* The siblings are unharmed: they ran on, the parent still has them
	 * all (join worked). */
	passed();
}

static int atfork_log[16];
static int atfork_n;
static void prep1(void) { atfork_log[atfork_n++] = 1; }
static void prep2(void) { atfork_log[atfork_n++] = 2; }
static void par1(void) { atfork_log[atfork_n++] = 11; }
static void par2(void) { atfork_log[atfork_n++] = 12; }
static void chi1(void) { atfork_log[atfork_n++] = 21; }
static void chi2(void) { atfork_log[atfork_n++] = 22; }

static void *fork_in_thread(void *arg)
{
	(void)arg;
	atfork_n = 0;
	pid_t pid = fork();
	if (pid == 0) {
		/* Prepare in reverse order of registration, child in order. */
		int want[] = { 2, 1, 21, 22 };
		if (atfork_n != 4)
			_exit(20);
		for (int i = 0; i < 4; i++)
			if (atfork_log[i] != want[i])
				_exit(21 + i);
		pthread_t me = pthread_self();
		if (!pthread_equal(me, pthread_self()))
			_exit(30);
		_exit(fork_child_checks());
	}
	if (pid < 0)
		return (void *)1;
	int status = reap(pid);
	int want[] = { 2, 1, 11, 12 };
	if (atfork_n != 4)
		return (void *)2;
	for (int i = 0; i < 4; i++)
		if (atfork_log[i] != want[i])
			return (void *)3;
	return (void *)(long)(WIFEXITED(status) ? WEXITSTATUS(status) << 8 : 0xffff);
}

static void tforkthread(void)
{
	CHECK(pthread_atfork(prep1, par1, chi1) == 0);
	CHECK(pthread_atfork(prep2, par2, chi2) == 0);
	pthread_t sib[SIBLINGS], th;
	start_siblings(sib);
	void *r;
	CHECK(pthread_create(&th, NULL, fork_in_thread, NULL) == 0);
	CHECK(pthread_join(th, &r) == 0);
	if (r != NULL)
		printf("t_posix: tforkthread: thread returned %p\n", r);
	CHECK(r == NULL);
	stop_and_join(sib);
	passed();
}

/* exec from a non-main thread: the sibling opens files all the while. */
static void *exec_thread(void *arg)
{
	(void)arg;
	pause_ms(5);
	char *argv[] = { "t_posix", "texec-child", NULL };
	execve(SELF, argv, environ);
	_exit(99);
}

static void *opener(void *arg)
{
	(void)arg;
	for (;;) {
		int fd = open("/etc/zoraya/session", O_RDONLY | O_CLOEXEC);
		if (fd >= 0)
			close(fd);
	}
	return NULL;
}

/* The new image: no descriptor above 2 (the opener's were O_CLOEXEC; a
 * leak would be one the exec copied while it was open without the flag
 * honoured), and it is alone. */
static int texec_child(void)
{
	for (int fd = 3; fd < 64; fd++)
		if (fcntl(fd, F_GETFD) != -1)
			return 40 + fd % 20;
	return 7;
}

static void texec(void)
{
	for (int i = 0; i < 100; i++) {
		pid_t pid = fork();
		CHECK(pid >= 0);
		if (pid == 0) {
			pthread_t a, b, c;
			if (pthread_create(&a, NULL, opener, NULL) ||
			    pthread_create(&b, NULL, sibling, (void *)3) ||
			    pthread_create(&c, NULL, exec_thread, NULL))
				_exit(98);
			/* The main thread waits to be ended by the exec. */
			for (;;)
				pause_ms(1000);
		}
		int status = reap(pid);
		if (!WIFEXITED(status) || WEXITSTATUS(status) != 7) {
			printf("t_posix: texec: round %d status 0x%x\n", i, status);
			CHECK(0);
		}
	}
	passed();
}

static void *spawner(void *arg)
{
	long id = (long)arg;
	for (int i = 0; i < 6; i++) {
		pid_t pid;
		char *argv[] = { "t_posix", "exit7", NULL };
		if (id % 2) {
			if (posix_spawn(&pid, SELF, NULL, NULL, argv, environ) != 0)
				return (void *)1;
		} else {
			pid = vfork();
			if (pid == 0)
				_exit(7);
			if (pid < 0)
				return (void *)2;
		}
		int status = 0;
		pid_t got;
		do
			got = waitpid(pid, &status, 0);
		while (got == -1 && errno == EINTR);
		if (got != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 7)
			return (void *)3;
	}
	return NULL;
}

static void tspawn(void)
{
	pthread_t t[4];
	for (long i = 0; i < 4; i++)
		CHECK(pthread_create(&t[i], NULL, spawner, (void *)i) == 0);
	for (int i = 0; i < 4; i++) {
		void *r;
		CHECK(pthread_join(t[i], &r) == 0);
		if (r != NULL)
			printf("t_posix: tspawn: thread %d returned %p\n", i, r);
		CHECK(r == NULL);
	}
	passed();
}


/* ------------------------------------------------------------ signals */

static volatile pthread_t handled_on;
static volatile int handled_count;
static void on_usr(int sig)
{
	(void)sig;
	handled_on = pthread_self();
	handled_count++;
}

static sigset_t wait_set;
static volatile int wait_got;
static void *sigwaiter(void *arg)
{
	(void)arg;
	int sig = 0;
	int r = sigwait(&wait_set, &sig);
	wait_got = r == 0 ? sig : -r;
	return NULL;
}

static volatile int ready_flag;
static volatile int leave_flag;
static void *handler_thread(void *arg)
{
	(void)arg;
	sigset_t none;
	sigemptyset(&none);
	pthread_sigmask(SIG_SETMASK, &none, NULL);
	ready_flag = 1;
	while (!leave_flag)
		pause_ms(5);
	return NULL;
}

static void tsig(void)
{
	/* sigwait in a thread, woken by a process-directed signal that every
	 * thread blocks. */
	sigset_t set, all, none;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	sigfillset(&all);
	sigemptyset(&none);
	wait_set = set;
	CHECK(pthread_sigmask(SIG_BLOCK, &set, NULL) == 0);
	pthread_t w;
	wait_got = 0;
	CHECK(pthread_create(&w, NULL, sigwaiter, NULL) == 0);
	pause_ms(150);
	CHECK(wait_got == 0);
	CHECK(kill(getpid(), SIGUSR1) == 0);
	CHECK(pthread_join(w, NULL) == 0);
	CHECK(wait_got == SIGUSR1);
	/* Taken: it is not pending any more. */
	struct timespec zero = { 0, 0 };
	errno = 0;
	CHECK(sigtimedwait(&set, NULL, &zero) == -1 && errno == EAGAIN);
	/* A timeout, and a pending signal returned at once with its info. */
	struct timespec t0, t1, half = { 0, 60000000 };
	clock_gettime(CLOCK_MONOTONIC, &t0);
	errno = 0;
	CHECK(sigtimedwait(&set, NULL, &half) == -1 && errno == EAGAIN);
	clock_gettime(CLOCK_MONOTONIC, &t1);
	long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
	CHECK(ms >= 50 && ms < 2000);
	CHECK(raise(SIGUSR1) == 0);
	siginfo_t info;
	CHECK(sigwaitinfo(&set, &info) == SIGUSR1 && info.si_signo == SIGUSR1);

	/* kill(getpid(), n) with n blocked here and unblocked in a thread:
	 * the handler runs on that thread. */
	handle(SIGUSR2, on_usr, 0);
	sigset_t two;
	sigemptyset(&two);
	sigaddset(&two, SIGUSR2);
	CHECK(pthread_sigmask(SIG_BLOCK, &two, NULL) == 0);
	pthread_t h;
	ready_flag = 0;
	leave_flag = 0;
	handled_count = 0;
	/* The new thread starts with the mask in force and clears it. */
	CHECK(pthread_create(&h, NULL, handler_thread, NULL) == 0);
	while (!ready_flag)
		pause_ms(5);
	CHECK(kill(getpid(), SIGUSR2) == 0);
	for (int i = 0; i < 200 && !handled_count; i++)
		pause_ms(5);
	CHECK(handled_count == 1);
	CHECK(pthread_equal(handled_on, h));
	leave_flag = 1;
	CHECK(pthread_join(h, NULL) == 0);

	/* Every thread blocks it: it stays with the process until a thread
	 * unblocks it, and runs there. */
	handled_count = 0;
	CHECK(kill(getpid(), SIGUSR2) == 0);
	pause_ms(50);
	CHECK(handled_count == 0);
	CHECK(pthread_sigmask(SIG_UNBLOCK, &two, NULL) == 0);
	CHECK(handled_count == 1);
	CHECK(pthread_equal(handled_on, pthread_self()));
	handle(SIGUSR2, SIG_DFL, 0);
	pthread_sigmask(SIG_SETMASK, &none, NULL);
	passed();
}

/* waitpid in a thread leaves the library free for the others (INV-0302-2). */
static volatile pid_t wait_child_pid;
static volatile int wait_status;
static volatile pid_t wait_result;
static void *waiter(void *arg)
{
	(void)arg;
	int st = 0;
	wait_result = waitpid(wait_child_pid, &st, 0);
	wait_status = st;
	return NULL;
}

static void twait(void)
{
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		pause_ms(600);
		_exit(5);
	}
	wait_child_pid = pid;
	wait_result = 0;
	pthread_t w;
	CHECK(pthread_create(&w, NULL, waiter, NULL) == 0);
	pause_ms(100);
	struct timespec t0, t1;
	clock_gettime(CLOCK_MONOTONIC, &t0);
	for (int i = 0; i < 20; i++) {
		struct stat st;
		CHECK(getpid() > 0);
		CHECK(stat("/etc/zoraya/session", &st) == 0);
		int fd = open("/etc/zoraya/session", O_RDONLY);
		CHECK(fd >= 0);
		close(fd);
	}
	clock_gettime(CLOCK_MONOTONIC, &t1);
	long ms = (t1.tv_sec - t0.tv_sec) * 1000 + (t1.tv_nsec - t0.tv_nsec) / 1000000;
	/* The calls did not wait for the child's end (it is still running). */
	CHECK(ms < 400);
	CHECK(wait_result == 0);
	CHECK(pthread_join(w, NULL) == 0);
	CHECK(wait_result == pid);
	CHECK(WIFEXITED(wait_status) && WEXITSTATUS(wait_status) == 5);
	passed();
}


/* INV-0304-7 (1000 rounds): A waits in sem_wait with the signal blocked; B
 * takes the process's signals one by one; A does not return from sem_wait
 * until the post. (kill of oneself needs the broker's pid: not in
 * t_threads.) */
static volatile int rr_stop, rr_a_done, rr_ready;
static sem_t rr_sem;
static volatile int rr_count;
static volatile pthread_t rr_on;

static void rr_handler(int sig)
{
	(void)sig;
	rr_on = pthread_self();
	rr_count++;
}

static void *rr_a(void *a)
{
	rr_ready = 1;
	while (sem_wait(&rr_sem) != 0)
		;
	rr_a_done = 1;
	return a;
}

static void *rr_b(void *a)
{
	sigset_t none;
	sigemptyset(&none);
	pthread_sigmask(SIG_SETMASK, &none, NULL);
	while (!rr_stop) {
		getpid();
		pause_ms(1);
	}
	return a;
}

static void treroute(void)
{
	pthread_t a, b;
	sigset_t set, none;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	sigemptyset(&none);
	handle(SIGUSR1, rr_handler, 0);
	CHECK(sem_init(&rr_sem, 0, 0) == 0);
	CHECK(pthread_sigmask(SIG_BLOCK, &set, NULL) == 0);
	CHECK(pthread_create(&a, NULL, rr_a, NULL) == 0);
	CHECK(pthread_create(&b, NULL, rr_b, NULL) == 0);
	while (!rr_ready)
		pause_ms(5);
	pause_ms(100);
	for (int i = 0; i < 1000; i++) {
		CHECK(kill(getpid(), SIGUSR1) == 0);
		int j;
		for (j = 0; j < 5000 && rr_count < i + 1; j++)
			pause_ms(1);
		CHECK(rr_count == i + 1);
		CHECK(!rr_a_done);
	}
	CHECK(pthread_equal(rr_on, b));
	rr_stop = 1;
	CHECK(pthread_join(b, NULL) == 0);
	CHECK(sem_post(&rr_sem) == 0);
	CHECK(pthread_join(a, NULL) == 0);
	CHECK(rr_a_done);
	CHECK(pthread_sigmask(SIG_SETMASK, &none, NULL) == 0);
	handle(SIGUSR1, SIG_DFL, 0);
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
	else if (!strcmp(argv[1], "diskfill") && argc > 3)
		diskfill(argv[2], argv[3]);
	else if (!strcmp(argv[1], "products"))
		products();
	else if (!strcmp(argv[1], "live") && argc > 2)
		live(atoi(argv[2]));
	else if (!strcmp(argv[1], "exit7"))
		return 7;
	else if (!strcmp(argv[1], "tstp"))
		tstp();
	else if (!strcmp(argv[1], "notstp"))
		notstp();
	else if (!strcmp(argv[1], "tfork"))
		tfork();
	else if (!strcmp(argv[1], "tforkthread"))
		tforkthread();
	else if (!strcmp(argv[1], "texec"))
		texec();
	else if (!strcmp(argv[1], "texec-child"))
		return texec_child();
	else if (!strcmp(argv[1], "tsig"))
		tsig();
	else if (!strcmp(argv[1], "treroute"))
		treroute();
	else if (!strcmp(argv[1], "twait"))
		twait();
	else if (!strcmp(argv[1], "tspawn"))
		tspawn();
	else if (!strcmp(argv[1], "bench"))
		bench();
	else
		return 2;
	return 0;
}
