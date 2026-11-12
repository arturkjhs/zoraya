/* See string.h. ring is built with NDEBUG, so assert is a no-op. */
#ifndef ZORAYA_RING_ASSERT_H
#define ZORAYA_RING_ASSERT_H
#define assert(condition) ((void)0)
#endif
