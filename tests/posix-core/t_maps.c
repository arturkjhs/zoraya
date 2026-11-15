/*
 * 4096 mappings per process (ADR-0415 item 1, INV-0415-1, plan part F5): a
 * static C program built by zcc against libc.a, run by posixrun in a 2 GiB
 * guest (tests/posix-core/maps.script). One binary with named cases; each
 * prints "t_maps: NAME passed" or a line ending in FAIL and exits 1.
 *
 *   t_maps limit   3,000 one-page mmaps and 900 MiB of small mallocs (each
 *                  1 MiB of heap is a mapping) all held at once and still
 *                  intact; the mmap that finds the kernel's 4096 mappings full
 *                  fails with ENOMEM, changes nothing, and a munmap makes room
 *                  again; munmap of what mmap did not make (the heap, half a
 *                  mapping, an unmapped address) is EINVAL
 *   t_maps arenas  400 mmaps of 1 MiB, the arenas of CPython's allocator,
 *                  each written whole, and 5,000,000 small allocations of
 *                  100 bytes on top: what
 *                  python3 -c "x=[bytearray(100) for _ in range(5_000_000)]"
 *                  asks of the process (it ended at 256 mappings before)
 *   t_maps all     every case, in one process (the heap keeps what `limit`
 *                  grew it by, so the script runs them one at a time)
 *
 * The CPython command itself is the python step's (tests/python/).
 */
#define _GNU_SOURCE
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_maps: line %d: %s (errno %d) FAIL\n",         \
			       __LINE__, #cond, errno);                         \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define PAGE 4096UL
#define MIB (1024UL * 1024UL)

static void *map_pages(size_t pages)
{
	return mmap(NULL, pages * PAGE, PROT_READ | PROT_WRITE,
		    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
}

/* Fills a mapping with a value that tells it from every other. */
static void stamp(unsigned char *p, size_t n, unsigned long tag)
{
	for (size_t i = 0; i < n; i += PAGE)
		memcpy(p + i, &tag, sizeof tag), p[i + PAGE - 1] = (unsigned char)tag;
}

static int stamped(const unsigned char *p, size_t n, unsigned long tag)
{
	for (size_t i = 0; i < n; i += PAGE) {
		unsigned long got;
		memcpy(&got, p + i, sizeof got);
		if (got != tag || p[i + PAGE - 1] != (unsigned char)tag)
			return 0;
	}
	return 1;
}

static void limit_case(void)
{
	enum { PAGES = 3000, BLOCK = 64 * 1024, BLOCKS = (900 * 1024) / 64 };
	static void *page[PAGES];
	static void *block[BLOCKS];

	for (int i = 0; i < PAGES; i++) {
		page[i] = map_pages(1);
		CHECK(page[i] != MAP_FAILED);
		stamp(page[i], PAGE, i + 1);
	}
	/* 900 MiB of small blocks: the heap grows by 1 MiB steps, 900 of them,
	 * and each step is a mapping of the kernel's. */
	for (int i = 0; i < BLOCKS; i++) {
		block[i] = malloc(BLOCK);
		CHECK(block[i] != NULL);
		memset(block[i], i & 0xff, BLOCK);
	}
	/* All held at once, and none disturbed by the others. */
	for (int i = 0; i < PAGES; i++)
		CHECK(stamped(page[i], PAGE, i + 1));
	for (int i = 0; i < BLOCKS; i++)
		CHECK(((unsigned char *)block[i])[0] == (i & 0xff) &&
		      ((unsigned char *)block[i])[BLOCK - 1] == (i & 0xff));

	/* The rest of the 4096: map until the kernel says no. */
	static void *more[200];
	int n = 0;
	for (;; n++) {
		CHECK(n < 200);
		more[n] = map_pages(1);
		if (more[n] == MAP_FAILED)
			break;
	}
	CHECK(errno == ENOMEM);
	printf("t_maps: %d + %d more mappings before the limit\n", PAGES + 900, n);
	/* At the limit and nothing changed: the same refusal again, and the
	 * ones held are intact. */
	CHECK(map_pages(1) == MAP_FAILED && errno == ENOMEM);
	for (int i = 0; i < PAGES; i++)
		CHECK(stamped(page[i], PAGE, i + 1));
	/* One munmap makes room for exactly one. */
	CHECK(munmap(more[n - 1], PAGE) == 0);
	void *again = map_pages(1);
	CHECK(again != MAP_FAILED);
	CHECK(map_pages(1) == MAP_FAILED && errno == ENOMEM);
	CHECK(munmap(again, PAGE) == 0);

	/* What mmap did not make is not munmap's to end. */
	errno = 0;
	CHECK(munmap(block[0], BLOCK) == -1 && errno == EINVAL);
	errno = 0;
	CHECK(munmap((char *)page[5] + PAGE, PAGE) == -1 && errno == EINVAL);
	errno = 0;
	CHECK(munmap((void *)(8 * PAGE), PAGE) == -1 && errno == EINVAL);
	CHECK(stamped(page[5], PAGE, 6));

	for (int i = 0; i < n - 1; i++)
		CHECK(munmap(more[i], PAGE) == 0);
	for (int i = 0; i < BLOCKS; i++)
		free(block[i]);
	for (int i = 0; i < PAGES; i++)
		CHECK(munmap(page[i], PAGE) == 0);
	/* The slots are free again: the same number can be made again. */
	for (int i = 0; i < PAGES; i++) {
		page[i] = map_pages(1);
		CHECK(page[i] != MAP_FAILED);
	}
	for (int i = 0; i < PAGES; i++)
		CHECK(munmap(page[i], PAGE) == 0);
	printf("t_maps: limit passed\n");
}

static void arenas_case(void)
{
	enum { ARENAS = 400, OBJECTS = 5000000 };
	static void *arena[ARENAS];
	for (int i = 0; i < ARENAS; i++) {
		arena[i] = map_pages(MIB / PAGE);
		CHECK(arena[i] != MAP_FAILED);
		stamp(arena[i], MIB, i + 1);
	}
	/* The list of 5,000,000 pointers is one large block; each object is a
	 * small one: 100 bytes and the library's header. */
	unsigned char **list = malloc(OBJECTS * sizeof *list);
	CHECK(list != NULL);
	for (long i = 0; i < OBJECTS; i++) {
		list[i] = malloc(100);
		CHECK(list[i] != NULL);
		list[i][0] = (unsigned char)i;
		list[i][99] = (unsigned char)(i >> 8);
	}
	for (long i = 0; i < OBJECTS; i += 997)
		CHECK(list[i][0] == (unsigned char)i && list[i][99] == (unsigned char)(i >> 8));
	for (int i = 0; i < ARENAS; i++)
		CHECK(stamped(arena[i], MIB, i + 1));
	for (long i = 0; i < OBJECTS; i++)
		free(list[i]);
	free(list);
	for (int i = 0; i < ARENAS; i++)
		CHECK(munmap(arena[i], MIB) == 0);
	printf("t_maps: arenas passed\n");
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "all";
	int all = strcmp(mode, "all") == 0, known = 0;
#define CASE(name, fn)                                                     \
	if (all || strcmp(mode, name) == 0) {                              \
		known = 1;                                                 \
		fn();                                                      \
	}
	CASE("limit", limit_case)
	CASE("arenas", arenas_case)
	if (!known) {
		printf("t_maps: unknown case %s FAIL\n", mode);
		return 2;
	}
	return 0;
}
