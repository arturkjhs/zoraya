/* Cancellation, first slice (ADR-0305, ADR-0314 item 4): replaces musl's
 * src/thread/pthread_cancel.c and syscall_cp.c, which need signals and an
 * assembly window. Only the flag is kept: pthread_cancel marks the target; the
 * mark is acted on by __testcancel at a cancellation point of the calling
 * thread (pthread_testcancel, pthread_cond_wait, pthread_join, ...). A thread
 * blocked in a futex wait is not woken by it yet: that is thread_interrupt in
 * step 3 of part 9b. */
#define _GNU_SOURCE
#include "pthread_impl.h"
#include "syscall.h"

long __cancel(void)
{
	pthread_t self = __pthread_self();
	if (self->canceldisable == PTHREAD_CANCEL_ENABLE || self->cancelasync)
		pthread_exit(PTHREAD_CANCELED);
	self->canceldisable = PTHREAD_CANCEL_DISABLE;
	return -ECANCELED;
}

void __testcancel(void)
{
	pthread_t self = __pthread_self();
	if (self->cancel && !self->canceldisable)
		__cancel();
}

int pthread_cancel(pthread_t t)
{
	a_store(&t->cancel, 1);
	if (t == pthread_self()) {
		if (t->canceldisable == PTHREAD_CANCEL_ENABLE && t->cancelasync)
			pthread_exit(PTHREAD_CANCELED);
	}
	return 0;
}
