/* The names OpenSSH's client reaches that libc.a does not export (TASK-31
 * ports-early; ADR-0346 lists them for part 10g). Like zshim.c and ttyshim.c,
 * they are linked from a separate archive, after everything else, and are not
 * part of libc.a or of any image: the surface of libc.a changes only by ADR
 * (INV-0005-1). Each fails the way an unoffered call fails in libposix
 * (ADR-0287 item 9) and changes nothing, so a caller that tries one reports an
 * error. None is reached on the paths a Zoraya ssh takes: each belongs to a
 * feature the description does not offer (a proxy command or jump host, an
 * interface to bind, a security-key or PKCS#11 helper, a privilege change).
 * Part 10g (ADR-0393) keeps it: ssh and ssh-keygen are in the apps' /bin since
 * then, so the file carries no marker (ports/check-early.sh looks for one only
 * in what must not reach an image). The link probe of 10g
 * (build/ports/logs/openssh.log) finds no other name undefined.
 */
#include <errno.h>
#include <netdb.h>
#include <stdarg.h>
#include <stddef.h>
#include <sys/socket.h>
#include <sys/types.h>

/* readconf.c: a service name in a port number (`-p ssh`). */
struct servent *getservbyname(const char *name, const char *proto)
{
	(void)name; (void)proto;
	return NULL;
}

/* sshconnect.c: ProxyCommand, ProxyJump. */
int socketpair(int domain, int type, int protocol, int sv[2])
{
	(void)domain; (void)type; (void)protocol; (void)sv;
	errno = ENOSYS;
	return -1;
}

/* sshconnect.c: BindInterface. */
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

/* ssh-sk-client.c, ssh-pkcs11-client.c: the helper programs of security keys
 * and PKCS#11, which are not built. */
int execlp(const char *file, const char *arg, ...)
{
	(void)file; (void)arg;
	errno = ENOSYS;
	return -1;
}

/* sshkey.c: advice about a buffer's core dump; there are no cores. */
int madvise(void *addr, size_t len, int advice)
{
	(void)addr; (void)len; (void)advice;
	errno = ENOSYS;
	return -1;
}

/* fake-rfc2553.c's getnameinfo, for a reverse lookup that `ssh` does not ask for. */
struct hostent *gethostbyaddr(const void *addr, socklen_t len, int type)
{
	(void)addr; (void)len; (void)type;
	return NULL;
}

/* misc.c: the privilege change of a subprocess (ProxyCommand and kin);
 * `setuid` and the rest stay unexported (ADR-0005). */
int initgroups(const char *user, gid_t group)
{
	(void)user; (void)group;
	errno = EPERM;
	return -1;
}
