/* libzoraya-rust.a (ADR-0451 item 5): the names Rust's `std` references that
 * libc.a keeps out, each failing with ENOSYS. Linked only by tools/zcargo
 * (-lzoraya-rust); no C or C++ program built by zcc or zc++ links it
 * (INV-0451-2, make check-rust-archive).
 *
 * What std does with each (rust-std/survey.md#what-std-links):
 *   setuid setgid setgroups setsid chroot  CommandExt::uid, gid, groups, chroot and
 *                         setsid: the call fails, so the child is never started
 *                         (a Rust program never changes identity, INV-0453-2)
 *   sigaltstack           the stack-overflow handler of every thread: std then reads
 *                         a zeroed stack_t and installs no alternate stack
 *   sched_getaffinity     available_parallelism: std falls back to sysconf
 *   sendfile splice       io::copy between descriptors: std falls back
 *   waitid                pidfds: an error to the caller
 *
 * A scoped exception to INV-0005-2's intent, named by ADR-0451: in a program
 * zcargo links, setuid, setgid and sigaltstack resolve (to failures), so that
 * a crate's C code that calls them links and gets ENOSYS. libc.a exports none
 * of them. A name enters this file only by an ADR (ports/rust/names.txt).
 *
 * The prototypes are the sysroot's, so a definition cannot disagree with the
 * header a caller compiled against. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <sched.h>
#include <signal.h>
#include <sys/sendfile.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define FAIL(type) do { errno = ENOSYS; return (type)-1; } while (0)

int setuid(uid_t uid) { (void)uid; FAIL(int); }
int setgid(gid_t gid) { (void)gid; FAIL(int); }
int setgroups(size_t n, const gid_t *list) { (void)n; (void)list; FAIL(int); }
pid_t setsid(void) { FAIL(pid_t); }
int chroot(const char *path) { (void)path; FAIL(int); }
int sigaltstack(const stack_t *restrict ss, stack_t *restrict old) { (void)ss; (void)old; FAIL(int); }
int sched_getaffinity(pid_t pid, size_t size, cpu_set_t *set) { (void)pid; (void)size; (void)set; FAIL(int); }
ssize_t sendfile(int out, int in, off_t *offset, size_t count) { (void)out; (void)in; (void)offset; (void)count; FAIL(ssize_t); }
ssize_t splice(int in, off_t *off_in, int out, off_t *off_out, size_t len, unsigned flags)
{
	(void)in; (void)off_in; (void)out; (void)off_out; (void)len; (void)flags;
	FAIL(ssize_t);
}
int waitid(idtype_t type, id_t id, siginfo_t *info, int options) { (void)type; (void)id; (void)info; (void)options; FAIL(int); }
