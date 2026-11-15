/* What nvi2 takes from libbsd on Linux (CMakeLists.txt: getprogname; strlcpy
 * and strlcat are musl's). Not a name of libc.a: it is part of the program. */
#include <stdlib.h>

const char *getprogname(void)
{
	return "vi";
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
