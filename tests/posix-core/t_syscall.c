/*
 * INV-0451-1 (ADR-0451 item 1): every libposix definition returns the width its
 * C prototype declares. `syscall()` was `int` where <unistd.h> says `long`, as
 * `tee` and `sendmsg` were `int` for `ssize_t`: on AArch64 the upper half of
 * the result register was garbage, so -1 read as 4294967295 (a success).
 *
 *   t_syscall syscall   syscall() is a long; a number it does not answer gives
 *                       -1 and ENOSYS, as a 64-bit -1 (also with a number
 *                       whose low 32 bits are a valid number)
 *   t_syscall stubs     tee and sendmsg return ssize_t: -1 and ENOSYS
 *   t_syscall wide      a result above 2^32 survives: lseek's off_t past 4 GiB
 *
 * What syscall() answers beyond that (ADR-0451's fixed list) is a later part;
 * this program uses only numbers that list does not hold.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

/* The declared types, at compile time (the call's value is what is checked at
 * run time: a narrow definition passes this and fails below). */
_Static_assert(sizeof(syscall(1L)) == 8, "syscall() returns long");
_Static_assert(sizeof(tee(0, 0, 0, 0)) == 8, "tee() returns ssize_t");
_Static_assert(sizeof(sendmsg(0, 0, 0)) == 8, "sendmsg() returns ssize_t");

static int checks;

#define CHECK(cond)                                                             \
	do {                                                                    \
		checks++;                                                       \
		if (!(cond)) {                                                  \
			printf("t_syscall: FAIL line %d: %s (errno %d: %s)\n",  \
			       __LINE__, #cond, errno, strerror(errno));        \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

/* A result that must be the 64-bit -1 with errno ENOSYS, not 0xffffffff. */
#define ENOSYS64(expr)                                                          \
	do {                                                                    \
		errno = 0;                                                      \
		long long r_ = (long long)(expr);                               \
		CHECK(r_ == -1LL);                                              \
		CHECK((unsigned long long)r_ == 0xffffffffffffffffULL);         \
		CHECK(errno == ENOSYS);                                         \
	} while (0)

static void numbers(void)
{
	/* Numbers no table holds, in the range, far above it, negative, and with
	 * a listed number in the low 32 bits only. */
	ENOSYS64(syscall(9999L));
	ENOSYS64(syscall(4096L, 1L, 2L, 3L, 4L, 5L, 6L));
	ENOSYS64(syscall(-1L));
	ENOSYS64(syscall(0x7fffffffL));
	ENOSYS64(syscall((1L << 32) | 98L));
	ENOSYS64(syscall((1L << 40) + 1L));
	printf("t_syscall: syscall passed (%d checks)\n", checks);
}

static void stubs(void)
{
	ENOSYS64(tee(-1, -1, 4096, 0));
	ENOSYS64(sendmsg(-1, NULL, 0));
	printf("t_syscall: stubs passed (%d checks)\n", checks);
}

static void wide(void)
{
	const char *name = "/work/t_syscall.wide";
	int fd = open(name, O_RDWR | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0);
	/* off_t is 64 bits: the position past 4 GiB comes back whole. */
	off_t big = (off_t)5 << 30;
	CHECK(lseek(fd, big, SEEK_SET) == big);
	CHECK(lseek(fd, 0, SEEK_CUR) == big);
	CHECK(lseek(fd, 1, SEEK_CUR) == big + 1);
	CHECK(close(fd) == 0);
	CHECK(unlink(name) == 0);
	printf("t_syscall: wide passed (%d checks)\n", checks);
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "syscall";
	if (strcmp(mode, "syscall") == 0)
		numbers();
	else if (strcmp(mode, "stubs") == 0)
		stubs();
	else if (strcmp(mode, "wide") == 0)
		wide();
	else {
		printf("t_syscall: unknown mode %s\n", mode);
		return 2;
	}
	return 0;
}
