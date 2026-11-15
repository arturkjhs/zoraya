/* TASK-35 probe only, linked into rprobe and nothing else: the 22 names
 * Rust's prebuilt std for aarch64-unknown-linux-musl (1.99.1) references
 * that Zoraya's libc.a does not export. Not a proposal for libc.a; what
 * libposix should do instead is in docs/research/rust-std/survey.md.
 *
 * Three kinds, so that the probe table can say which result depends on what:
 *   H  libposix implements the name but the tier list hides it as
 *      __zoraya_NAME (ADR-0141): the shim only forwards.
 *   C  absent; the shim composes it from exported functions.
 *   E  absent; the shim fails with ENOSYS (what std then does is measured). */
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/types.h>

#define ENOSYS_STUB(ret, name, ...) \
    ret name(__VA_ARGS__) { errno = ENOSYS; return (ret)-1; }

/* H */
int __zoraya_bcmp(const void *, const void *, size_t);
int __zoraya_pause(void);
int __zoraya_pipe2(int *, int);
ssize_t __zoraya_sendmsg(int, const void *, int);
int __zoraya_setgroups(size_t, const gid_t *);
int __zoraya_setsid(void);
int bcmp(const void *a, const void *b, size_t n) { return __zoraya_bcmp(a, b, n); }
int pause(void) { return __zoraya_pause(); }
int pipe2(int *fds, int flags) { return __zoraya_pipe2(fds, flags); }
ssize_t sendmsg(int fd, const struct msghdr *m, int flags) { return __zoraya_sendmsg(fd, m, flags); }
int setgroups(size_t n, const gid_t *g) { return __zoraya_setgroups(n, g); }
pid_t setsid(void) { return __zoraya_setsid(); }

/* C */
/* std asks for AT_MINSIGSTKSZ (stack_overflow.rs) and std_detect for
 * AT_HWCAP; 0 is musl's answer for an absent entry. */
unsigned long getauxval(unsigned long type) {
    (void)type;
    errno = ENOENT;
    return 0;
}

/* libposix's DIR is a struct dirent of 280 bytes and then the fd
 * (crates/zoraya-posix/src/ffi.rs, struct Dir). */
int dirfd(void *dir) { return *(int *)((char *)dir + 280); }

int sched_yield(void) {
    struct timespec t = {0, 0};
    return nanosleep(&t, 0);
}

int clock_nanosleep(clockid_t clock, int flags, const struct timespec *t, struct timespec *rem) {
    struct timespec d = *t;
    if (flags & TIMER_ABSTIME) {
        struct timespec now;
        if (clock_gettime(clock, &now) != 0)
            return errno;
        d.tv_sec = t->tv_sec - now.tv_sec;
        d.tv_nsec = t->tv_nsec - now.tv_nsec;
        if (d.tv_nsec < 0) {
            d.tv_nsec += 1000000000;
            d.tv_sec -= 1;
        }
        if (d.tv_sec < 0)
            return 0;
        rem = 0;
    }
    return nanosleep(&d, rem) == 0 ? 0 : errno;
}

/* posix_spawn with a $PATH search for a name without a slash. */
int posix_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *fa,
                 const posix_spawnattr_t *attr, char *const argv[], char *const envp[]) {
    if (strchr(file, '/'))
        return posix_spawn(pid, file, fa, attr, argv, envp);
    const char *path = getenv("PATH");
    if (!path)
        path = "/bin";
    char buf[1024];
    int last = ENOENT;
    while (*path) {
        size_t n = strcspn(path, ":");
        if (n + strlen(file) + 2 <= sizeof buf) {
            memcpy(buf, path, n);
            buf[n] = '/';
            strcpy(buf + n + 1, file);
            int r = posix_spawn(pid, buf, fa, attr, argv, envp);
            if (r != ENOENT)
                return r;
        }
        path += n;
        if (*path == ':')
            path++;
    }
    return last;
}

/* E: std uses these only for options a probe does not set
 * (CommandExt::uid, gid, chroot), for pidfds, for kernel copies with a
 * fallback, or for CPU affinity with a fallback. */
int sigaltstack(const stack_t *ss, stack_t *old) {
    (void)ss;
    (void)old;
    errno = ENOSYS;
    return -1;
}
ENOSYS_STUB(int, chroot, const char *p)
ENOSYS_STUB(int, setuid, uid_t u)
ENOSYS_STUB(int, setgid, gid_t g)
ENOSYS_STUB(int, sched_getaffinity, pid_t p, size_t n, void *s)
ENOSYS_STUB(ssize_t, sendfile, int a, int b, off_t *o, size_t n)
ENOSYS_STUB(ssize_t, splice, int a, off_t *b, int c, off_t *d, size_t n, unsigned f)
ENOSYS_STUB(ssize_t, recvmsg, int fd, struct msghdr *m, int f)
ENOSYS_STUB(int, socketpair, int d, int t, int p, int *sv)
ENOSYS_STUB(int, waitid, int t, id_t id, void *info, int o)
int posix_spawn_file_actions_addchdir_np(posix_spawn_file_actions_t *fa, const char *p) {
    (void)fa;
    (void)p;
    return ENOSYS;
}
