/* Names that CPython's sources reference and Zoraya's libc.a does not export
 * (TASK-27 9c, proposed ADR-0320). Linked into python3 from libpyshim.a,
 * never into libc.a: the surface of libc.a changes only by ADR (INV-0005-1),
 * and a program that does not name these does not link them.
 */
#include <unistd.h>

extern char **environ;

/* posixmodule.c needs execv for posix_spawn's argument list and for os.execv;
 * libc.a exports execve and execvp (the tier list) and not execv. */
int execv(const char *path, char *const argv[])
{
	return execve(path, argv, environ);
}

#include <dirent.h>
#include <errno.h>
#include <sched.h>
#include <sys/mman.h>
#include <time.h>

/* obmalloc.c asks madvise for huge pages on an arena, mmapmodule.c msyncs a
 * flushed mapping, and a thread of lock.c yields while it spins: advice that
 * no kernel here takes. madvise and msync fail with ENOSYS and change
 * nothing; sched_yield is libposix's own (SYS_sched_yield of ADR-0308 item 4,
 * which libc.a does not export under the POSIX name). */
int madvise(void *addr, size_t len, int advice)
{
	(void)addr; (void)len; (void)advice;
	errno = ENOSYS;
	return -1;
}

int msync(void *addr, size_t len, int flags)
{
	(void)addr; (void)len; (void)flags;
	errno = ENOSYS;
	return -1;
}

long zoraya_posix_sched_yield(void);

int sched_yield(void)
{
	return (int)zoraya_posix_sched_yield();
}

/* thread.c parks a thread for good with pause; it returns, as POSIX says,
 * only when a signal has been delivered (nanosleep ends with EINTR then), not
 * after a fixed time. */
int pause(void)
{
	struct timespec ts = { 3600, 0 };
	while (nanosleep(&ts, 0) == 0)
		;
	errno = EINTR;
	return -1;
}

/* _posixsubprocess.c lists the open descriptors by opening a directory of
 * them (/proc/self/fd, /dev/fd); neither exists, so the opendir fails before
 * dirfd is reached, and the loop over every descriptor takes over. libposix
 * keeps a directory stream without a descriptor. */
int dirfd(DIR *d)
{
	(void)d;
	errno = ENOTSUP;
	return -1;
}

/* ---- names of the database and terminal modules

 * _socket.c names the classic resolver and the protocol and service
 * databases; termios.c the speed and flow calls; sendmsg
 * and recvmsg carry ancillary data, which Zoraya's sockets do not.
 * gethostbyname is getaddrinfo (libposix answers IPv4 only); the databases
 * are the few entries a program asks for by name (there is no /etc/services
 * on the volume); the terminal calls fail the way libposix's tcsetattr does
 * (ENOTTY); the other calls fail with ENOSYS and change nothing. */
#include <netdb.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <string.h>
#include <sys/socket.h>
#include <termios.h>

int *__h_errno_location(void)
{
	static int h_errno_value;
	return &h_errno_value;
}

const char *hstrerror(int e)
{
	switch (e) {
	case HOST_NOT_FOUND: return "Unknown host";
	case TRY_AGAIN: return "Host name lookup failure";
	case NO_RECOVERY: return "Unknown server error";
	case NO_DATA: return "No address associated with name";
	}
	return "Resolver internal error";
}

struct hostent *gethostbyname(const char *name)
{
	static struct hostent he;
	static char canon[256];
	static unsigned char addr[4];
	static char *addrs[2];
	static char *aliases[1];
	struct addrinfo hints, *res;

	memset(&hints, 0, sizeof hints);
	hints.ai_family = AF_INET;
	hints.ai_socktype = SOCK_STREAM;
	if (getaddrinfo(name, 0, &hints, &res) != 0 || !res) {
		h_errno = HOST_NOT_FOUND;
		return 0;
	}
	memcpy(addr, &((struct sockaddr_in *)res->ai_addr)->sin_addr, 4);
	freeaddrinfo(res);
	strncpy(canon, name, sizeof canon - 1);
	addrs[0] = (char *)addr;
	addrs[1] = 0;
	aliases[0] = 0;
	he.h_name = canon;
	he.h_aliases = aliases;
	he.h_addrtype = AF_INET;
	he.h_length = 4;
	he.h_addr_list = addrs;
	return &he;
}

struct hostent *gethostbyaddr(const void *addr, socklen_t len, int type)
{
	(void)addr; (void)len; (void)type;
	h_errno = HOST_NOT_FOUND;
	return 0;
}

struct protoent *getprotobyname(const char *name)
{
	static struct protoent tcp = { "tcp", 0, IPPROTO_TCP };
	static struct protoent udp = { "udp", 0, IPPROTO_UDP };
	static struct protoent icmp = { "icmp", 0, IPPROTO_ICMP };
	static char *none[1];
	tcp.p_aliases = udp.p_aliases = icmp.p_aliases = none;
	if (!strcmp(name, "tcp")) return &tcp;
	if (!strcmp(name, "udp")) return &udp;
	if (!strcmp(name, "icmp")) return &icmp;
	return 0;
}

static struct servent *service(const char *name, int port, const char *proto)
{
	static const struct { const char *name; int port; } table[] = {
		{ "ftp", 21 }, { "ssh", 22 }, { "smtp", 25 }, { "domain", 53 },
		{ "http", 80 }, { "ntp", 123 }, { "https", 443 }, { "imaps", 993 },
	};
	static struct servent se;
	static char *none[1];
	static char pname[8];
	for (unsigned i = 0; i < sizeof table / sizeof table[0]; i++) {
		if ((name && !strcmp(name, table[i].name)) || (!name && port == table[i].port)) {
			strncpy(pname, proto ? proto : "tcp", sizeof pname - 1);
			se.s_name = (char *)table[i].name;
			se.s_aliases = none;
			se.s_port = htons(table[i].port);
			se.s_proto = pname;
			return &se;
		}
	}
	return 0;
}

struct servent *getservbyname(const char *name, const char *proto)
{
	return service(name, 0, proto);
}

struct servent *getservbyport(int port, const char *proto)
{
	return service(0, ntohs(port), proto);
}

int __libc_current_sigrtmin(void) { return 35; }
int __libc_current_sigrtmax(void) { return 64; }

speed_t cfgetospeed(const struct termios *t) { return t->c_cflag & CBAUD; }
speed_t cfgetispeed(const struct termios *t) { return cfgetospeed(t); }

int cfsetospeed(struct termios *t, speed_t speed)
{
	if (speed & ~CBAUD) {
		errno = EINVAL;
		return -1;
	}
	t->c_cflag &= ~CBAUD;
	t->c_cflag |= speed;
	return 0;
}

int cfsetispeed(struct termios *t, speed_t speed)
{
	return speed ? cfsetospeed(t, speed) : 0;
}

int tcdrain(int fd) { (void)fd; errno = ENOTTY; return -1; }
int tcflow(int fd, int action) { (void)fd; (void)action; errno = ENOTTY; return -1; }
int tcflush(int fd, int queue) { (void)fd; (void)queue; errno = ENOTTY; return -1; }
int tcsendbreak(int fd, int duration) { (void)fd; (void)duration; errno = ENOTTY; return -1; }

ssize_t sendmsg(int fd, const struct msghdr *m, int flags)
{
	(void)fd; (void)m; (void)flags;
	errno = ENOSYS;
	return -1;
}

ssize_t recvmsg(int fd, struct msghdr *m, int flags)
{
	(void)fd; (void)m; (void)flags;
	errno = ENOSYS;
	return -1;
}
