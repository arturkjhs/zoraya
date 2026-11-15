/* What nvi2 takes from libbsd on Linux (CMakeLists.txt: getprogname; strlcpy
 * and strlcat are musl's). Not a name of libc.a: it is part of the program. */
#include <stdlib.h>

/* The name nvi was run by, from argv[0] (patches/nvi2/0004): `ex` and `view` are links to `vi`
 * and the program takes its mode and its messages' prefix from this name. */
#include <string.h>
static char progname[32] = "vi";

void zoraya_setprogname(const char *argv0)
{
	const char *p;
	if (argv0 == NULL)
		return;
	p = strrchr(argv0, '/');
	p = p ? p + 1 : argv0;
	if (*p != '\0') {
		strncpy(progname, p, sizeof(progname) - 1);
		progname[sizeof(progname) - 1] = '\0';
	}
}

const char *getprogname(void)
{
	return progname;
}

/* The BSD error functions, which musl does not have (libbsd's on Linux). */
#include <stdarg.h>
#include <stdio.h>
#include <errno.h>
#include <string.h>

static void vwarnc(int code, const char *fmt, va_list ap)
{
	fprintf(stderr, "%s: ", getprogname());
	if (fmt) {
		vfprintf(stderr, fmt, ap);
		if (code >= 0)
			fputs(": ", stderr);
	}
	if (code >= 0)
		fputs(strerror(code), stderr);
	fputc('\n', stderr);
}

void warn(const char *fmt, ...)
{
	va_list ap;
	int e = errno;
	va_start(ap, fmt);
	vwarnc(e, fmt, ap);
	va_end(ap);
}

void warnx(const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	vwarnc(-1, fmt, ap);
	va_end(ap);
}

void err(int status, const char *fmt, ...)
{
	va_list ap;
	int e = errno;
	va_start(ap, fmt);
	vwarnc(e, fmt, ap);
	va_end(ap);
	exit(status);
}

void errx(int status, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	vwarnc(-1, fmt, ap);
	va_end(ap);
	exit(status);
}
