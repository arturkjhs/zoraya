/* gtrun NAME [ARG...]: runs GoogleTest's test program /bin/gt/NAME (TASK-30,
 * make test-cxx-gtest) as a POSIX program of the shell, so that it has what
 * the line gives every POSIX program (a /tmp of its own, fork through the
 * broker): it executes the program in its own place, with argv[0] its full
 * path, which the threadsafe death tests execute again. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv)
{
	static char path[256];
	if (argc < 2 || strchr(argv[1], '/') != NULL || strlen(argv[1]) > 200) {
		fprintf(stderr, "usage: gtrun NAME [ARG...]\n");
		return 2;
	}
	snprintf(path, sizeof path, "/bin/gt/%s", argv[1]);
	argv[1] = path;
	execv(path, argv + 1);
	perror(path);
	return 127;
}
