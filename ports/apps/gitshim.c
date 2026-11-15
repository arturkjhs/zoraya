/* The names git reaches that libc.a does not export (TASK-31 ports-early;
 * ADR-0347 lists them for part 10h). As for sshshim.c, ttyshim.c and zshim.c:
 * a separate archive, linked after everything else, in no image; each fails the
 * way an unoffered call fails in libposix (ADR-0287 item 9) and changes nothing.
 * Part 10h decides, name by name, whether to export it or to patch the use out,
 * and deletes this file.
 */
#include <errno.h>
#include <netdb.h>
#include <sys/socket.h>
#include <sys/statvfs.h>
#include <sys/types.h>

/* upload-pack.c: the timeout of a served fetch (`--timeout`). git is built with
 * NO_SETITIMER; ADR-0342 item 6 keeps `alarm` out. The server half of git is
 * not part of a Zoraya git, which only fetches and pushes. */
unsigned int alarm(unsigned int seconds)
{
	(void)seconds;
	return 0;
}

/* connect.c: the printed form of the address a connection went to. */
int getnameinfo(const struct sockaddr *sa, socklen_t salen, char *host, socklen_t hostlen,
		char *serv, socklen_t servlen, int flags)
{
	(void)sa; (void)salen; (void)host; (void)hostlen; (void)serv; (void)servlen; (void)flags;
	return EAI_FAIL;
}

/* diagnose.c: `git diagnose`'s disk figures. */
int statvfs(const char *path, struct statvfs *buf)
{
	(void)path; (void)buf;
	errno = ENOSYS;
	return -1;
}

/* help.c: `git help` starting a manual viewer. */
int execlp(const char *file, const char *arg, ...)
{
	(void)file; (void)arg;
	errno = ENOSYS;
	return -1;
}
