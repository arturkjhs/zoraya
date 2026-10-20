/* Runtime for the LD_PRELOAD libc interposer (M1 dynamic lower bound).
 *
 * Every interposed symbol is an assembly trampoline (psurf_tramp.S, generated) that saves all argument
 * registers, calls psurf_resolve(idx, caller), restores the registers and tail-branches to the real musl
 * function. Arguments are never re-marshalled, so variadic functions (printf, open, fcntl, ioctl, execl)
 * and non-returning or stack-sensitive ones (vfork, setjmp, longjmp) pass through unchanged.
 *
 * A hit is logged once per symbol per process image, and only when the caller is outside musl itself
 * (musl routes its internal malloc calls through an interposed malloc; those are not program calls).
 * This file must not call any libc function except dlsym (which is never interposed): every other libc
 * symbol this library exports resolves to its own trampoline. I/O uses raw aarch64 syscalls.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdint.h>

extern const char *const psurf_names[];
extern const unsigned psurf_count;
extern void *psurf_slots[];   /* fast path, set after a direct (non-libc) hit was logged */
static void *real[8192];      /* resolved addresses */

static long sys(long n, long a, long b, long c, long d) {
    register long x8 __asm__("x8") = n, x0 __asm__("x0") = a, x1 __asm__("x1") = b,
                  x2 __asm__("x2") = c, x3 __asm__("x3") = d;
    __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3) : "memory", "cc");
    return x0;
}
#define AT_FDCWD (-100)
enum { SYS_openat = 56, SYS_close = 57, SYS_read = 63, SYS_write = 64, SYS_readlinkat = 78,
       SYS_exit_group = 94, SYS_getpid = 172 };

static unsigned long libc_lo, libc_hi;
static int ranges_ready;

static unsigned long hex(const char **p) {
    unsigned long v = 0;
    for (;; (*p)++) {
        char c = **p;
        if (c >= '0' && c <= '9') v = v * 16 + (c - '0');
        else if (c >= 'a' && c <= 'f') v = v * 16 + (c - 'a' + 10);
        else return v;
    }
}
static int contains(const char *s, const char *e, const char *needle) {
    for (; s < e; s++) {
        const char *a = s, *b = needle;
        while (a < e && *b && *a == *b) a++, b++;
        if (!*b) return 1;
    }
    return 0;
}
/* find the address range of musl (ld-musl-aarch64.so.1 == libc.so) from /proc/self/maps */
static void find_libc(void) {
    static char buf[1 << 16];
    long fd = sys(SYS_openat, AT_FDCWD, (long)"/proc/self/maps", 0, 0), n, len = 0;
    if (fd < 0) { ranges_ready = 1; return; }
    while (len < (long)sizeof buf - 1 && (n = sys(SYS_read, fd, (long)(buf + len), sizeof buf - 1 - len, 0)) > 0)
        len += n;
    sys(SYS_close, fd, 0, 0, 0);
    const char *p = buf, *end = buf + len;
    while (p < end) {
        const char *eol = p;
        while (eol < end && *eol != '\n') eol++;
        if (contains(p, eol, "ld-musl")) {
            const char *q = p;
            unsigned long lo = hex(&q); q++;
            unsigned long hi = hex(&q);
            if (!libc_lo || lo < libc_lo) libc_lo = lo;
            if (hi > libc_hi) libc_hi = hi;
        }
        p = eol + 1;
    }
    ranges_ready = 1;
}

static long slen(const char *s) { long n = 0; while (s[n]) n++; return n; }
static char *put(char *d, const char *s, long n) { for (long i = 0; i < n; i++) *d++ = s[i]; return d; }
static char *putnum(char *d, long v) {
    char t[24]; int i = 0;
    do t[i++] = '0' + v % 10; while (v /= 10);
    while (i) *d++ = t[--i];
    return d;
}

static void log_hit(unsigned idx) {
    char path[64], line[4096 + 512], *w;
    long pid = sys(SYS_getpid, 0, 0, 0, 0);
    w = put(path, "/tmp/psurf-hits/", 16); w = putnum(w, pid); *w = 0;
    /* argv[0] from /proc/self/cmdline, executable from /proc/self/exe */
    char arg0[512]; long an = 0;
    long fd = sys(SYS_openat, AT_FDCWD, (long)"/proc/self/cmdline", 0, 0);
    if (fd >= 0) { an = sys(SYS_read, fd, (long)arg0, sizeof arg0 - 1, 0); sys(SYS_close, fd, 0, 0, 0); }
    if (an < 0) an = 0;
    arg0[an] = 0; an = slen(arg0);
    char exe[2048];
    long en = sys(SYS_readlinkat, AT_FDCWD, (long)"/proc/self/exe", (long)exe, sizeof exe - 1);
    if (en < 0) en = 0;
    w = line;
    w = put(w, arg0, an); *w++ = '\t';
    w = put(w, exe, en); *w++ = '\t';
    w = put(w, psurf_names[idx], slen(psurf_names[idx])); *w++ = '\n';
    fd = sys(SYS_openat, AT_FDCWD, (long)path, 01 | 0100 | 02000 | 02000000, 0644); /* WRONLY|CREAT|APPEND|CLOEXEC */
    if (fd >= 0) { sys(SYS_write, fd, (long)line, w - line, 0); sys(SYS_close, fd, 0, 0, 0); }
}

void *psurf_resolve(unsigned idx, unsigned long caller) {
    void *p = __atomic_load_n(&real[idx], __ATOMIC_ACQUIRE);
    if (!p) {
        p = dlsym(RTLD_NEXT, psurf_names[idx]);
        if (!p) {
            static const char m[] = "psurf: unresolved libc symbol\n";
            sys(SYS_write, 2, (long)m, sizeof m - 1, 0);
            sys(SYS_exit_group, 127, 0, 0, 0);
        }
        __atomic_store_n(&real[idx], p, __ATOMIC_RELEASE);
    }
    if (!ranges_ready) find_libc();
    if (!(caller >= libc_lo && caller < libc_hi)) {
        log_hit(idx);
        __atomic_store_n(&psurf_slots[idx], p, __ATOMIC_RELEASE);
    }
    return p;
}
