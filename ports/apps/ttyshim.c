/* The terminal names that libc.a does not export before part 10c (ADR-0342
 * item 4), for the early builds of ncurses programs (TASK-31 ports-early:
 * less, nvi, ssh, git). Like zshim.c's, they are linked from a separate
 * archive, after everything else, and are not part of libc.a or of any
 * image: the surface of libc.a changes only by ADR (INV-0005-1). Each
 * does what the unoffered call does in libposix: fails with an errno
 * and changes nothing, so a program built with them links and can be sized
 * but is not a working terminal program. `tcsetattr` and `select` are weak:
 * zshim.c, which curl, wget, OpenSSH and git also link, defines the same two
 * names, and its definition wins. Part 10c adds the real names to
 * libposix and deletes this file and its rule in ports/Makefile.
 */
#include <errno.h>
#include <termios.h>

__attribute__((weak)) int tcsetattr(int fd, int action, const struct termios *t)
{
	(void)fd; (void)action; (void)t;
	errno = ENOTTY;
	return -1;
}

int tcflush(int fd, int queue)
{
	(void)fd; (void)queue;
	errno = ENOTTY;
	return -1;
}

speed_t cfgetospeed(const struct termios *t)
{
	return t->c_cflag & CBAUD;
}

speed_t cfgetispeed(const struct termios *t)
{
	return t->c_cflag & CBAUD;
}

/* The wait of nvi's input (cl/cl_read.c): `select` is ADR-0342 item 5's, built
 * with the terminal's readiness in part 10c; until then it is `pselect`, as in
 * zshim.c. */
#include <sys/select.h>
#include <time.h>

__attribute__((weak)) int select(int n, fd_set *r, fd_set *w, fd_set *e, struct timeval *tv)
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

/* Names of libposix's side (musl's src/fcntl and src/unistd are not retained,
 * ADR-0141) that less and ncurses reach: `creat` (less's log file) and `sleep`
 * (ncurses' mcprint). Part 10d asks libposix for them. */
#include <fcntl.h>
#include <unistd.h>

int creat(const char *path, mode_t mode)
{
	return open(path, O_WRONLY | O_CREAT | O_TRUNC, mode);
}

unsigned int sleep(unsigned int seconds)
{
	struct timespec ts = { seconds, 0 };
	return nanosleep(&ts, &ts) ? (unsigned int)ts.tv_sec : 0;
}

/* The speed setters of OpenSSH's ttymodes.c (ADR-0342 item 4: stored, without effect). */
int cfsetospeed(struct termios *t, speed_t speed)
{
	t->c_cflag = (t->c_cflag & ~CBAUD) | (speed & CBAUD);
	return 0;
}

int cfsetispeed(struct termios *t, speed_t speed)
{
	(void)t;
	return speed == 0 || (speed & ~CBAUD) == 0 ? 0 : -1;
}

/* Marker: ports/check-early.sh looks for it to prove that no image binary links this shim. */
__attribute__((used)) const char zoraya_early_shim_ttyshim[] = "ZORAYA-EARLY-SHIM-ttyshim";
