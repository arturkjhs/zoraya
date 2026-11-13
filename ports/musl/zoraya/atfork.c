/* The libc side of fork (ADR-0306 item 1 and 5): musl's src/process/fork.c
 * and _Fork.c are not compiled (the fork wrapper is zoraya-posix's, ADR-0102);
 * this file does what they do around the snapshot and nothing else:
 *
 *   __zoraya_fork_prepare  the atfork prepare handlers, the signal mask, musl's
 *                          locks and the thread-list lock, in musl's order;
 *   __zoraya_fork_parent   the same in reverse, then the parent handlers
 *                          (also after a failed fork);
 *   __zoraya_fork_child    the child's reduction: one thread in the list,
 *                          every lock free, stdio locks of the dead threads
 *                          released, the dead threads' stacks unmapped, then
 *                          the child handlers.
 *
 * The library lock and the memory lock are zoraya-posix's own and are taken
 * by the wrapper after prepare (order: musl's locks, the thread-list lock,
 * the library lock, the memory lock; ADR-0306 item 1). */
#define _GNU_SOURCE
#include <unistd.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <sys/mman.h>
#include "libc.h"
#include "lock.h"
#include "pthread_impl.h"
#include "stdio_impl.h"
#include "fork_impl.h"

static volatile int *const dummy_lockptr = 0;

weak_alias(dummy_lockptr, __at_quick_exit_lockptr);
weak_alias(dummy_lockptr, __atexit_lockptr);
weak_alias(dummy_lockptr, __gettext_lockptr);
weak_alias(dummy_lockptr, __locale_lockptr);
weak_alias(dummy_lockptr, __random_lockptr);
weak_alias(dummy_lockptr, __sem_open_lockptr);
weak_alias(dummy_lockptr, __stdio_ofl_lockptr);
weak_alias(dummy_lockptr, __syslog_lockptr);
weak_alias(dummy_lockptr, __timezone_lockptr);
weak_alias(dummy_lockptr, __bump_lockptr);
weak_alias(dummy_lockptr, __vmlock_lockptr);

static volatile int *const *const atfork_locks[] = {
	&__at_quick_exit_lockptr,
	&__atexit_lockptr,
	&__gettext_lockptr,
	&__locale_lockptr,
	&__random_lockptr,
	&__sem_open_lockptr,
	&__stdio_ofl_lockptr,
	&__syslog_lockptr,
	&__timezone_lockptr,
	&__bump_lockptr,
};

static void dummy(int x) { }
weak_alias(dummy, __fork_handler);
weak_alias(dummy, __aio_atfork);
weak_alias(dummy, __pthread_key_atfork);
weak_alias(dummy, __ldso_atfork);

static void dummy_0(void) { }
weak_alias(dummy_0, __tl_lock);
weak_alias(dummy_0, __tl_unlock);

/* What prepare keeps for parent and child. The wrapper gives room for it
 * (32 words). */
struct zoraya_fork {
	sigset_t set;
	int need_locks;
	pthread_t self, next;
};
_Static_assert(sizeof(struct zoraya_fork) <= 32 * sizeof(long), "zoraya_fork too big");

void __zoraya_fork_prepare(struct zoraya_fork *c)
{
	__fork_handler(-1);
	__block_app_sigs(&c->set);
	c->need_locks = libc.need_locks > 0;
	if (c->need_locks) {
		__ldso_atfork(-1);
		__pthread_key_atfork(-1);
		__aio_atfork(-1);
		__inhibit_ptc();
		for (int i = 0; i < sizeof atfork_locks / sizeof *atfork_locks; i++)
			if (*atfork_locks[i]) LOCK(*atfork_locks[i]);
		__tl_lock();
	}
	c->self = __pthread_self();
	c->next = c->self->next;
}

/* ret: 1 in the parent after a fork, 0 after a failed one (errno is kept). */
void __zoraya_fork_parent(struct zoraya_fork *c)
{
	int errno_save = errno;
	if (c->need_locks) {
		__tl_unlock();
		for (int i = 0; i < sizeof atfork_locks / sizeof *atfork_locks; i++)
			if (*atfork_locks[i]) UNLOCK(*atfork_locks[i]);
		__release_ptc();
		__aio_atfork(0);
		__pthread_key_atfork(0);
		__ldso_atfork(0);
	}
	__restore_sigs(&c->set);
	__fork_handler(0);
	errno = errno_save;
}

/* A FILE locked by a thread that is not here is free in the child (a
 * sibling in the middle of printf when the fork came). A negative lock is
 * "never locked" (musl's single-threaded form) and stays. The buffer of a
 * FILE whose lock is freed this way can hold a half-written record of the dead
 * thread (INV-0306-1, ADR-0315 item 9): the lock is free, the data is as the
 * sibling left it. */
static void free_stdio_locks(pthread_t self)
{
	FILE *fixed[3] = { stdin, stdout, stderr };
	for (int i = 0; i < 3; i++)
		if (fixed[i]->lock >= 0 && (fixed[i]->lock & ~MAYBE_WAITERS) != self->tid)
			fixed[i]->lock = 0;
	for (FILE *f = *__ofl_lock(); f; f = f->next)
		if (f->lock >= 0 && (f->lock & ~MAYBE_WAITERS) != self->tid)
			f->lock = 0;
	__ofl_unlock();
}

void __zoraya_fork_child(struct zoraya_fork *c)
{
	pthread_t self = c->self;
	/* musl's __post_Fork. */
	self->robust_list.off = 0;
	self->robust_list.pending = 0;
	self->next = self->prev = self;
	__thread_list_lock = 0;
	libc.threads_minus_1 = 0;
	if (libc.need_locks) libc.need_locks = -1;
	if (c->need_locks) {
		__release_ptc();
		/* musl's fork.c: the memory-mapping lock of vmlock.c. */
		if (__vmlock_lockptr) {
			__vmlock_lockptr[0] = 0;
			__vmlock_lockptr[1] = 0;
		}
		for (int i = 0; i < sizeof atfork_locks / sizeof *atfork_locks; i++)
			if (*atfork_locks[i]) **atfork_locks[i] = 0;
		__aio_atfork(1);
		__pthread_key_atfork(1);
		__ldso_atfork(1);
	}
	free_stdio_locks(self);
	/* The other threads' stacks and thread blocks (ADR-0306 item 5): they
	 * can never run. The next pointer is read before its block goes. */
	for (pthread_t td = c->next; td != self; ) {
		pthread_t nx = td->next;
		if (td->map_base) munmap(td->map_base, td->map_size);
		td = nx;
	}
	__restore_sigs(&c->set);
	__fork_handler(1);
}
