/* pthread_getattr_np, pthread_getname_np and pthread_setname_np (ADR-0308
 * item 2), replacing musl's, which read /proc and call prctl. The main
 * thread's stack is the one the kernel gave the process (zoraya-posix knows
 * its top and size); names live in the library's thread table. */
#define _GNU_SOURCE
#include "pthread_impl.h"
#include <string.h>

long zoraya_posix_thread_name(long tid, char *buf, long set);
void zoraya_posix_main_stack(size_t out[2]);

int pthread_getattr_np(pthread_t t, pthread_attr_t *a)
{
	*a = (pthread_attr_t){0};
	a->_a_detach = t->detach_state >= DT_DETACHED;
	a->_a_guardsize = t->guard_size;
	if (t->stack) {
		a->_a_stackaddr = (uintptr_t)t->stack;
		a->_a_stacksize = t->stack_size;
	} else {
		size_t m[2];
		zoraya_posix_main_stack(m);
		a->_a_stackaddr = m[0];
		a->_a_stacksize = m[1];
	}
	return 0;
}

int pthread_getname_np(pthread_t t, char *name, size_t len)
{
	char buf[16];
	if (len < 16) return ERANGE;
	long r = zoraya_posix_thread_name(t->tid, buf, 0);
	if (r < 0) return -r;
	memcpy(name, buf, 16);
	return 0;
}

int pthread_setname_np(pthread_t t, const char *name)
{
	if (strnlen(name, 16) > 15) return ERANGE;
	char buf[16] = {0};
	strncpy(buf, name, 15);
	long r = zoraya_posix_thread_name(t->tid, buf, 1);
	return r < 0 ? -r : 0;
}
