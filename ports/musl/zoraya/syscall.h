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
 * The list is the one the compiler found for the files ports/musl/files.mk
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
/* There are no cancellation points: one thread (ADR-0100 item 5). */
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
/* _Exit calls exit_group, then exit in a loop; both end the process. */
#define __zoraya_sys_SYS_exit_group(c) (zoraya_posix_exit_group(__scc(c)), 0L)
#define __zoraya_sys_SYS_exit(c) (zoraya_posix_exit_group(__scc(c)), 0L)

/* fcntl's third argument is absent for F_GETFL and F_GETFD. */
#define __zoraya_sys_arg0(...) __scc(__VA_ARGS__ + 0)

/* src/internal/pthread_impl.h's __wake and __futexwait are compiled into
 * every file that includes it. A process has one thread (ADR-0100 item 5):
 * no futex ever has a waiter, and nobody waits on a lock another thread holds,
 * so both are answered here with ENOSYS without a call. */
#define __zoraya_sys_SYS_futex(...) (-ENOSYS)

#endif
