/*
 * Plan part F6 (ADR-0415 item 2, INV-0415-2, proposed ADR-0438): a program that
 * spawns itself three times holds its read-only pages once.
 *
 * Three roles of one executable:
 *
 *   t_shared            the driver T, started by the shell. It starts L, its
 *                       own image, and asks it for spawns, reading what L's job
 *                       is charged between them (zoraya_child_memory).
 *   t_shared launcher   L. On a byte of stdin it spawns a "hold" child of the
 *                       image it was started from ('s': /proc/self/exe) or of
 *                       the file that image came from ('p': its path, which
 *                       reads the file and makes an image of its own).
 *   t_shared hold       a child: says "h" and waits for stdin to end.
 *
 * The job that makes an image pays for its pages as long as the image or any
 * process of it lives. L makes the image in the one case ('p'); in the other
 * the pages are the ones L itself runs from, which the job above it paid for.
 * So what T reads of L's job grows by the image's pages after one 'p' and by
 * nothing after an 's', however many; and each child's own job holds its
 * own pages only, 's' or 'p' alike.
 */
#define _GNU_SOURCE
#include <stdint.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#include <zoraya/posix.h>

extern char **environ;

#define FAIL(...)                                                      \
	do {                                                           \
		fprintf(stderr, "t_shared: FAIL %s:%d: ", __FILE__, __LINE__); \
		fprintf(stderr, __VA_ARGS__);                          \
		fprintf(stderr, "\n");                                 \
		exit(1);                                               \
	} while (0)

#define PAGE 4096ULL

/* What an image of this program keeps (ADR-0438 item 1, `Program::kept`): the
 * whole pages of every segment that is not writable, the zero tail included,
 * and the pages with file bytes of a writable one. Read from the program
 * headers of the file this process runs. */
static unsigned long long kept_exact(const char *exe)
{
	unsigned char h[64 + 56 * 8];
	unsigned long long total = 0;
	int fd = open(exe, O_RDONLY);
	ssize_t n;
	unsigned phnum, i;

	if (fd < 0)
		FAIL("open %s: errno %d", exe, errno);
	n = read(fd, h, sizeof h);
	close(fd);
	if (n < 64 || memcmp(h, "\177ELF", 4))
		FAIL("not an ELF file");
	phnum = h[56] | (h[57] << 8);
	if (phnum > 8 || n < 64 + 56 * (ssize_t)phnum)
		FAIL("%u program headers", phnum);
	for (i = 0; i < phnum; i++) {
		const unsigned char *ph = h + 64 + 56 * i;
		unsigned long long vaddr = 0, filesz = 0, memsz = 0, start, end;
		unsigned type = ph[0] | (ph[1] << 8) | (ph[2] << 16) | ((unsigned)ph[3] << 24);
		unsigned flags = ph[4] | (ph[5] << 8) | (ph[6] << 16) | ((unsigned)ph[7] << 24);
		for (int k = 7; k >= 0; k--) {
			vaddr = vaddr << 8 | ph[16 + k];
			filesz = filesz << 8 | ph[32 + k];
			memsz = memsz << 8 | ph[40 + k];
		}
		if (type != 1)
			continue;
		start = vaddr & ~(PAGE - 1);
		if (!(flags & 2))
			end = vaddr + memsz;
		else if (filesz == 0)
			continue;
		else
			end = vaddr + filesz;
		end = (end + PAGE - 1) & ~(PAGE - 1);
		total += end - start;
	}
	return total;
}

/* ---------------------------------------------------------------- hold */

static int hold(void)
{
	char b;
	if (write(1, "h\n", 2) != 2)
		return 1;
	while (read(0, &b, 1) > 0)
		;
	return 0;
}

/* ---------------------------------------------------------------- launcher */

struct kid {
	pid_t pid;
	int stdin_w;
};
static struct kid kids[8];
static int nkids;

/* A hold child of `path`, with a pipe for its stdin and one for its stdout,
 * and waits until it says "h": it ran. */
static int start_hold(const char *path)
{
	int in[2], out[2];
	char buf[2];
	posix_spawn_file_actions_t fa;
	pid_t pid;
	char *argv[] = {"t_shared", "hold", NULL};
	int r;

	if (nkids == 8 || pipe(in) || pipe(out))
		return -1;
	posix_spawn_file_actions_init(&fa);
	posix_spawn_file_actions_adddup2(&fa, in[0], 0);
	posix_spawn_file_actions_adddup2(&fa, out[1], 1);
	posix_spawn_file_actions_addclose(&fa, in[0]);
	posix_spawn_file_actions_addclose(&fa, in[1]);
	posix_spawn_file_actions_addclose(&fa, out[0]);
	posix_spawn_file_actions_addclose(&fa, out[1]);
	r = posix_spawn(&pid, path, &fa, NULL, argv, environ);
	posix_spawn_file_actions_destroy(&fa);
	close(in[0]);
	close(out[1]);
	if (r) {
		close(in[1]);
		close(out[0]);
		errno = r;
		return -1;
	}
	if (read(out[0], buf, 2) != 2 || buf[0] != 'h') {
		close(in[1]);
		close(out[0]);
		return -1;
	}
	close(out[0]);
	kids[nkids].pid = pid;
	kids[nkids].stdin_w = in[1];
	nkids++;
	return 0;
}

static int launcher(void)
{
	char c;
	char exe[PATH_MAX];
	ssize_t n;

	while (read(0, &c, 1) == 1) {
		int bad = 0;
		if (c == 's') {
			bad = start_hold("/proc/self/exe");
		} else if (c == 'p') {
			n = readlink("/proc/self/exe", exe, sizeof exe - 1);
			if (n <= 0)
				bad = -1;
			else {
				exe[n] = 0;
				bad = start_hold(exe);
			}
		} else if (c == 'r') {
			/* Reads 256 KiB of the executable's file: a process's first
			 * transfer above the inline size makes its transfer buffer
			 * (1 MiB), which stays. */
			static char big[256 * 1024];
			int fd;
			n = readlink("/proc/self/exe", exe, sizeof exe - 1);
			if (n <= 0)
				bad = -1;
			else {
				exe[n] = 0;
				fd = open(exe, O_RDONLY);
				if (fd < 0 || read(fd, big, sizeof big) != (ssize_t)sizeof big)
					bad = -1;
				if (fd >= 0)
					close(fd);
			}
		} else if (c == 'u') {
			for (int i = 0; i < nkids; i++) {
				unsigned long long used = 0;
				if (zoraya_child_memory(kids[i].pid, &used))
					bad = -1;
				printf("u %d %llu\n", i, used);
			}
		} else {
			bad = -1;
		}
		if (bad)
			printf("e %d\n", errno);
		else
			printf("k\n");
		fflush(stdout);
	}
	for (int i = 0; i < nkids; i++)
		close(kids[i].stdin_w);
	for (int i = 0; i < nkids; i++) {
		int st;
		if (waitpid(kids[i].pid, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st))
			return 1;
	}
	return 0;
}

/* ---------------------------------------------------------------- driver */

static int to_l, from_l;

/* One byte to L and its reply: the lines it printed, ended by "k". */
static void ask(char c, unsigned long long *used, int max)
{
	char line[128];
	int len = 0;

	if (write(to_l, &c, 1) != 1)
		FAIL("write to the launcher");
	for (;;) {
		char b;
		if (read(from_l, &b, 1) != 1)
			FAIL("the launcher ended");
		if (b != '\n') {
			if (len < (int)sizeof line - 1)
				line[len++] = b;
			continue;
		}
		line[len] = 0;
		len = 0;
		if (line[0] == 'k')
			return;
		if (line[0] == 'u') {
			int i;
			unsigned long long v;
			if (sscanf(line, "u %d %llu", &i, &v) != 2 || i < 0 || i >= max)
				FAIL("bad line '%s'", line);
			used[i] = v;
			continue;
		}
		FAIL("the launcher said '%s' to '%c'", line, c);
	}
}

/* L's job's memory once it has stopped changing: a file's object that L
 * closed goes to the reaper with its charge, and the reading before it is
 * freed is too large. The loop ends when two readings agree; the runner's own
 * limit ends a run where they never do. */
static unsigned long long memory_of(pid_t pid)
{
	unsigned long long last = 0, used = 0;
	for (int i = 0; i < 100000; i++) {
		if (zoraya_child_memory(pid, &used))
			FAIL("zoraya_child_memory(%d): errno %d", (int)pid, errno);
		if (i > 0 && used == last)
			return used;
		last = used;
		usleep(20000);
	}
	FAIL("the memory of %d never settled", (int)pid);
	return 0;
}

/* Waits for the file the script writes after it has read `top`, with no time
 * limit but the runner's. */
static void phase(pid_t l, const char *what, unsigned long long kept)
{
	struct stat sb;
	/* L's job settled first: a file's object that L closed is still charged
	 * while the reaper holds it, and the script reads `top` right after. */
	(void)memory_of(l);
	printf("t_shared: keep: %s (an image keeps exactly %llu bytes)\n", what, kept);
	fflush(stdout);
	while (stat("/work/t_shared.go", &sb))
		usleep(100000);
	unlink("/work/t_shared.go");
}

static int driver(int keep)
{
	int tin[2], tout[2];
	posix_spawn_file_actions_t fa;
	pid_t l;
	char *argv[] = {"t_shared", "launcher", NULL};
	char exe[PATH_MAX];
	ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
	unsigned long long kept;
	unsigned long long u[8] = {0};
	unsigned long long before, after_path, after_self[2], d_path, d_self[2];
	int st;

	if (n <= 0)
		FAIL("readlink /proc/self/exe");
	exe[n] = 0;
	kept = kept_exact(exe);
	if (kept < 64 * 1024)
		FAIL("an image of %llu bytes: too small to measure", kept);
	if (pipe(tin) || pipe(tout))
		FAIL("pipe");
	posix_spawn_file_actions_init(&fa);
	posix_spawn_file_actions_adddup2(&fa, tin[0], 0);
	posix_spawn_file_actions_adddup2(&fa, tout[1], 1);
	posix_spawn_file_actions_addclose(&fa, tin[0]);
	posix_spawn_file_actions_addclose(&fa, tin[1]);
	posix_spawn_file_actions_addclose(&fa, tout[0]);
	posix_spawn_file_actions_addclose(&fa, tout[1]);
	/* L is a spawn of this program's own image: no file is read for it. */
	if (posix_spawn(&l, "/proc/self/exe", &fa, NULL, argv, environ))
		FAIL("posix_spawn of /proc/self/exe: errno %d", errno);
	posix_spawn_file_actions_destroy(&fa);
	close(tin[0]);
	close(tout[1]);
	to_l = tin[1];
	from_l = tout[0];

	/* L reads a file once, so that its transfer buffer (1 MiB, kept) is in
	 * its job before anything is measured. */
	ask('r', u, 8);

	if (keep) {
		/* The script reads `top` in a room at each phase (plan part F6's
		 * "done when"): L alone; then three spawns of its own image; then one
		 * spawn of the file. */
		phase(l, "L alone", kept);
		ask('s', u, 8);
		ask('s', u, 8);
		ask('s', u, 8);
		phase(l, "three children of its own image", kept);
		ask('p', u, 8);
		phase(l, "and one child of the file", kept);
	} else {
		/* The first spawn of its own image, which also settles L's heap. */
		ask('s', u, 8);
		before = memory_of(l);

		/* A spawn of the file: L reads it and makes an image, which it pays
		 * for. Every spawn here also leaves a pipe of L's behind (what L pays
		 * for it is the same for each), so the spawns are compared with one
		 * another. */
		ask('p', u, 8);
		after_path = memory_of(l);
		/* Two more spawns of its own image. */
		ask('s', u, 8);
		after_self[0] = memory_of(l);
		ask('s', u, 8);
		after_self[1] = memory_of(l);

		d_path = after_path - before;
		d_self[0] = after_self[0] - after_path;
		d_self[1] = after_self[1] - after_self[0];
		if (after_path < before || after_self[0] < after_path || after_self[1] < after_self[0])
			FAIL("L's job shrank: %llu %llu %llu %llu", before, after_path,
			     after_self[0], after_self[1]);
		/* The file's spawn cost the image's pages, exactly, more than a spawn
		 * of the image (and a few pages for the record and the tables). */
		if (d_path < d_self[0] + kept || d_path > d_self[0] + kept + 16 * PAGE)
			FAIL("a spawn of the file cost L %llu bytes and one of its image %llu: the difference is not the image's %llu",
			     d_path, d_self[0], kept);
		/* And a spawn of the image costs the same each time. */
		if (d_self[0] + 2 * PAGE < d_self[1] || d_self[1] + 2 * PAGE < d_self[0])
			FAIL("two spawns of its image cost L %llu and %llu bytes", d_self[0], d_self[1]);
		printf("t_shared: image keeps %llu bytes; L's job: %llu, +%llu for a spawn of the file, +%llu +%llu for two of its image\n",
		       kept, before, d_path, d_self[0], d_self[1]);
	}

	/* Each child's own job holds its own pages: the three of its own image
	 * alike, and no more than the one that read the file. */
	ask('u', u, 8);
	printf("t_shared: children's own jobs: %llu %llu %llu %llu bytes (the one of the file is %s)\n",
	       u[0], u[1], u[2], u[3], keep ? "last" : "second");
	if (keep) {
		close(to_l);
		close(from_l);
		if (waitpid(l, &st, 0) != l || !WIFEXITED(st) || WEXITSTATUS(st))
			FAIL("the launcher ended with %d", st);
		printf("t_shared: passed\n");
		return 0;
	}
	for (int i = 2; i < 4; i++) {
		unsigned long long d = u[i] > u[0] ? u[i] - u[0] : u[0] - u[i];
		if (d > PAGE * 4)
			FAIL("children of one image differ by %llu bytes", d);
	}
	if (u[1] > u[0] + PAGE * 4)
		FAIL("the child of the file holds %llu more than one of the image", u[1] - u[0]);
	if (u[0] == 0)
		FAIL("a child's job holds nothing");

	close(to_l);
	close(from_l);
	if (waitpid(l, &st, 0) != l || !WIFEXITED(st) || WEXITSTATUS(st))
		FAIL("the launcher ended with %d", st);
	printf("t_shared: passed\n");
	return 0;
}

int main(int argc, char **argv)
{
	if (argc > 1 && !strcmp(argv[1], "hold"))
		return hold();
	if (argc > 1 && !strcmp(argv[1], "launcher"))
		return launcher();
	if (argc > 1 && !strcmp(argv[1], "keep"))
		return driver(1);
	if (argc > 1)
		FAIL("usage: t_shared [keep]");
	return driver(0);
}
