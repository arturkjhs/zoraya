/* What libc.a answers for Rust's std that needs musl's thread record
 * (ADR-0451 item 3). Compiled as musl's own, so it may include its internal
 * headers.
 *
 * gettid: the thread's id, as pthread_self's record has it (the main thread's
 * is the one set_tid_address returned, a new thread's the one its creator
 * gave it). syscall(SYS_gettid) in zoraya-posix calls this. */
#define _GNU_SOURCE
#include "pthread_impl.h"

int gettid(void)
{
	return __pthread_self()->tid;
}
