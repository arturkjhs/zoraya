/* Names that OpenSSH's client and ssh-keygen reference and Zoraya's libc.a does
 * not export (TASK-31 10g, proposed ADR-0393). They are linked into those two
 * programs from libsshshim.a, as libzshim.a is into curl and wget, not into
 * libc.a: the surface of libc.a changes only by ADR (INV-0005-1), and a program
 * that does not call these names never links them. The link probe of the port
 * (build/ports/logs/openssh.log) found exactly these; none is reached on a path
 * a Zoraya ssh takes: the configuration file, ProxyCommand, LocalCommand, the
 * agent, the syslog, DNS host-key records and the interface list are all
 * compiled out or unreachable (ports/patches/openssh/, ADR-0340 item 3).
 * Each does what the unoffered call does in libposix (ADR-0287 item 9): fails
 * with an errno and changes nothing, so that a caller that tries one reports an
 * error instead of crashing. `sleep` is real, over `nanosleep`: ssh waits
 * between connection attempts.
 */
#include <errno.h>
#include <netdb.h>
#include <stdarg.h>
#include <stddef.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

unsigned int sleep(unsigned int seconds)
{
	struct timespec left = { .tv_sec = (time_t)seconds, .tv_nsec = 0 };
	if (nanosleep(&left, &left) == 0)
		return 0;
	return (unsigned int)left.tv_sec;
}

int initgroups(const char *user, gid_t group)
{
	(void)user; (void)group;
	errno = ENOSYS;
	return -1;
}

int execl(const char *path, const char *arg, ...)
{
	(void)path; (void)arg;
	errno = ENOSYS;
	return -1;
}

int execlp(const char *file, const char *arg, ...)
{
	(void)file; (void)arg;
	errno = ENOSYS;
	return -1;
}

int madvise(void *addr, size_t length, int advice)
{
	(void)addr; (void)length; (void)advice;
	errno = ENOSYS;
	return -1;
}

struct ifaddrs;

int getifaddrs(struct ifaddrs **ifap)
{
	(void)ifap;
	errno = ENOSYS;
	return -1;
}

void freeifaddrs(struct ifaddrs *ifa)
{
	(void)ifa;
}

/* `ssh` finds the default port in the services table, and takes 22 when there
 * is no entry. */
struct servent *getservbyname(const char *name, const char *proto)
{
	(void)name; (void)proto;
	return NULL;
}

struct hostent *gethostbyaddr(const void *addr, socklen_t len, int type)
{
	(void)addr; (void)len; (void)type;
	return NULL;
}

void openlog(const char *ident, int option, int facility)
{
	(void)ident; (void)option; (void)facility;
}

void closelog(void)
{
}

void syslog(int priority, const char *format, ...)
{
	(void)priority; (void)format;
}

/* The resolver's records (VerifyHostKeyDNS, which no line can turn on): none.
 * `__res_state` hands out zeroed memory big enough for musl's struct. */
void *__res_state(void)
{
	static long long state[512];
	return state;
}

int __h_errno_dummy;

int *__h_errno_location(void)
{
	return &__h_errno_dummy;
}

int res_init(void)
{
	return -1;
}

int res_query(const char *name, int class, int type, unsigned char *answer, int len)
{
	(void)name; (void)class; (void)type; (void)answer; (void)len;
	return -1;
}

int dn_expand(const unsigned char *msg, const unsigned char *eom, const unsigned char *src,
	char *dst, int space)
{
	(void)msg; (void)eom; (void)src; (void)dst; (void)space;
	return -1;
}

unsigned int ns_get16(const unsigned char *src)
{
	(void)src;
	return 0;
}

unsigned long ns_get32(const unsigned char *src)
{
	(void)src;
	return 0;
}
