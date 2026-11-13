/* Deferred cancellation (ADR-0305, ADR-0314 item 4 as refined by ADR-0315):
 * replaces musl's src/thread/pthread_cancel.c and syscall_cp.c, which need
 * signals and an assembly window. pthread_cancel sets the flag and interrupts
 * the target (thread_interrupt, through zoraya_posix_interrupt); the flag is
 * acted on at a cancellation point of the target: __zoraya_cp_before/after
 * around every `__syscall_cp` (the futex waits of musl's code), and
 * __testcancel in pthread_testcancel, pthread_join and the library's waits
 * once they wake (ffi.rs `wait_unlocked`). Asynchronous cancellation is the
 * deferred kind: there is no signal to interrupt a running thread. */
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

/* A cancellation point is entered (ADR-0305): 0 to go on; a pending, enabled
 * cancel ends the thread (or, in masked mode, is reported as -ECANCELED
 * without making the call). */
long __zoraya_cp_before(void)
{
	pthread_t self = __pthread_self();
	if (self->canceldisable == PTHREAD_CANCEL_DISABLE)
		return 0;
	if (self->cancel)
		return __cancel();
	return 0;
}

/* The call returned `r`. A cancel that arrived while it waited (the futex
 * shim returns 0 or -EINTR after `thread_interrupt`, ADR-0304 item 11) is
 * acted on now. */
long __zoraya_cp_after(long r)
{
	pthread_t self = __pthread_self();
	if (self->canceldisable == PTHREAD_CANCEL_DISABLE || !self->cancel)
		return r;
	if (r == -EINTR || r == 0)
		return __cancel();
	return r;
}

long zoraya_posix_interrupt(long tid);

int pthread_cancel(pthread_t t)
{
	a_store(&t->cancel, 1);
	if (t == pthread_self()) {
		if (t->canceldisable == PTHREAD_CANCEL_ENABLE && t->cancelasync)
			pthread_exit(PTHREAD_CANCELED);
		return 0;
	}
	/* Wake the target out of a futex wait, a sleep or a library wait; it
	 * sees the flag at the cancellation point it is in. A thread that has
	 * ended is not an error (it was cancelled too late). */
	zoraya_posix_interrupt(t->tid);
	return 0;
}
