/* TASK-35 probe only: the "fixed" variant of rprobe, which emulates in the
 * program what the survey proposes libposix should answer, to measure what
 * is left after it. Linked beside zshim.c, with -Wl,--wrap=syscall and -Wl,--wrap=ioctl.
 *
 * 1. syscall() (by -Wl,--wrap=syscall) returning long (libposix's returns int: -1 in w0 reads as
 *    4294967295 in x0) and answering the calls std makes through it:
 *    futex (FUTEX_WAIT_BITSET with an absolute CLOCK_MONOTONIC deadline and
 *    a full mask, FUTEX_WAKE), getrandom; ENOSYS for the rest, which std
 *    falls back from (statx, copy_file_range, splice, pidfd_*, gettid).
 * 2. ioctl(FIONBIO), which std uses on Linux for set_nonblocking: through
 *    fcntl(F_SETFL, O_NONBLOCK). */
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <sys/ioctl.h>
#include <sys/random.h>
#include <time.h>

long zoraya_posix_futex(long, long, long, long, long, long);

#define NR_FUTEX 98
#define NR_GETRANDOM 278
#define FUTEX_WAIT 0
#define FUTEX_WAKE 1
#define FUTEX_WAIT_BITSET 9
#define FUTEX_PRIVATE 128

static long ret(long r) {
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return r;
}

static long futex(long addr, long op, long val, long ts, long addr2, long val3) {
    long cmd = op & ~FUTEX_PRIVATE;
    if (cmd == FUTEX_WAIT_BITSET && (unsigned)val3 == 0xffffffffu) {
        struct timespec rel, *p = 0;
        if (ts) {
            const struct timespec *abs = (const struct timespec *)ts;
            struct timespec now;
            clock_gettime(CLOCK_MONOTONIC, &now);
            rel.tv_sec = abs->tv_sec - now.tv_sec;
            rel.tv_nsec = abs->tv_nsec - now.tv_nsec;
            if (rel.tv_nsec < 0) {
                rel.tv_nsec += 1000000000;
                rel.tv_sec -= 1;
            }
            if (rel.tv_sec < 0 || (rel.tv_sec == 0 && rel.tv_nsec == 0))
                return ret(-ETIMEDOUT);
            p = &rel;
        }
        return ret(zoraya_posix_futex(addr, FUTEX_WAIT | FUTEX_PRIVATE, val, (long)p, 0, 0));
    }
    if (cmd == FUTEX_WAIT || cmd == FUTEX_WAKE)
        return ret(zoraya_posix_futex(addr, op, val, ts, addr2, val3));
    return ret(-ENOSYS);
}

long __wrap_syscall(long n, ...) {
    va_list ap;
    va_start(ap, n);
    long a = va_arg(ap, long), b = va_arg(ap, long), c = va_arg(ap, long);
    long d = va_arg(ap, long), e = va_arg(ap, long), f = va_arg(ap, long);
    va_end(ap);
    switch (n) {
    case NR_FUTEX:
        return futex(a, b, c, d, e, f);
    case NR_GETRANDOM:
        return getrandom((void *)a, (size_t)b, (unsigned)c);
    default:
        return ret(-ENOSYS);
    }
}

int __real_ioctl(int, int, ...);
int __wrap_ioctl(int fd, int req, ...) {
    va_list ap;
    va_start(ap, req);
    void *arg = va_arg(ap, void *);
    va_end(ap);
    if (req == FIONBIO) {
        int on = *(const int *)arg;
        int fl = fcntl(fd, F_GETFL);
        if (fl < 0)
            return -1;
        return fcntl(fd, F_SETFL, on ? fl | O_NONBLOCK : fl & ~O_NONBLOCK);
    }
    return __real_ioctl(fd, req, arg);
}
