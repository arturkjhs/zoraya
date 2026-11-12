/* The few libc declarations ring's C files use, for a freestanding build
   (aarch64-unknown-none, ADR-0228): clang supplies stdint.h, stddef.h,
   stdbool.h and stdalign.h itself. The functions come from compiler-builtins. */
#ifndef ZORAYA_RING_STRING_H
#define ZORAYA_RING_STRING_H
#include <stddef.h>
void *memcpy(void *dest, const void *src, size_t n);
void *memmove(void *dest, const void *src, size_t n);
void *memset(void *dest, int c, size_t n);
int memcmp(const void *a, const void *b, size_t n);
size_t strlen(const char *s);
#endif
