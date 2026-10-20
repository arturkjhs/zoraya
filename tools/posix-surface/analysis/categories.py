"""Symbol -> header -> category, for musl libc symbols.

The header is found by scanning the installed musl headers (/usr/include) for a declaration of the symbol.
The category comes from HEADER_CAT, with per-symbol overrides for headers that mix concerns (unistd.h,
stdlib.h, ...). The mapping is a classification choice, not a measurement; method.md says so.
"""
import os, re, functools

CATS = ["stdio", "string", "process", "signal", "file", "dir", "time", "locale", "tty",
        "memory", "threads", "network", "other"]

HEADER_CAT = {
    "stdio.h": "stdio", "stdio_ext.h": "stdio", "err.h": "stdio", "printf.h": "stdio",
    "string.h": "string", "strings.h": "string", "ctype.h": "string", "wchar.h": "string",
    "wctype.h": "string", "fnmatch.h": "string", "regex.h": "string", "libgen.h": "string",
    "inttypes.h": "string", "uchar.h": "string",
    "signal.h": "signal",
    "fcntl.h": "file", "sys/stat.h": "file", "sys/file.h": "file", "sys/statvfs.h": "file",
    "sys/statfs.h": "file", "utime.h": "file", "sys/sendfile.h": "file", "sys/xattr.h": "file",
    "sys/mount.h": "file", "sys/inotify.h": "file", "sys/uio.h": "file", "sys/vfs.h": "file",
    "mntent.h": "file", "sys/sysmacros.h": "file", "linux/fs.h": "file", "aio.h": "file",
    "dirent.h": "dir", "ftw.h": "dir", "glob.h": "dir", "fts.h": "dir",
    "time.h": "time", "sys/time.h": "time", "sys/times.h": "time", "sys/timerfd.h": "time",
    "sys/timex.h": "time",
    "locale.h": "locale", "langinfo.h": "locale", "iconv.h": "locale", "nl_types.h": "locale",
    "libintl.h": "locale", "monetary.h": "locale",
    "termios.h": "tty", "pty.h": "tty", "utmp.h": "tty", "utmpx.h": "tty", "sys/ioctl.h": "tty",
    "sys/mman.h": "memory", "malloc.h": "memory", "alloca.h": "memory",
    "pthread.h": "threads", "semaphore.h": "threads", "threads.h": "threads", "stdatomic.h": "threads",
    "sys/socket.h": "network", "netdb.h": "network", "arpa/inet.h": "network", "netinet/in.h": "network",
    "netinet/ether.h": "network", "ifaddrs.h": "network", "net/if.h": "network", "resolv.h": "network",
    "netinet/tcp.h": "network", "poll.h": "file", "sys/select.h": "file", "sys/epoll.h": "file",
    "sys/eventfd.h": "file", "sys/signalfd.h": "signal",
    "sys/wait.h": "process", "spawn.h": "process", "sys/resource.h": "process", "sched.h": "process",
    "sys/prctl.h": "process", "pwd.h": "process", "grp.h": "process", "shadow.h": "process",
    "sys/capability.h": "process", "sys/personality.h": "process", "sys/ptrace.h": "process",
    "dlfcn.h": "process", "sys/auxv.h": "process", "setjmp.h": "other", "sys/syscall.h": "other",
    "math.h": "other", "complex.h": "other", "fenv.h": "other", "search.h": "other",
    "errno.h": "other", "syslog.h": "other", "sys/utsname.h": "other", "getopt.h": "other",
    "sys/random.h": "other", "sys/sysinfo.h": "other", "sys/reboot.h": "other", "sys/swap.h": "file",
    "sys/klog.h": "other", "assert.h": "other", "stdlib.h": "other", "unistd.h": "file",
    "sys/shm.h": "memory", "sys/sem.h": "threads", "sys/msg.h": "other", "sys/ipc.h": "other",
    "wordexp.h": "process", "paths.h": "other", "sys/fsuid.h": "process",
}

# headers that mix concerns: per-symbol overrides
OVERRIDE = {}
def _set(cat, names):
    for n in names.split():
        OVERRIDE[n] = cat
_set("memory", "malloc calloc realloc free posix_memalign aligned_alloc reallocarray valloc memalign "
     "malloc_usable_size brk sbrk getpagesize")
_set("process", "exit _Exit _exit abort atexit at_quick_exit quick_exit system getenv setenv unsetenv putenv "
     "clearenv secure_getenv fork vfork execl execle execlp execv execve execvp execvpe fexecve getpid getppid "
     "getpgid getpgrp setpgid setpgrp getsid setsid getuid geteuid getgid getegid setuid seteuid setgid setegid "
     "setreuid setregid setresuid setresgid getresuid getresgid getgroups setgroups initgroups getlogin "
     "getlogin_r nice alarm pause sleep usleep daemon getopt getopt_long getopt_long_only gethostname "
     "sethostname getdomainname sysconf confstr pathconf fpathconf issetugid")
_set("file", "mkstemp mkstemps mkostemp mkostemps mkdtemp mktemp realpath tmpfile tmpnam tempnam")
_set("dir", "chdir fchdir getcwd get_current_dir_name rmdir mkdir mkdirat chroot")
_set("tty", "isatty ttyname ttyname_r tcgetpgrp tcsetpgrp tcgetsid ctermid ioctl grantpt unlockpt ptsname "
     "ptsname_r posix_openpt getpass")
_set("string", "strtol strtoul strtoll strtoull strtod strtof strtold atoi atol atoll atof mblen mbtowc wctomb "
     "mbstowcs wcstombs strtoimax strtoumax")
_set("signal", "sigsetjmp siglongjmp")
_set("time", "times")
_set("stdio", "dprintf vdprintf")

@functools.lru_cache(None)
def _headers(root="/usr/include"):
    out = []
    for d, _, fs in os.walk(root):
        for f in fs:
            if f.endswith(".h"):
                p = os.path.join(d, f)
                try:
                    out.append((os.path.relpath(p, root), open(p, errors="replace").read()))
                except OSError:
                    pass
    # prefer top-level and well-known headers first when several declare a symbol
    out.sort(key=lambda h: (h[0] not in HEADER_CAT, h[0].count("/"), h[0]))
    return out

def header_of(sym):
    pat = re.compile(r"(?<![\w])" + re.escape(sym) + r"\s*\(|\b" + re.escape(sym) + r"\s*[;\[]")
    for rel, text in _headers():
        if rel.startswith(("c++/", "linux/", "asm", "fortify/", "bits/")):
            continue
        if pat.search(text):
            return rel
    return ""

def category(sym, header):
    if sym in OVERRIDE:
        return OVERRIDE[sym]
    if header in HEADER_CAT:
        return HEADER_CAT[header]
    if sym.startswith(("pthread_", "sem_", "thrd_", "mtx_", "cnd_")):
        return "threads"
    if sym.startswith(("__stack_chk", "__libc_start", "__errno", "__cxa", "__register", "__deregister")):
        return "other"
    return "other"
