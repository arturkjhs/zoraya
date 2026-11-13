/* Names that NetSurf's sources reference and Zoraya's libc.a does not export
 * (TASK-27 9d, proposed ADR-0318). Linked into `netsurf` from libnsshim.a, not
 * into libc.a (INV-0005-1), as libzshim.a is for curl and wget.
 *
 * atol, strncasecmp and modf are what musl has, written out here; iconv is
 * musl's own (libc.a keeps it under the hidden `__zoraya_` prefix, which the
 * surface check leaves unexported), forwarded; scandir is built on
 * opendir/readdir; dirfd fails, because the DIR of libposix does not expose
 * its descriptor (NetSurf's only use is an optimisation it can do without).
 */
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <iconv.h>
#include <stdlib.h>
#include <string.h>

long atol(const char *s)
{
	return strtol(s, 0, 10);
}

int strncasecmp(const char *a, const char *b, size_t n)
{
	for (; n; n--, a++, b++) {
		int x = tolower((unsigned char)*a), y = tolower((unsigned char)*b);
		if (x != y)
			return x - y;
		if (!x)
			return 0;
	}
	return 0;
}

double modf(double x, double *ip)
{
	/* |x| >= 2^52 has no fraction; NaN and infinity keep their value. */
	union { double d; unsigned long long u; } v = { x };
	int e = (int)((v.u >> 52) & 0x7ff) - 0x3ff;
	if (e >= 52) {
		*ip = x;
		if (e == 0x400 && (v.u << 12))
			return x; /* NaN */
		v.u &= 1ULL << 63;
		return v.d; /* signed zero */
	}
	if (e < 0) {
		v.u &= 1ULL << 63;
		*ip = v.d;
		return x;
	}
	{
		union { double d; unsigned long long u; } t = { x };
		t.u &= ~((1ULL << (52 - e)) - 1);
		*ip = t.d;
		return x - t.d;
	}
}

extern iconv_t __zoraya_iconv_open(const char *to, const char *from);
extern size_t __zoraya_iconv(iconv_t cd, char **in, size_t *inleft, char **out, size_t *outleft);
extern int __zoraya_iconv_close(iconv_t cd);

iconv_t iconv_open(const char *to, const char *from)
{
	return __zoraya_iconv_open(to, from);
}

size_t iconv(iconv_t cd, char **in, size_t *inleft, char **out, size_t *outleft)
{
	return __zoraya_iconv(cd, in, inleft, out, outleft);
}

int iconv_close(iconv_t cd)
{
	return __zoraya_iconv_close(cd);
}

int dirfd(DIR *d)
{
	(void)d;
	errno = ENOTSUP;
	return -1;
}

int scandir(const char *path, struct dirent ***res, int (*sel)(const struct dirent *),
	    int (*cmp)(const struct dirent **, const struct dirent **))
{
	DIR *d = opendir(path);
	struct dirent *de, **v = 0, **w;
	size_t cnt = 0, len = 0;
	int saved;

	if (!d)
		return -1;
	errno = 0;
	while ((de = readdir(d))) {
		struct dirent *c;
		if (sel && !sel(de))
			continue;
		if (cnt >= len) {
			len = 2 * len + 8;
			w = realloc(v, len * sizeof *v);
			if (!w)
				break;
			v = w;
		}
		c = malloc(de->d_reclen);
		if (!c)
			break;
		memcpy(c, de, de->d_reclen);
		v[cnt++] = c;
	}
	saved = errno;
	closedir(d);
	if (saved) {
		while (cnt)
			free(v[--cnt]);
		free(v);
		errno = saved;
		return -1;
	}
	if (cmp && cnt)
		qsort(v, cnt, sizeof *v, (int (*)(const void *, const void *))cmp);
	*res = v;
	return (int)cnt;
}
