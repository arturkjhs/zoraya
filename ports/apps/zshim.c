/* Names that curl's and wget's sources reference and Zoraya's libc.a does not
 * export (TASK-27 9a, proposed ADR-0310). They are linked into those two
 * programs from libzshim.a, not into libc.a: the surface of libc.a changes
 * only by ADR (INV-0005-1), and a program that does not call these names
 * never links them. Each does what the unoffered call does in libposix
 * (ADR-0287 item 9): fails with an errno and changes nothing, so that a
 * caller that tries one reports an error instead of crashing.
 *
 * `select` is not a refusal: it is `pselect` (which libc.a exports, ADR-0006)
 * with a timeval. `tcsetattr` is refused with ENOTTY: curl's password
 * prompt is the only caller, and `--user` without a password is not offered
 * by curl.cmd.
 */
#include <errno.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <termios.h>
#include <time.h>

int select(int n, fd_set *r, fd_set *w, fd_set *e, struct timeval *tv)
{
	struct timespec ts;
	if (tv) {
		if (tv->tv_sec < 0 || tv->tv_usec < 0 || tv->tv_usec >= 1000000) {
			errno = EINVAL;
			return -1;
		}
		ts.tv_sec = tv->tv_sec;
		ts.tv_nsec = tv->tv_usec * 1000L;
	}
	return pselect(n, r, w, e, tv ? &ts : 0, 0);
}

int bind(int fd, const struct sockaddr *a, socklen_t len)
{
	(void)fd; (void)a; (void)len;
	errno = ENOSYS;
	return -1;
}

int listen(int fd, int backlog)
{
	(void)fd; (void)backlog;
	errno = ENOSYS;
	return -1;
}

int accept(int fd, struct sockaddr *a, socklen_t *len)
{
	(void)fd; (void)a; (void)len;
	errno = ENOSYS;
	return -1;
}

int tcsetattr(int fd, int action, const struct termios *t)
{
	(void)fd; (void)action; (void)t;
	errno = ENOTTY;
	return -1;
}

#include <unistd.h>

/* Hard links are not offered (ADR-0107: no link objects but symbolic ones);
 * gnulib in wget insists on a `link` to build and uses it for nothing wget
 * does here. */
int link(const char *old, const char *new)
{
	(void)old; (void)new;
	errno = ENOSYS;
	return -1;
}

#include <dirent.h>

/* wget (through gnulib's own getcwd, spawn and timer code) names these. None
 * is reached on the paths a Zoraya wget takes: `setsid` is `-b`
 * (background), `setresuid`/`setresgid` are posix_spawn's RESETIDS flag,
 * `clock_getres` is the timer's choice of clock (wget then reads the time of
 * day), and `rewinddir` is the fallback of gnulib's getcwd when the C
 * library's fails (it does not: libposix keeps the directory itself). */
int setsid(void)
{
	errno = ENOSYS;
	return -1;
}

int setresuid(uid_t r, uid_t e, uid_t s)
{
	(void)r; (void)e; (void)s;
	errno = ENOSYS;
	return -1;
}

int setresgid(gid_t r, gid_t e, gid_t s)
{
	(void)r; (void)e; (void)s;
	errno = ENOSYS;
	return -1;
}

int clock_getres(clockid_t id, struct timespec *res)
{
	(void)id; (void)res;
	errno = ENOSYS;
	return -1;
}

void rewinddir(DIR *d)
{
	(void)d;
}

/* glibc's two program-name variables, which gnulib's getprogname reads where
 * <errno.h> declares them (musl does, under _GNU_SOURCE) and which musl
 * defines in a file libc.a leaves out. wget's own messages use its argv[0];
 * these two name it for gnulib's error(), and only wget links them. */
char *program_invocation_name = "wget";
char *program_invocation_short_name = "wget";
