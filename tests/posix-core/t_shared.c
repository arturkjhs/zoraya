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

/* A lower bound of what an image of this program keeps (proposed ADR-0438):
 * the pages of the segments that are not writable and those of the writable
 * ones that hold file bytes, which together are most of what lies between the
 * start of the program and the end of its initialised data; the other half
 * is the room for the gaps that a segment's alignment leaves. */
extern char __executable_start, edata;

static unsigned long long kept_at_least(void)
{
	return (unsigned long long)(&edata - &__executable_start) / 2;
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

static unsigned long long memory_of(pid_t pid)
{
	unsigned long long used = 0;
	if (zoraya_child_memory(pid, &used))
		FAIL("zoraya_child_memory(%d): errno %d", (int)pid, errno);
	return used;
}

static int driver(void)
{
	int tin[2], tout[2];
	posix_spawn_file_actions_t fa;
	pid_t l;
	char *argv[] = {"t_shared", "launcher", NULL};
	unsigned long long kept = kept_at_least();
	unsigned long long u[8] = {0};
	unsigned long long before, after_path, after_self[2], d_path, d_self[2];
	int st;

	if (kept < 16 * 1024)
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

	/* The first spawn of its own image, which also settles L's heap. */
	ask('s', u, 8);
	before = memory_of(l);

	/* A spawn of the file: L reads it and makes an image, which it pays for.
	 * Every spawn here also leaves a pipe of L's behind (what L pays for it
	 * is the same for each), so the spawns are compared with one another. */
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
		FAIL("L's job shrank: %llu %llu %llu %llu", before, after_path, after_self[0],
		     after_self[1]);
	/* The file's spawn cost the image's pages more than a spawn of the image. */
	if (d_path < d_self[0] + kept)
		FAIL("a spawn of the file cost L %llu bytes, one of its image %llu, an image keeps at least %llu",
		     d_path, d_self[0], kept);
	/* And a spawn of the image costs the same each time: no image of its own. */
	if (d_self[0] + 2 * PAGE < d_self[1] || d_self[1] + 2 * PAGE < d_self[0])
		FAIL("two spawns of its image cost L %llu and %llu bytes", d_self[0], d_self[1]);

	/* Each child's own job holds its own pages: the three of its own image
	 * alike, and no more than the one that read the file. */
	ask('u', u, 8);
	printf("t_shared: image keeps at least %llu bytes; L's job: %llu, +%llu for a spawn of the file, +%llu +%llu for two of its image\n",
	       kept, before, d_path, d_self[0], d_self[1]);
	printf("t_shared: children (own image, file, own image, own image): %llu %llu %llu %llu bytes\n",
	       u[0], u[1], u[2], u[3]);
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
	if (argc > 1)
		FAIL("usage: t_shared");
	return driver();
}
