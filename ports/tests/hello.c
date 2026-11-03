/* The host check of ADR-0109 item 7: tcc for Zoraya, built for the host,
 * compiles and links this against the sysroot, and zoraya-elf-check accepts
 * the result. It touches what tcc's own code generation and libtcc1.a must
 * get right for musl: variadic calls, long double (IEEE quad on AArch64) and
 * wchar_t (unsigned on AAPCS64 and musl). */
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <wchar.h>

static int second(int n, ...)
{
	va_list ap;
	int v;

	va_start(ap, n);
	v = va_arg(ap, int);
	v = va_arg(ap, int);
	va_end(ap);
	return v;
}

int main(void)
{
	wchar_t w = L'z';
	long double third = 1.0L / 3;

	puts("hello from tcc");
	printf("%d %Lf %lu %u\n", second(2, 7, 42), third, (unsigned long)sizeof(long double), (unsigned)w);
	return 0;
}
