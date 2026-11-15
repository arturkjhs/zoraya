/* <bitstring.h> for nvi2 (TASK-31 ports-early). nvi2 includes it from every vi
 * source and expects the system to have it (4.4BSD's, and libbsd's on Linux);
 * musl has none. This is a small implementation of the same interface, written
 * for this repository, under the repository's licence: bit_alloc, bit_decl,
 * bit_test, bit_set, bit_clear, bit_nclear, bit_nset, bit_ffc, bit_ffs. */
#ifndef ZORAYA_BITSTRING_H
#define ZORAYA_BITSTRING_H

#include <stdlib.h>

typedef unsigned char bitstr_t;

#define _bit_byte(bit)	((bit) >> 3)
#define _bit_mask(bit)	(1 << ((bit) & 0x7))

/* The number of bytes for nbits bits. */
#define bitstr_size(nbits)	(((nbits) + 7) >> 3)

#define bit_alloc(nbits)	((bitstr_t *)calloc(bitstr_size(nbits), sizeof(bitstr_t)))
#define bit_decl(name, nbits)	((name)[bitstr_size(nbits)])

#define bit_test(name, bit)	((name)[_bit_byte(bit)] & _bit_mask(bit))
#define bit_set(name, bit)	((name)[_bit_byte(bit)] |= _bit_mask(bit))
#define bit_clear(name, bit)	((name)[_bit_byte(bit)] &= ~_bit_mask(bit))

/* Clears bits start through stop, inclusive. */
#define bit_nclear(name, start, stop) do { \
	int _bn_i; \
	for (_bn_i = (start); _bn_i <= (stop); ++_bn_i) \
		bit_clear(name, _bn_i); \
} while (0)

/* Sets bits start through stop, inclusive. */
#define bit_nset(name, start, stop) do { \
	int _bn_i; \
	for (_bn_i = (start); _bn_i <= (stop); ++_bn_i) \
		bit_set(name, _bn_i); \
} while (0)

/* The first clear bit below nbits, or -1. */
#define bit_ffc(name, nbits, value) do { \
	int _bf_i, _bf_v = -1; \
	for (_bf_i = 0; _bf_i < (nbits); ++_bf_i) \
		if (!bit_test(name, _bf_i)) { _bf_v = _bf_i; break; } \
	*(value) = _bf_v; \
} while (0)

/* The first set bit below nbits, or -1. */
#define bit_ffs(name, nbits, value) do { \
	int _bf_i, _bf_v = -1; \
	for (_bf_i = 0; _bf_i < (nbits); ++_bf_i) \
		if (bit_test(name, _bf_i)) { _bf_v = _bf_i; break; } \
	*(value) = _bf_v; \
} while (0)

#endif
