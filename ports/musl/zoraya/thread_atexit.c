/* __cxa_thread_atexit_impl (TASK-30, proposed ADR-0366): the destructors of a
 * thread's thread_local objects, which libc++abi registers here (it is built
 * with HAVE___CXA_THREAD_ATEXIT_IMPL), run when the thread ends, in reverse
 * order of registration, before the thread's pthread key destructors, as
 * glibc runs them: libc++'s notify_all_at_thread_exit and
 * set_value_at_thread_exit are a key destructor, and C++ requires them after
 * every thread_local is destroyed ([thread.condition], [futures.promise]).
 * libc++abi's own fallback keeps the list in a key of its own, whose
 * destructor musl runs in key order, after libc++'s: the notification came
 * first (tests/cxx/cxxtest.cpp, mode exit-order). __pthread_exit (patch 0003)
 * and exit (for the thread that calls it) run __zoraya_thread_dtors first. */
#include <errno.h>
#include <stdlib.h>

struct thread_dtor {
	void (*f)(void *);
	void *arg;
	struct thread_dtor *next;
};

static _Thread_local struct thread_dtor *thread_dtors;

int __cxa_thread_atexit_impl(void (*f)(void *), void *arg, void *dso)
{
	(void)dso; /* one module: there is no dynamic loader (ADR-0100) */
	struct thread_dtor *d = malloc(sizeof *d);
	if (!d) {
		errno = ENOMEM;
		return -1;
	}
	d->f = f;
	d->arg = arg;
	d->next = thread_dtors;
	thread_dtors = d;
	return 0;
}

/* A destructor may construct another thread_local, whose destructor is then
 * run too: the list is taken from its head until it is empty. */
void __zoraya_thread_dtors(void)
{
	struct thread_dtor *d;
	while ((d = thread_dtors)) {
		thread_dtors = d->next;
		d->f(d->arg);
		free(d);
	}
}
