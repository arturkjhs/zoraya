/* gtrun NAME [FILTER]: runs GoogleTest's test program /bin/gt/NAME (TASK-30,
 * make test-cxx-gtest) as a POSIX program of the shell, so that it has what
 * the line gives every POSIX program (a /tmp of its own, fork through the
 * broker): it executes the program in its own place, with argv[0] its full
 * path, which the threadsafe death tests execute again. FILTER, if given, is
 * passed as --gtest_filter=FILTER (an operand, not an option of the line:
 * GoogleTest's options are not the shell's). */
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv)
{
	static char path[256];
	static char filter[1024];
	if (argc < 2 || argc > 3 || strchr(argv[1], '/') != NULL || strlen(argv[1]) > 200 ||
	    (argc == 3 && strlen(argv[2]) > 1000)) {
		fprintf(stderr, "usage: gtrun NAME [FILTER]\n");
		return 2;
	}
	snprintf(path, sizeof path, "/bin/gt/%s", argv[1]);
	char *args[3] = {path, NULL, NULL};
	if (argc == 3) {
		snprintf(filter, sizeof filter, "--gtest_filter=%s", argv[2]);
		args[1] = filter;
	}
	execv(path, args);
	perror(path);
	return 127;
}
