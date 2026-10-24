/* W8 (TASK-02b): table generator run by every leaf target. Deterministic output; the work per row is a fixed
 * number of LCG steps, sized so that one table takes a few tenths of a second under tcc. Each row is flushed,
 * so an interrupted `./mktab N > tN.h` leaves a partly written target behind (what make must delete). */
#include <stdio.h>
#include <stdlib.h>

#define ROWS 40
#define STEPS 8000000L

int main(int argc, char **argv)
{
	int id = argc > 1 ? atoi(argv[1]) : 0;
	unsigned long h = (unsigned long)id * 2654435761UL + 1;
	printf("static const unsigned long tab%d[] = {\n", id);
	for (int row = 0; row < ROWS; row++) {
		for (long i = 0; i < STEPS; i++)
			h = (h * 6364136223846793005UL + 1442695040888963407UL) ^ (h >> 29);
		printf("\t%luUL,\n", h);
		fflush(stdout);
	}
	printf("\t0 };\n");
	return 0;
}
