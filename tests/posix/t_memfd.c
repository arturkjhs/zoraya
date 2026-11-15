/*
 * Memory descriptors through libposix in QEMU (ADR-0413, plan part F3): run
 * from dash in the POSIX session of make test-posix, through the broker. One
 * binary with named cases; each prints "t_memfd: NAME passed", or a FAIL line
 * and exits 1. The child of the plan's criterion is a fork child: handing a
 * descriptor to a spawned program waits for plan part F4 (the bootstrap record
 * carries it), and handing one over a Unix socket needs a spawn too; both are
 * checked over a fake kernel by crates/zoraya-posix/tests/memfd.rs, and
 * `t_unix memfd` sends one over a socketpair in one process.
 *
 *   t_memfd fork      the criterion: a 4 MiB memfd, written through a shared
 *                     mapping by the parent, read by a fork child through the
 *                     mapping it inherited and through a new mmap of the
 *                     descriptor; the child's write is the parent's; the size
 *                     is fixed from the first mapping (INV-0413-1)
 *   t_memfd seal      F_SEAL_FUTURE_WRITE: a new writable mapping is refused,
 *                     the old one still works, a read-only mapping reads, the
 *                     write of a fork child to its read-only mapping ends it
 *                     with SIGSEGV (INV-0413-2)
 *   t_memfd shm       shm_open(SHM_ANON) works, a name is EACCES, shm_unlink
 *                     is ENOENT (INV-0413-3)
 *   t_memfd exec      no mmap or mprotect gives an executable mapping
 *                     (INV-0413-5)
 *   t_memfd grow      a memfd used as a growing file: writes past the end
 *                     grow it in steps, until it is mapped
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static const char *test = "";

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_memfd: %s: FAIL line %d: %s (errno %d: %s)\n", \
			       test, __LINE__, #cond, errno, strerror(errno));  \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define FAILS(call, e) CHECK((call) == -1 && errno == (e))

static void passed(void)
{
	printf("t_memfd: %s passed\n", test);
	fflush(stdout);
}

static int reap(pid_t pid)
{
	int status = 0;
	pid_t got;
	do
		got = waitpid(pid, &status, 0);
	while (got == -1 && errno == EINTR);
	CHECK(got == pid);
	return status;
}

#define SIZE (4u << 20)

static void fork_case(void)
{
	int fd = memfd_create("f3", MFD_CLOEXEC | MFD_ALLOW_SEALING);
	CHECK(fd >= 0);
	CHECK(ftruncate(fd, SIZE) == 0);
	struct stat st;
	CHECK(fstat(fd, &st) == 0 && st.st_size == SIZE);
	unsigned char *m = mmap(NULL, SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(m != MAP_FAILED);
	for (size_t i = 0; i < SIZE; i++)
		m[i] = (unsigned char)(i % 251);
	/* The size is fixed from the first mapping, and the old size is fine. */
	FAILS(ftruncate(fd, SIZE / 2), EBUSY);
	CHECK(ftruncate(fd, SIZE) == 0);
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		/* The mapping is shared with the parent across the fork. */
		for (size_t i = 0; i < SIZE; i++)
			if (m[i] != (unsigned char)(i % 251))
				_exit(2);
		/* And a new mapping of the descriptor the child inherited. */
		unsigned char *n = mmap(NULL, SIZE, PROT_READ, MAP_SHARED, fd, 0);
		if (n == MAP_FAILED)
			_exit(3);
		if (memcmp(n, m, SIZE) != 0)
			_exit(4);
		/* pread reads the same memory. */
		unsigned char b[16];
		if (pread(fd, b, sizeof b, 1000) != (ssize_t)sizeof b || memcmp(b, m + 1000, sizeof b))
			_exit(5);
		memcpy(m, "child was here", 15);
		_exit(0);
	}
	int status = reap(pid);
	CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
	CHECK(memcmp(m, "child was here", 15) == 0);
	char b[15];
	CHECK(pread(fd, b, sizeof b, 0) == (ssize_t)sizeof b && memcmp(b, "child was here", 15) == 0);
	CHECK(munmap(m, SIZE) == 0);
	close(fd);
	passed();
}

static void seal_case(void)
{
	int fd = memfd_create("seal", MFD_ALLOW_SEALING);
	CHECK(fd >= 0);
	CHECK(ftruncate(fd, 8192) == 0);
	char *m = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(m != MAP_FAILED);
	strcpy(m, "sealed");
	int seals = F_SEAL_FUTURE_WRITE | F_SEAL_GROW | F_SEAL_SHRINK;
	CHECK(fcntl(fd, F_ADD_SEALS, seals) == 0);
	CHECK(fcntl(fd, F_GET_SEALS) == seals);
	/* The old mapping keeps its access; later ones are read-only. */
	strcpy(m, "again!");
	CHECK(mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0) == MAP_FAILED && errno == EPERM);
	char *ro = mmap(NULL, 4096, PROT_READ, MAP_SHARED, fd, 0);
	CHECK(ro != MAP_FAILED);
	CHECK(strcmp(ro, "again!") == 0);
	FAILS(write(fd, "x", 1), EPERM);
	FAILS(ftruncate(fd, 4096), EPERM);
	FAILS(ftruncate(fd, 16384), EPERM);
	/* F_SEAL_WRITE is refused while a writable mapping exists. */
	FAILS(fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE), EBUSY);
	/* A fork child that writes to its read-only mapping is ended by SIGSEGV. */
	pid_t pid = fork();
	CHECK(pid >= 0);
	if (pid == 0) {
		*(volatile char *)ro = 1;
		_exit(0);
	}
	int status = reap(pid);
	CHECK(WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV);
	/* A sealed descriptor without write access in the parent's view too. */
	CHECK(strcmp(ro, "again!") == 0);
	close(fd);
	passed();
}

static void shm_case(void)
{
	int fd = shm_open(SHM_ANON, O_RDWR, 0600);
	CHECK(fd >= 0);
	CHECK((fcntl(fd, F_GETFD) & FD_CLOEXEC) != 0);
	CHECK(ftruncate(fd, 4096) == 0);
	char *m = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(m != MAP_FAILED);
	strcpy(m, "anon");
	char b[5];
	CHECK(pread(fd, b, 5, 0) == 5 && strcmp(b, "anon") == 0);
	/* No names. */
	FAILS(shm_open("/f3", O_RDWR | O_CREAT, 0600), EACCES);
	FAILS(shm_open("/f3", O_RDONLY, 0), EACCES);
	FAILS(shm_unlink("/f3"), ENOENT);
	FAILS(shm_open(SHM_ANON, O_RDONLY, 0), EINVAL);
	CHECK(munmap(m, 4096) == 0);
	close(fd);
	passed();
}

static void exec_case(void)
{
	int fd = memfd_create("x", 0);
	CHECK(fd >= 0 && ftruncate(fd, 4096) == 0);
	for (int prot = 0; prot < 8; prot++) {
		if (!(prot & PROT_EXEC))
			continue;
		void *p = mmap(NULL, 4096, prot, MAP_SHARED, fd, 0);
		CHECK(p == MAP_FAILED && errno == EACCES);
		p = mmap(NULL, 4096, prot, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		CHECK(p == MAP_FAILED && errno == EACCES);
		p = mmap(NULL, 4096, prot, MAP_PRIVATE, fd, 0);
		CHECK(p == MAP_FAILED && errno == EACCES);
	}
	void *rw = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	CHECK(rw != MAP_FAILED);
	FAILS(mprotect(rw, 4096, PROT_READ | PROT_EXEC), EACCES);
	FAILS(mprotect(rw, 4096, PROT_EXEC), EACCES);
	/* A memory object is never mapped executable: a read-write mapping keeps
	 * being one, and a descriptor that was never mapped shared is not fixed. */
	CHECK(munmap(rw, 4096) == 0);
	CHECK(ftruncate(fd, 8192) == 0);
	close(fd);
	passed();
}

static void grow_case(void)
{
	int fd = memfd_create("grow", 0);
	CHECK(fd >= 0);
	char block[1000];
	for (int i = 0; i < 100; i++) {
		memset(block, i, sizeof block);
		CHECK(write(fd, block, sizeof block) == (ssize_t)sizeof block);
	}
	struct stat st;
	CHECK(fstat(fd, &st) == 0 && st.st_size == 100000);
	CHECK(lseek(fd, 0, SEEK_SET) == 0);
	for (int i = 0; i < 100; i++) {
		CHECK(read(fd, block, sizeof block) == (ssize_t)sizeof block);
		CHECK((unsigned char)block[0] == i && (unsigned char)block[999] == i);
	}
	char more;
	CHECK(read(fd, &more, 1) == 0);
	/* Once mapped, the file is as big as it will be. */
	char *m = mmap(NULL, 100000, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	CHECK(m != MAP_FAILED);
	CHECK(lseek(fd, 0, SEEK_END) == 100000);
	FAILS(write(fd, "x", 1), EFBIG);
	CHECK(munmap(m, 100000) == 0);
	close(fd);
	passed();
}

int main(int argc, char **argv)
{
	if (argc < 2)
		return 2;
	test = argv[1];
	if (!strcmp(argv[1], "fork"))
		fork_case();
	else if (!strcmp(argv[1], "seal"))
		seal_case();
	else if (!strcmp(argv[1], "shm"))
		shm_case();
	else if (!strcmp(argv[1], "exec"))
		exec_case();
	else if (!strcmp(argv[1], "grow"))
		grow_case();
	else
		return 2;
	return 0;
}
