/* Zoraya's src/internal/syscall.h for musl 1.2.5 (ADR-0100 item 2).
 *
 * This file replaces musl's own. musl's retained C reaches the system through
 * __syscall(SYS_x, ...) and syscall(SYS_x, ...). Here each such call becomes a
 * call of a function of zoraya-posix, by name: __syscall(SYS_readv, a, b, c) is
 * zoraya_posix_readv(a, b, c). Every routed function takes `long` arguments, as
 * musl's call sites pass them (__scc), and returns a value or -errno, as the
 * Linux system call did, so that musl's handling of the result is unchanged.
 *
 * Only the names below are routed. A retained file that makes any other system
 * call names an undeclared function __zoraya_sys_SYS_<name>, which is an error
 * (-Werror=implicit-function-declaration): it fails to compile rather than
 * calling Linux (INV-0100-2). The <bits/syscall.h> of the sysroot defines no
 * SYS_ number at all.
 *
 * The list is the one the compiler found for the files ports/musl/musl.mk
 * retains (ADR-0100's open question); ADR-0141 records how it differs from
 * ADR-0100 item 2's estimate.
 */
#ifndef _INTERNAL_SYSCALL_H
#define _INTERNAL_SYSCALL_H

#include <features.h>
#include <errno.h>
#include <sys/syscall.h>

#ifndef SYSCALL_RLIM_INFINITY
#define SYSCALL_RLIM_INFINITY (~0ULL)
#endif

#ifndef SYSCALL_MMAP2_UNIT
#define SYSCALL_MMAP2_UNIT 4096ULL
#endif

#define __scc(X) ((long) (X))
typedef long syscall_arg_t;

hidden long __syscall_ret(unsigned long);

/* The two levels let a file's own `#define SYS_mmap2 SYS_mmap` apply before
 * the name is pasted. */
#define __zoraya_sys_paste(n, ...) __zoraya_sys_##n(__VA_ARGS__)
#define __zoraya_sys_call(n, ...) __zoraya_sys_paste(n, __VA_ARGS__)

#define __syscall(...) __zoraya_sys_call(__VA_ARGS__)
#define syscall(...) __syscall_ret(__syscall(__VA_ARGS__))
/* No cancellation points yet: deferred cancellation is step 3 (ADR-0305,
 * ADR-0314 item 4). */
#define __syscall_cp(...) __syscall(__VA_ARGS__)
#define syscall_cp(...) __syscall_ret(__syscall(__VA_ARGS__))

long zoraya_posix_read(long fd, long buf, long count);
long zoraya_posix_readv(long fd, long iov, long iovcnt);
long zoraya_posix_writev(long fd, long iov, long iovcnt);
long zoraya_posix_close(long fd);
long zoraya_posix_fcntl(long fd, long cmd, long arg);
long zoraya_posix_ioctl(long fd, long request, long arg);
long zoraya_posix_mmap(long addr, long length, long prot, long flags, long fd, long offset);
long zoraya_posix_set_tid_address(long tidptr);
long zoraya_posix_wait4(long pid, long status, long options, long usage);
_Noreturn void zoraya_posix_exit_group(long status);

#define __zoraya_sys_SYS_read(fd, buf, n) zoraya_posix_read(__scc(fd), __scc(buf), __scc(n))
#define __zoraya_sys_SYS_readv(fd, iov, n) zoraya_posix_readv(__scc(fd), __scc(iov), __scc(n))
#define __zoraya_sys_SYS_writev(fd, iov, n) zoraya_posix_writev(__scc(fd), __scc(iov), __scc(n))
#define __zoraya_sys_SYS_close(fd) zoraya_posix_close(__scc(fd))
#define __zoraya_sys_SYS_fcntl(fd, cmd, ...) \
	zoraya_posix_fcntl(__scc(fd), __scc(cmd), __zoraya_sys_arg0(__VA_ARGS__))
#define __zoraya_sys_SYS_ioctl(fd, req, arg) zoraya_posix_ioctl(__scc(fd), __scc(req), __scc(arg))
#define __zoraya_sys_SYS_mmap(a, l, p, fl, fd, off) \
	zoraya_posix_mmap(__scc(a), __scc(l), __scc(p), __scc(fl), __scc(fd), __scc(off))
#define __zoraya_sys_SYS_set_tid_address(p) zoraya_posix_set_tid_address(__scc(p))
/* _Exit calls exit_group, which ends the process; SYS_exit ends the calling
 * thread (src/thread: a thread's last act, ADR-0302 item 2). */
#define __zoraya_sys_SYS_exit_group(c) (zoraya_posix_exit_group(__scc(c)), 0L)
#define __zoraya_sys_SYS_exit(c) (zoraya_posix_thread_exit(__scc(c)), 0L)

/* musl's internal name for wait4 (src/stdio/pclose.c), which musl's own
 * syscall.h defines as the system call: here waitpid in zoraya-posix. */
#define __sys_wait4(pid, st, opt, ru) \
	zoraya_posix_wait4(__scc(pid), __scc(st), __scc(opt), __scc(ru))

/* fcntl's third argument is absent for F_GETFL and F_GETFD. */
#define __zoraya_sys_arg0(...) __scc(__VA_ARGS__ + 0)

/* Threads (ADR-0302 item 2). src/thread reaches the kernel through these. The
 * futex takes three to six arguments; the missing ones are 0. */
_Noreturn void zoraya_posix_thread_exit(long code);
long zoraya_posix_futex(long, long, long, long, long, long);
long zoraya_posix_rt_sigprocmask(long how, long set, long old, long size);
long zoraya_posix_tkill(long tid, long sig);
long zoraya_posix_sched_yield(void);

#define __zoraya_futex6(a, b, c, d, e, f, ...) \
	zoraya_posix_futex(__scc(a), __scc(b), __scc(c), __scc(d), __scc(e), __scc(f))
#define __zoraya_sys_SYS_futex(...) __zoraya_futex6(__VA_ARGS__, 0, 0, 0, 0)
#define __zoraya_sys_SYS_rt_sigprocmask(how, set, old, size) \
	zoraya_posix_rt_sigprocmask(__scc(how), __scc(set), __scc(old), __scc(size))
#define __zoraya_sys_SYS_tkill(tid, sig) zoraya_posix_tkill(__scc(tid), __scc(sig))
#define __zoraya_sys_SYS_sched_yield() zoraya_posix_sched_yield()
/* No robust lists in the kernel (musl walks its own at thread exit) and no
 * priorities or policies (ADR-0030): musl tolerates the answer. */
#define __zoraya_sys_SYS_set_robust_list(...) (-ENOSYS)
#define __zoraya_sys_SYS_get_robust_list(...) (-ENOSYS)
#define __zoraya_sys_SYS_sched_setscheduler(...) (-ENOSYS)
#define __zoraya_sys_SYS_sched_getscheduler(...) (-ENOSYS)
#define __zoraya_sys_SYS_sched_getparam(...) (-ENOSYS)
#define __zoraya_sys_SYS_sched_setparam(...) (-ENOSYS)

#endif
