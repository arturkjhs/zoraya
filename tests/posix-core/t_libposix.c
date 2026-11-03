/*
 * The criterion of stage 6c (docs/research/stage-6/plan.md, TASK-20): a
 * static C program, built with zcc against libc.a, run by posixrun against
 * fsd. It creates, writes, reads, renames, stats and removes files through
 * libposix, with Ukrainian and Czech names, directories, dup and dup2 sharing
 * one position, a pipe between two descriptors of one process, malloc under
 * load, stdio, and hostile inputs at the C boundary.
 *
 *   t_libposix files     the criterion; "t_libposix: files passed"
 *   t_libposix env       prints the environment (INV-0107-3)
 *   t_libposix refused   names a path outside its table twice and a missing
 *                        one inside (INV-0153-7); run with a table without /
 *   t_libposix stdout N  writes N lines to fd 1 (a shared file, posixrun -o)
 *   t_libposix fault     reads a file into unmapped memory: the process
 *                        faults, the service and the kernel go on
 *   t_libposix bench     the measurements of TASK-20
 *
 * Every check prints its line on failure and the program exits 1.
 */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

/* Only the tier list's names (docs/adr/posix-surface-T1.txt): mkdirat and
 * unlinkat stand for mkdir and rmdir, which it does not have. */
extern char **environ;

static int checks;

#define CHECK(cond)                                                             \
	do {                                                                    \
		checks++;                                                       \
		if (!(cond)) {                                                  \
			printf("t_libposix: FAIL line %d: %s (errno %d: %s)\n", \
			       __LINE__, #cond, errno, strerror(errno));        \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

/* A call that must fail with `e`. */
#define FAILS(call, e) CHECK((errno = 0, (call) == -1) && errno == (e))

static void write_all(int fd, const void *p, size_t n)
{
	CHECK(write(fd, p, n) == (ssize_t)n);
}

static void read_exactly(int fd, char *out, size_t n)
{
	CHECK(read(fd, out, n) == (ssize_t)n);
}

static void a_file_round_trip(const char *name, const char *renamed)
{
	char back[256];
	struct stat st;
	int fd = open(name, O_RDWR | O_CREAT | O_EXCL, 0644);
	CHECK(fd >= 0);
	write_all(fd, name, strlen(name));
	CHECK(lseek(fd, 0, SEEK_SET) == 0);
	read_exactly(fd, back, strlen(name));
	CHECK(memcmp(back, name, strlen(name)) == 0);
	CHECK(read(fd, back, sizeof back) == 0);
	CHECK(fstat(fd, &st) == 0);
	CHECK(S_ISREG(st.st_mode) && (st.st_mode & 0777) == 0644);
	CHECK(st.st_size == (off_t)strlen(name) && st.st_nlink == 1);
	CHECK(st.st_atime == st.st_mtime);
	CHECK(close(fd) == 0);
	CHECK(rename(name, renamed) == 0);
	FAILS(open(name, O_RDONLY), ENOENT);
	CHECK(stat(renamed, &st) == 0 && st.st_size == (off_t)strlen(name));
	CHECK(unlink(renamed) == 0);
	FAILS(stat(renamed, &st), ENOENT);
}

static void files(void)
{
	a_file_round_trip("notes.txt", "notes.bak");
	a_file_round_trip("Київ.txt", "Львів.txt");
	a_file_round_trip("Plzeň — žluťoučký kůň.txt", "Příliš.txt");

	/* The mode of a create, masked; ids differ; stat by path agrees. */
	struct stat a, b, c;
	int fa = open("a", O_WRONLY | O_CREAT | O_TRUNC, 0777);
	int fb = open("b", O_WRONLY | O_CREAT | O_TRUNC, 0640);
	CHECK(fa >= 0 && fb >= 0);
	CHECK(fstat(fa, &a) == 0 && fstat(fb, &b) == 0 && stat("a", &c) == 0);
	CHECK((a.st_mode & 0777) == 0755 && (b.st_mode & 0777) == 0640);
	CHECK(a.st_ino != b.st_ino && a.st_ino == c.st_ino && a.st_dev == c.st_dev);
	CHECK(access("a", X_OK) == 0);
	FAILS(access("b", X_OK), EACCES);
	CHECK(fchmod(fb, 0444) == 0);
	FAILS(open("b", O_WRONLY | O_TRUNC), EACCES);
	CHECK(close(fa) == 0 && close(fb) == 0);
	CHECK(unlink("a") == 0 && unlink("b") == 0);

	/* Directories, with names from two alphabets. */
	CHECK(mkdirat(AT_FDCWD, "тека", 0755) == 0);
	FAILS(mkdirat(AT_FDCWD, "тека", 0755), EEXIST);
	CHECK(mkdirat(AT_FDCWD, "тека/složka", 0700) == 0);
	const char *names[] = {"тека/один", "тека/dva", "тека/tři"};
	for (int i = 0; i < 3; i++) {
		int fd = open(names[i], O_WRONLY | O_CREAT, 0600);
		CHECK(fd >= 0 && close(fd) == 0);
	}
	DIR *d = opendir("тека");
	CHECK(d != NULL);
	int seen = 0, dirs = 0;
	struct dirent *e;
	while ((e = readdir(d)) != NULL) {
		seen++;
		if (e->d_type == DT_DIR)
			dirs++;
		/* ADR-0014 rule 7: d_ino is the object's id, as st_ino is. */
		char named[300];
		struct stat es;
		snprintf(named, sizeof named, "тека/%s", e->d_name);
		CHECK(lstat(named, &es) == 0 && es.st_ino == e->d_ino);
	}
	CHECK(seen == 4 && dirs == 1);
	CHECK(closedir(d) == 0);
	FAILS(unlinkat(AT_FDCWD, "тека", AT_REMOVEDIR), ENOTEMPTY);
	FAILS(unlink("тека/složka"), EISDIR);
	CHECK(unlinkat(AT_FDCWD, "тека/složka", AT_REMOVEDIR) == 0);
	for (int i = 0; i < 3; i++)
		CHECK(remove(names[i]) == 0);
	CHECK(unlinkat(AT_FDCWD, "тека", AT_REMOVEDIR) == 0);

	/* cwd. */
	char cwd[256];
	CHECK(mkdirat(AT_FDCWD, "deep", 0755) == 0 && mkdirat(AT_FDCWD, "deep/er", 0755) == 0);
	CHECK(chdir("deep/er") == 0);
	CHECK(getcwd(cwd, sizeof cwd) != NULL && strcmp(cwd, "/deep/er") == 0);
	CHECK(chdir("../..") == 0);
	CHECK(getcwd(cwd, sizeof cwd) != NULL && strcmp(cwd, "/") == 0);
	CHECK(getcwd(cwd, 1) == NULL && errno == ERANGE);
	CHECK(unlinkat(AT_FDCWD, "deep/er", AT_REMOVEDIR) == 0 && unlinkat(AT_FDCWD, "deep", AT_REMOVEDIR) == 0);

	/* dup and dup2 share one position; a second open has its own. */
	int fd = open("shared", O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
	CHECK(fd >= 0);
	int copy = dup(fd);
	CHECK(copy >= 0 && copy != fd);
	CHECK(fcntl(fd, F_GETFD) == FD_CLOEXEC && fcntl(copy, F_GETFD) == 0);
	write_all(fd, "abc", 3);
	CHECK(lseek(copy, 0, SEEK_CUR) == 3);
	write_all(copy, "def", 3);
	CHECK(lseek(fd, 0, SEEK_CUR) == 6);
	CHECK(dup2(fd, 20) == 20);
	CHECK(lseek(20, 1, SEEK_SET) == 1);
	char two[2];
	read_exactly(fd, two, 2);
	CHECK(memcmp(two, "bc", 2) == 0);
	int other = open("shared", O_RDONLY);
	char three[3];
	read_exactly(other, three, 3);
	CHECK(memcmp(three, "abc", 3) == 0 && lseek(fd, 0, SEEK_CUR) == 3);
	CHECK(dup2(other, copy) == copy);
	CHECK(lseek(copy, 0, SEEK_CUR) == 3 && dup2(copy, copy) == copy);
	int app = open("shared", O_WRONLY | O_APPEND);
	CHECK(app >= 0 && (fcntl(app, F_GETFL) & O_APPEND));
	write_all(app, "!", 1);
	CHECK(lseek(app, 0, SEEK_CUR) == 7);
	CHECK(close(fd) == 0 && close(copy) == 0 && close(20) == 0);
	CHECK(close(other) == 0 && close(app) == 0 && unlink("shared") == 0);

	/* A pipe between two descriptors of one process. */
	int p[2];
	CHECK(pipe(p) == 0 && p[0] < p[1]);
	write_all(p[1], "through a pipe", 14);
	char buf[64];
	CHECK(read(p[0], buf, sizeof buf) == 14 && memcmp(buf, "through a pipe", 14) == 0);
	FAILS(lseek(p[0], 0, SEEK_SET), ESPIPE);
	struct stat ps;
	CHECK(fstat(p[0], &ps) == 0 && S_ISFIFO(ps.st_mode));
	struct iovec v[3] = {{"wri", 3}, {"te", 2}, {"v", 1}};
	CHECK(writev(p[1], v, 3) == 6);
	CHECK(read(p[0], buf, sizeof buf) == 6 && memcmp(buf, "writev", 6) == 0);
	CHECK(close(p[1]) == 0);
	CHECK(read(p[0], buf, sizeof buf) == 0);
	CHECK(close(p[0]) == 0);

	/* stdio over the shim: fopen, fprintf, fgets, fseek, fclose. */
	FILE *f = fopen("stdio.txt", "w+");
	CHECK(f != NULL);
	CHECK(fprintf(f, "line %d: %s\n", 1, "Київ") > 0);
	CHECK(fprintf(f, "line %d: %.3f\n", 2, 3.14159) > 0);
	CHECK(fseek(f, 0, SEEK_SET) == 0);
	char line[64];
	CHECK(fgets(line, sizeof line, f) && strcmp(line, "line 1: Київ\n") == 0);
	CHECK(fgets(line, sizeof line, f) && strcmp(line, "line 2: 3.142\n") == 0);
	CHECK(fclose(f) == 0);
	CHECK(fopen("missing/x", "r") == NULL && errno == ENOENT);
	CHECK(remove("stdio.txt") == 0);

	/* /dev/null. */
	int null = open("/dev/null", O_RDWR);
	CHECK(null >= 0 && write(null, "x", 1) == 1 && read(null, buf, 1) == 0);
	CHECK(close(null) == 0);

	/* Links. */
	fd = open("target", O_WRONLY | O_CREAT, 0644);
	write_all(fd, "linked", 6);
	CHECK(close(fd) == 0);
	CHECK(symlinkat("target", AT_FDCWD, "link") == 0);
	CHECK(readlink("link", buf, sizeof buf) == 6 && memcmp(buf, "target", 6) == 0);
	fd = open("link", O_RDONLY);
	CHECK(fd >= 0 && read(fd, buf, sizeof buf) == 6 && close(fd) == 0);
	struct stat ls;
	CHECK(lstat("link", &ls) == 0 && S_ISLNK(ls.st_mode));
	CHECK(unlink("link") == 0 && unlink("target") == 0);

	/* malloc under load: many sizes, patterns checked, realloc, calloc. */
	enum { N = 2000 };
	unsigned char **blocks = calloc(N, sizeof *blocks);
	size_t *sizes = calloc(N, sizeof *sizes);
	CHECK(blocks && sizes);
	uint32_t x = 12345;
	for (int round = 0; round < 6; round++) {
		for (int i = 0; i < N; i++) {
			x = x * 1103515245 + 12345;
			if (blocks[i] && (x & 1)) {
				for (size_t k = 0; k < sizes[i]; k++)
					if (blocks[i][k] != (unsigned char)(i + k))
						CHECK(blocks[i][k] == (unsigned char)(i + k));
				free(blocks[i]);
				blocks[i] = NULL;
			}
			if (!blocks[i]) {
				size_t n = (x >> 8) % ((x & 16) ? 20000 : 300) + 1;
				blocks[i] = malloc(n);
				CHECK(blocks[i] != NULL && ((uintptr_t)blocks[i] & 15) == 0);
				for (size_t k = 0; k < n; k++)
					blocks[i][k] = (unsigned char)(i + k);
				sizes[i] = n;
			}
		}
		int i = round * 7 % N;
		unsigned char *grown = realloc(blocks[i], sizes[i] * 3);
		CHECK(grown != NULL);
		for (size_t k = 0; k < sizes[i]; k++)
			if (grown[k] != (unsigned char)(i + k))
				CHECK(grown[k] == (unsigned char)(i + k));
		for (size_t k = sizes[i]; k < sizes[i] * 3; k++)
			grown[k] = (unsigned char)(i + k);
		blocks[i] = grown;
		sizes[i] *= 3;
	}
	for (int i = 0; i < N; i++)
		free(blocks[i]);
	free(blocks);
	free(sizes);
	char *big = malloc(8 << 20);
	CHECK(big != NULL);
	memset(big, 0x5a, 8 << 20);
	free(big);
	/* Through a volatile pointer: clang knows calloc, and at -O2 may fold
	 * an overflowing one to NULL or read errno from before the call (LLVM
	 * models the malloc family as touching no other memory). */
	void *(*volatile calloc_p)(size_t, size_t) = calloc;
	errno = 0;
	void *none = calloc_p(SIZE_MAX / 2, 4);
	int calloc_errno = errno;
	CHECK(none == NULL && calloc_errno == ENOMEM);

	/* Hostile inputs: bad fds, NULL where POSIX allows EFAULT, absurd
	 * sizes, control characters, a leading / outside the table. */
	int bad[] = {-1, 77, 511, 512, 0x7fffffff};
	for (unsigned i = 0; i < sizeof bad / sizeof *bad; i++) {
		FAILS(read(bad[i], buf, 1), EBADF);
		FAILS(write(bad[i], buf, 1), EBADF);
		FAILS(close(bad[i]), EBADF);
		FAILS(lseek(bad[i], 0, SEEK_SET), EBADF);
		FAILS(dup(bad[i]), EBADF);
		FAILS(fstat(bad[i], &ls), EBADF);
		FAILS(fcntl(bad[i], F_GETFL), EBADF);
	}
	FAILS(open(NULL, O_RDONLY), EFAULT);
	FAILS(stat(NULL, &ls), EFAULT);
	FAILS(rename(NULL, "x"), EFAULT);
	FAILS(mkdirat(AT_FDCWD, NULL, 0), EFAULT);
	FAILS(pipe(NULL), EFAULT);
	fd = open("hostile", O_RDWR | O_CREAT, 0600);
	CHECK(fd >= 0);
	FAILS(read(fd, NULL, 10), EFAULT);
	FAILS(write(fd, NULL, 10), EFAULT);
	CHECK(read(fd, NULL, 0) == 0);
	FAILS(write(fd, buf, (size_t)-1), EINVAL);
	FAILS(lseek(fd, -1, SEEK_SET), EINVAL);
	FAILS(lseek(fd, 0, 7), EINVAL);
	CHECK(close(fd) == 0 && unlink("hostile") == 0);
	FAILS(open("ctl\001name", O_WRONLY | O_CREAT, 0600), EINVAL);
	FAILS(open("bad\xff", O_RDONLY), EILSEQ);
	FAILS(open("", O_RDONLY), ENOENT);
	char longer[5000];
	memset(longer, 'a', sizeof longer - 1);
	longer[sizeof longer - 1] = 0;
	FAILS(open(longer, O_RDONLY), ENAMETOOLONG);
	FAILS(open("/../../../nope", O_RDONLY), ENOENT);
	int pp[2];
	CHECK(pipe(pp) == 0);
	/* An unmapped pointer to a stream: the kernel checks it. */
	FAILS(write(pp[1], (void *)0x10, 4), EFAULT);
	write_all(pp[1], "x", 1);
	FAILS(read(pp[0], (void *)0x10, 4), EFAULT);
	CHECK(close(pp[0]) == 0 && close(pp[1]) == 0);

	/* What the process knows of itself. */
	struct utsname u;
	CHECK(uname(&u) == 0 && strcmp(u.sysname, "Zoraya") == 0 && strcmp(u.machine, "aarch64") == 0);
	CHECK(getuid() == 1000 && geteuid() == 1000 && getgid() == 1000);
	CHECK(umask(077) == 022 && umask(022) == 077);
	CHECK(isatty(1) == 0 && errno == ENOTTY);
	struct timespec t0, t1;
	CHECK(clock_gettime(CLOCK_MONOTONIC, &t0) == 0);
	CHECK(clock_gettime(CLOCK_MONOTONIC, &t1) == 0);
	CHECK(t1.tv_sec > t0.tv_sec || (t1.tv_sec == t0.tv_sec && t1.tv_nsec >= t0.tv_nsec));
	/* Process functions are 6d's. */
	FAILS(fork(), ENOSYS);

	printf("t_libposix: files passed (%d checks)\n", checks);
}

static void env(void)
{
	int n = 0;
	for (char **e = environ; *e; e++, n++)
		printf("t_libposix: env[%d]=%s\n", n, *e);
	printf("t_libposix: env count %d\n", n);
}

static void refused(void)
{
	/* Outside every entry, twice: one report. Missing inside: none. */
	FAILS(open("/etc/passwd", O_RDONLY), ENOENT);
	FAILS(open("/etc/passwd", O_RDONLY), ENOENT);
	FAILS(open("/work/missing.h", O_RDONLY), ENOENT);
	FAILS(mkdirat(AT_FDCWD, "/elsewhere/new", 0755), ENOENT);
	printf("t_libposix: refused passed\n");
}

static void to_stdout(int lines)
{
	for (int i = 0; i < lines; i++)
		printf("line %d\n", i);
	fflush(stdout);
	fprintf(stderr, "t_libposix: wrote %d lines, at %lld\n", lines, (long long)lseek(1, 0, SEEK_CUR));
}

static void fault(void)
{
	int fd = open("fault.txt", O_RDWR | O_CREAT | O_TRUNC, 0600);
	CHECK(fd >= 0);
	write_all(fd, "0123456789", 10);
	CHECK(lseek(fd, 0, SEEK_SET) == 0);
	fprintf(stderr, "t_libposix: reading into unmapped memory\n");
	read(fd, (void *)0x10, 10);
	fprintf(stderr, "t_libposix: FAIL the read returned\n");
	exit(1);
}

static double now(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec + t.tv_nsec / 1e9;
}

static void bench(void)
{
	enum { OPENS = 200, SEEKS = 500 };
	int fd = open("bench.txt", O_RDWR | O_CREAT | O_TRUNC, 0644);
	CHECK(fd >= 0 && close(fd) == 0);
	double t = now();
	for (int i = 0; i < OPENS; i++) {
		fd = open("bench.txt", O_RDONLY);
		CHECK(fd >= 0 && close(fd) == 0);
	}
	double open_us = (now() - t) / OPENS * 1e6;
	fd = open("bench.txt", O_RDWR);
	t = now();
	for (int i = 0; i < SEEKS; i++)
		CHECK(lseek(fd, 0, SEEK_CUR) == 0);
	double seek_us = (now() - t) / SEEKS * 1e6;
	static char block[1 << 20];
	memset(block, 'z', sizeof block);
	double rates[4];
	size_t sizes[2] = {4096, 1 << 20};
	for (int s = 0; s < 2; s++) {
		size_t total = 8 << 20, n = sizes[s];
		CHECK(lseek(fd, 0, SEEK_SET) == 0);
		t = now();
		for (size_t done = 0; done < total; done += n)
			write_all(fd, block, n);
		rates[2 * s] = total / (now() - t) / 1e6;
		CHECK(lseek(fd, 0, SEEK_SET) == 0);
		t = now();
		for (size_t done = 0; done < total; done += n)
			read_exactly(fd, block, n);
		rates[2 * s + 1] = total / (now() - t) / 1e6;
	}
	CHECK(close(fd) == 0 && unlink("bench.txt") == 0);
	printf("t_libposix: bench open+close %.1f us, lseek %.1f us, "
	       "4 KiB write %.1f MB/s read %.1f MB/s, 1 MiB write %.1f MB/s read %.1f MB/s\n",
	       open_us, seek_us, rates[0], rates[1], rates[2], rates[3]);
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "files";
	if (strcmp(mode, "files") == 0)
		files();
	else if (strcmp(mode, "env") == 0)
		env();
	else if (strcmp(mode, "refused") == 0)
		refused();
	else if (strcmp(mode, "stdout") == 0)
		to_stdout(argc > 2 ? atoi(argv[2]) : 10);
	else if (strcmp(mode, "fault") == 0)
		fault();
	else if (strcmp(mode, "bench") == 0)
		bench();
	else {
		printf("t_libposix: unknown mode %s\n", mode);
		return 2;
	}
	return 0;
}
