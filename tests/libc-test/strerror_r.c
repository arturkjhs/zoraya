/* threadtest3.c calls strerror_r, which libc.a does not export (it is not on
 * the tier list); the test links this one instead (XSI form). */
#include <errno.h>
#include <string.h>
#include <unistd.h>

int strerror_r(int e, char *buf, size_t n)
{
	const char *s = strerror(e);
	if (strlen(s) >= n) {
		if (n)
			buf[0] = 0;
		return ERANGE;
	}
	strcpy(buf, s);
	return 0;
}

/* posixrun starts in the root directory; the scenarios make test.db in the
 * current one. */
__attribute__((constructor)) static void work_directory(void)
{
	if (chdir("/work") != 0)
		_exit(251);
}
