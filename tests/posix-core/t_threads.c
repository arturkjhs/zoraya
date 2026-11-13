/*
 * Part 9b's threads through libposix (ADR-0302, ADR-0303, ADR-0309 item 3):
 * a static C program, built with zcc against libc.a, run by posixrun. One
 * binary with named cases; each prints "t_threads: NAME passed" or a line ending in FAIL and exits 1. Cases that need fork, exec or posix_spawn
 * (posixrun has no broker) are in tests/posix/t_posix.c (tfork, texec, ...).
 *
 *   t_threads attr      pthread_getattr_np (main and other thread), thread names
 *   t_threads create    create, join, results, pthread_self/equal, detach
 *   t_threads mutex     normal, recursive, errorcheck mutexes; trylock; timedlock
 *   t_threads cond      a producer and consumer ring, broadcast, timed waits
 *   t_threads once      pthread_once raced by many threads
 *   t_threads tls       errno and a _Thread_local array per thread; keys and destructors
 *   t_threads cycles    create and join 500 threads, 300 detached ones, stack reuse
 *   t_threads locks     rwlock, barrier, spin lock, semaphore
 *   t_threads stdio     printf and a pipe from several threads
 *   t_threads limit     create threads until EAGAIN, release them, create again
 *   t_threads exit      exit() from a non-main thread while a sibling spins (exit 7)
 *   t_threads mainexit  pthread_exit in the main thread; the process ends with the last thread
 *   t_threads waits     read, nanosleep, pselect in threads; the others use the library (INV-0302-2)
 *   t_threads signals   pthread_kill wakes a thread blocked in read, nanosleep, sem_wait; masks are per thread
 *   t_threads selfkill  pthread_kill/raise of oneself run the handler after musl's lock is released (handlers that
 *                       pthread_kill oneself again or pthread_exit do not deadlock)
 *   t_threads cancel    pthread_cancel of a thread blocked in each wait; cleanup handlers; PTHREAD_CANCELED;
 *                       a cancel taken by a mutex wait is still acted on at the next read; a cancel of a thread
 *                       that has ended; a disabled cancel waits for the enable
 *   t_threads blockedexit exit() flushes stdio while a sibling is blocked in read (exit 4)
 *   t_threads blocked   _Exit from the main thread while a sibling is blocked in read (exit 3)
 *   t_threads overflow  a thread writes far below its stack: the process faults (INV-0302-6)
 *   t_threads all       every case but the last three, which end the process
 */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/select.h>
#include <sys/stat.h>

#define CHECK(cond)                                                             \
	do {                                                                    \
		if (!(cond)) {                                                  \
			printf("t_threads: line %d: %s (errno %d) FAIL\n",      \
			       __LINE__, #cond, errno);                         \
			fflush(stdout);                                         \
			exit(1);                                                \
		}                                                               \
	} while (0)

#define OK(call) CHECK((call) == 0)

static double now(void)
{
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec + t.tv_nsec / 1e9;
}

/* Real time for a thread that announced it is about to block to be blocked:
 * the kernel-visible state of a thread is not exposed to a program, so the
 * tests announce and then sleep. The sleep is long against the few
 * microseconds the call needs to block, short against the case timeout. A
 * thread that is not yet blocked when the next step comes still gives the
 * right result (the interrupt and the cancel flag are sticky); the sleep only
 * makes the interesting order the likely one, and a slow host (TCG) makes the
 * wait longer, not shorter. */
static void settle(void)
{
	struct timespec ts = {0, 150 * 1000 * 1000};
	nanosleep(&ts, NULL);
}

/* Waits (bounded, 20 s) until *flag is nonzero. */
static void wait_flag(volatile int *flag)
{
	for (int i = 0; i < 2000 && !*flag; i++) {
		struct timespec ts = {0, 10 * 1000 * 1000};
		nanosleep(&ts, NULL);
	}
	CHECK(*flag);
}

/* Waits (bounded, 20 s) until *v reaches n. */
static void wait_count(volatile int *v, int n)
{
	for (int i = 0; i < 2000 && *v < n; i++) {
		struct timespec ts = {0, 10 * 1000 * 1000};
		nanosleep(&ts, NULL);
	}
	CHECK(*v >= n);
}

/* ------------------------------------------------------------ create */

static void *ret_arg(void *a) { return (char *)a + 1; }

static pthread_t main_thread;
static volatile int equal_seen;

static void *check_self(void *a)
{
	(void)a;
	equal_seen = !pthread_equal(pthread_self(), main_thread);
	return NULL;
}

static volatile int detached_ran;

static void *detached_fn(void *a)
{
	(void)a;
	detached_ran = 1;
	return NULL;
}

static void *exits_early(void *a)
{
	pthread_exit((char *)a + 7);
	return NULL;
}

static void create(void)
{
	pthread_t t;
	void *r;
	main_thread = pthread_self();
	OK(pthread_create(&t, NULL, ret_arg, (void *)100));
	OK(pthread_join(t, &r));
	CHECK(r == (void *)101);
	OK(pthread_create(&t, NULL, check_self, NULL));
	OK(pthread_join(t, NULL));
	CHECK(equal_seen == 1 && pthread_equal(main_thread, pthread_self()));
	OK(pthread_create(&t, NULL, exits_early, (void *)5));
	OK(pthread_join(t, &r));
	CHECK(r == (void *)12);

	pthread_attr_t at;
	OK(pthread_attr_init(&at));
	OK(pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED));
	OK(pthread_create(&t, &at, detached_fn, NULL));
	for (int i = 0; i < 1000 && !detached_ran; i++)
		thrd_yield();
	CHECK(detached_ran == 1);
	OK(pthread_attr_destroy(&at));

	/* A joinable thread detached after it started. */
	detached_ran = 0;
	OK(pthread_create(&t, NULL, detached_fn, NULL));
	OK(pthread_detach(t));
	for (int i = 0; i < 1000 && !detached_ran; i++)
		thrd_yield();
	CHECK(detached_ran == 1);
	/* Joining twice, or a thread that is not there, is not our promise; a
	 * small stack is refused as musl refuses it. */
	OK(pthread_attr_init(&at));
	CHECK(pthread_attr_setstacksize(&at, 100) == EINVAL);
	/* A bigger stack works. */
	OK(pthread_attr_setstacksize(&at, 512 * 1024));
	OK(pthread_create(&t, &at, ret_arg, (void *)1));
	OK(pthread_join(t, &r));
	CHECK(r == (void *)2);
	printf("t_threads: create passed\n");
}

/* ------------------------------------------------------------ mutex */

#define THREADS 8
#define ROUNDS 20000

static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
static long counter;

static void *count_up(void *a)
{
	(void)a;
	for (int i = 0; i < ROUNDS; i++) {
		pthread_mutex_lock(&counter_lock);
		long c = counter;
		if ((i & 255) == 0)
			thrd_yield(); /* hold the lock across a yield */
		counter = c + 1;
		pthread_mutex_unlock(&counter_lock);
	}
	return NULL;
}

static pthread_mutex_t held = PTHREAD_MUTEX_INITIALIZER;
static volatile int trylock_result, timedlock_result;

static void *try_held(void *a)
{
	(void)a;
	trylock_result = pthread_mutex_trylock(&held);
	struct timespec t;
	clock_gettime(CLOCK_REALTIME, &t);
	t.tv_nsec += 50 * 1000 * 1000;
	if (t.tv_nsec >= 1000000000) {
		t.tv_sec++;
		t.tv_nsec -= 1000000000;
	}
	timedlock_result = pthread_mutex_timedlock(&held, &t);
	return NULL;
}

static void mutex(void)
{
	pthread_t t[THREADS];
	for (int i = 0; i < THREADS; i++)
		OK(pthread_create(&t[i], NULL, count_up, NULL));
	for (int i = 0; i < THREADS; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(counter == (long)THREADS * ROUNDS);

	pthread_mutexattr_t ma;
	pthread_mutex_t rec, err;
	OK(pthread_mutexattr_init(&ma));
	OK(pthread_mutexattr_settype(&ma, PTHREAD_MUTEX_RECURSIVE));
	OK(pthread_mutex_init(&rec, &ma));
	OK(pthread_mutex_lock(&rec));
	OK(pthread_mutex_lock(&rec));
	OK(pthread_mutex_unlock(&rec));
	OK(pthread_mutex_unlock(&rec));
	CHECK(pthread_mutex_unlock(&rec) == EPERM);
	OK(pthread_mutexattr_settype(&ma, PTHREAD_MUTEX_ERRORCHECK));
	OK(pthread_mutex_init(&err, &ma));
	OK(pthread_mutex_lock(&err));
	CHECK(pthread_mutex_lock(&err) == EDEADLK);
	OK(pthread_mutex_unlock(&err));
	CHECK(pthread_mutex_unlock(&err) == EPERM);
	/* ADR-0302 item 6: nothing shared between processes, no priorities. */
	CHECK(pthread_mutexattr_setpshared(&ma, PTHREAD_PROCESS_SHARED) == ENOTSUP);
	CHECK(pthread_mutexattr_setprotocol(&ma, PTHREAD_PRIO_INHERIT) == ENOTSUP);

	pthread_t u;
	OK(pthread_mutex_lock(&held));
	double t0 = now();
	OK(pthread_create(&u, NULL, try_held, NULL));
	OK(pthread_join(u, NULL));
	CHECK(trylock_result == EBUSY);
	CHECK(timedlock_result == ETIMEDOUT);
	CHECK(now() - t0 >= 0.04);
	OK(pthread_mutex_unlock(&held));
	printf("t_threads: mutex passed\n");
}

/* ------------------------------------------------------------ cond */

#define RING 8
#define ITEMS 5000

static pthread_mutex_t ring_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t not_full = PTHREAD_COND_INITIALIZER;
static pthread_cond_t not_empty = PTHREAD_COND_INITIALIZER;
static int ring[RING], head, tail, used, done;
static long consumed_sum;

static void *producer(void *a)
{
	(void)a;
	for (int i = 1; i <= ITEMS; i++) {
		pthread_mutex_lock(&ring_lock);
		while (used == RING)
			pthread_cond_wait(&not_full, &ring_lock);
		ring[tail] = i;
		tail = (tail + 1) % RING;
		used++;
		pthread_cond_signal(&not_empty);
		pthread_mutex_unlock(&ring_lock);
	}
	pthread_mutex_lock(&ring_lock);
	done = 1;
	pthread_cond_broadcast(&not_empty);
	pthread_mutex_unlock(&ring_lock);
	return NULL;
}

static void *consumer(void *a)
{
	(void)a;
	for (;;) {
		pthread_mutex_lock(&ring_lock);
		while (used == 0 && !done)
			pthread_cond_wait(&not_empty, &ring_lock);
		if (used == 0) {
			pthread_mutex_unlock(&ring_lock);
			return NULL;
		}
		int v = ring[head];
		head = (head + 1) % RING;
		used--;
		consumed_sum += v;
		pthread_cond_signal(&not_full);
		pthread_mutex_unlock(&ring_lock);
	}
}

static pthread_mutex_t gate_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t gate = PTHREAD_COND_INITIALIZER;
static int open_gate, waiting, passed_gate;

static void *wait_gate(void *a)
{
	(void)a;
	pthread_mutex_lock(&gate_lock);
	waiting++;
	while (!open_gate)
		pthread_cond_wait(&gate, &gate_lock);
	passed_gate++;
	pthread_mutex_unlock(&gate_lock);
	return NULL;
}

static void cond(void)
{
	pthread_t p, c[3];
	OK(pthread_create(&p, NULL, producer, NULL));
	for (int i = 0; i < 3; i++)
		OK(pthread_create(&c[i], NULL, consumer, NULL));
	OK(pthread_join(p, NULL));
	for (int i = 0; i < 3; i++)
		OK(pthread_join(c[i], NULL));
	CHECK(consumed_sum == (long)ITEMS * (ITEMS + 1) / 2);

	/* A timed wait that nobody signals times out, and not early. */
	pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
	pthread_cond_t cv = PTHREAD_COND_INITIALIZER;
	struct timespec t;
	clock_gettime(CLOCK_REALTIME, &t);
	double t0 = now();
	t.tv_nsec += 100 * 1000 * 1000;
	if (t.tv_nsec >= 1000000000) {
		t.tv_sec++;
		t.tv_nsec -= 1000000000;
	}
	OK(pthread_mutex_lock(&m));
	CHECK(pthread_cond_timedwait(&cv, &m, &t) == ETIMEDOUT);
	OK(pthread_mutex_unlock(&m));
	double dt = now() - t0;
	CHECK(dt >= 0.09 && dt < 5.0);

	/* Broadcast wakes all of them. */
	pthread_t w[6];
	for (int i = 0; i < 6; i++)
		OK(pthread_create(&w[i], NULL, wait_gate, NULL));
	for (;;) {
		pthread_mutex_lock(&gate_lock);
		int n = waiting;
		pthread_mutex_unlock(&gate_lock);
		if (n == 6)
			break;
		thrd_yield();
	}
	pthread_mutex_lock(&gate_lock);
	open_gate = 1;
	pthread_cond_broadcast(&gate);
	pthread_mutex_unlock(&gate_lock);
	for (int i = 0; i < 6; i++)
		OK(pthread_join(w[i], NULL));
	CHECK(passed_gate == 6);
	printf("t_threads: cond passed\n");
}

/* ------------------------------------------------------------ once */

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int once_runs;
static volatile int start_flag;

static void once_fn(void)
{
	once_runs++;
	thrd_yield();
}

static void *race_once(void *a)
{
	(void)a;
	while (!start_flag)
		thrd_yield();
	pthread_once(&once, once_fn);
	CHECK(once_runs == 1);
	return NULL;
}

static void once_case(void)
{
	pthread_t t[THREADS];
	for (int i = 0; i < THREADS; i++)
		OK(pthread_create(&t[i], NULL, race_once, NULL));
	start_flag = 1;
	for (int i = 0; i < THREADS; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(once_runs == 1);
	printf("t_threads: once passed\n");
}

/* ------------------------------------------------------------ tls */

static _Thread_local int tl_array[64];
static _Thread_local int tl_counter = 5;
static pthread_key_t key;
static volatile int destructed;
#define TLS_ROUNDS 10000

static void destructor(void *v)
{
	destructed += (int)(intptr_t)v;
}

static void *tls_thread(void *a)
{
	int me = (int)(intptr_t)a;
	/* A locale pointer of this thread's own (INV-0303-2: errno and the
	 * locale pointer): a distinct, zeroed object for each thread, which
	 * uselocale only stores; it is put back before the thread ends. */
	static char fake_locale[5][1024];
	locale_t mine = (locale_t)fake_locale[me];
	CHECK(uselocale(mine) == LC_GLOBAL_LOCALE);
	CHECK(tl_counter == 5);
	for (int r = 0; r < TLS_ROUNDS; r++) {
		errno = me * 1000 + (r & 511);
		tl_array[r & 63] = me + r;
		tl_counter++;
		if ((r & 31) == 0)
			thrd_yield();
		CHECK(errno == me * 1000 + (r & 511));
		CHECK(uselocale((locale_t)0) == mine);
		CHECK(tl_array[r & 63] == me + r);
	}
	CHECK(tl_counter == 5 + TLS_ROUNDS);
	CHECK(uselocale(LC_GLOBAL_LOCALE) == mine);
	OK(pthread_setspecific(key, (void *)(intptr_t)me));
	CHECK(pthread_getspecific(key) == (void *)(intptr_t)me);
	return NULL;
}

static void tls(void)
{
	pthread_t t[4];
	OK(pthread_key_create(&key, destructor));
	CHECK(pthread_getspecific(key) == NULL);
	for (int i = 0; i < 4; i++)
		OK(pthread_create(&t[i], NULL, tls_thread, (void *)(intptr_t)(i + 1)));
	for (int i = 0; i < 4; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(destructed == 1 + 2 + 3 + 4);
	/* The main thread's own copies were not touched. */
	CHECK(tl_counter == 5 && tl_array[0] == 0);
	CHECK(uselocale((locale_t)0) == LC_GLOBAL_LOCALE);
	CHECK(pthread_getspecific(key) == NULL);
	OK(pthread_key_delete(key));
	printf("t_threads: tls passed\n");
}

/* ------------------------------------------------------------ cycles */

static pthread_mutex_t cycles_lock = PTHREAD_MUTEX_INITIALIZER;
static int cycles_done;

static void *bump(void *a)
{
	(void)a;
	pthread_mutex_lock(&cycles_lock);
	cycles_done++;
	pthread_mutex_unlock(&cycles_lock);
	return NULL;
}

static void cycles(void)
{
	for (int i = 0; i < 500; i++) {
		pthread_t t;
		OK(pthread_create(&t, NULL, bump, NULL));
		OK(pthread_join(t, NULL));
	}
	CHECK(cycles_done == 500);

	pthread_attr_t at;
	OK(pthread_attr_init(&at));
	OK(pthread_attr_setdetachstate(&at, PTHREAD_CREATE_DETACHED));
	for (int i = 0; i < 300; i++) {
		pthread_t t;
		int r;
		/* The quota of live threads is 128 or less: wait for room. */
		while ((r = pthread_create(&t, &at, bump, NULL)) == EAGAIN)
			thrd_yield();
		OK(r);
	}
	for (int i = 0; i < 20000 && cycles_done < 800; i++)
		thrd_yield();
	CHECK(cycles_done == 800);
	/* Batches of joinable threads, 16 alive at once. */
	for (int b = 0; b < 20; b++) {
		pthread_t t[16];
		for (int i = 0; i < 16; i++)
			OK(pthread_create(&t[i], NULL, bump, NULL));
		for (int i = 0; i < 16; i++)
			OK(pthread_join(t[i], NULL));
	}
	CHECK(cycles_done == 1120);
	printf("t_threads: cycles passed\n");
}

/* ------------------------------------------------------------ locks */

static pthread_rwlock_t rw = PTHREAD_RWLOCK_INITIALIZER;
static long rw_value, rw_bad;

static void *reader(void *a)
{
	(void)a;
	for (int i = 0; i < 2000; i++) {
		pthread_rwlock_rdlock(&rw);
		long v = rw_value;
		if ((i & 63) == 0)
			thrd_yield();
		if (v != rw_value || (v & 1))
			rw_bad++;
		pthread_rwlock_unlock(&rw);
	}
	return NULL;
}

static void *writer(void *a)
{
	(void)a;
	for (int i = 0; i < 500; i++) {
		pthread_rwlock_wrlock(&rw);
		rw_value++;
		thrd_yield();
		rw_value++;
		pthread_rwlock_unlock(&rw);
	}
	return NULL;
}

static pthread_barrier_t barrier;
static int phase_count[3];
static int serial;

static void *phased(void *a)
{
	(void)a;
	for (int p = 0; p < 3; p++) {
		__atomic_fetch_add(&phase_count[p], 1, __ATOMIC_SEQ_CST);
		int r = pthread_barrier_wait(&barrier);
		if (r == PTHREAD_BARRIER_SERIAL_THREAD)
			__atomic_fetch_add(&serial, 1, __ATOMIC_SEQ_CST);
		CHECK(__atomic_load_n(&phase_count[p], __ATOMIC_SEQ_CST) == 4);
	}
	return NULL;
}

static pthread_spinlock_t spin;
static long spun;

static void *spinner(void *a)
{
	(void)a;
	for (int i = 0; i < 5000; i++) {
		pthread_spin_lock(&spin);
		long v = spun;
		if ((i & 255) == 0)
			thrd_yield();
		spun = v + 1;
		pthread_spin_unlock(&spin);
	}
	return NULL;
}

static sem_t sem_a, sem_b;
static int sem_total;

static void *sem_worker(void *a)
{
	(void)a;
	for (int i = 0; i < 100; i++) {
		sem_wait(&sem_a);
		sem_total++;
		sem_post(&sem_b);
	}
	return NULL;
}

static void locks(void)
{
	pthread_t t[6];
	for (int i = 0; i < 4; i++)
		OK(pthread_create(&t[i], NULL, reader, NULL));
	for (int i = 4; i < 6; i++)
		OK(pthread_create(&t[i], NULL, writer, NULL));
	for (int i = 0; i < 6; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(rw_bad == 0 && rw_value == 2000);

	OK(pthread_barrier_init(&barrier, NULL, 4));
	for (int i = 0; i < 4; i++)
		OK(pthread_create(&t[i], NULL, phased, NULL));
	for (int i = 0; i < 4; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(serial == 3);
	OK(pthread_barrier_destroy(&barrier));

	OK(pthread_spin_init(&spin, 0));
	for (int i = 0; i < 4; i++)
		OK(pthread_create(&t[i], NULL, spinner, NULL));
	for (int i = 0; i < 4; i++)
		OK(pthread_join(t[i], NULL));
	CHECK(spun == 20000);

	OK(sem_init(&sem_a, 0, 0));
	OK(sem_init(&sem_b, 0, 0));
	CHECK(sem_init(&sem_a, 1, 0) == -1 && errno == ENOTSUP);
	OK(pthread_create(&t[0], NULL, sem_worker, NULL));
	for (int i = 0; i < 100; i++) {
		sem_post(&sem_a);
		sem_wait(&sem_b);
	}
	OK(pthread_join(t[0], NULL));
	CHECK(sem_total == 100);
	int v;
	OK(sem_getvalue(&sem_a, &v));
	CHECK(v == 0 && sem_trywait(&sem_a) == -1 && errno == EAGAIN);
	printf("t_threads: locks passed\n");
}

/* ------------------------------------------------------------ stdio */

static void *printer(void *a)
{
	int me = (int)(intptr_t)a;
	for (int i = 0; i < 20; i++) {
		printf("t_threads: line from %d\n", me);
		if (i % 5 == 0)
			thrd_yield();
	}
	return NULL;
}

static void stdio_case(void)
{
	pthread_t t[4];
	for (int i = 0; i < 4; i++)
		OK(pthread_create(&t[i], NULL, printer, (void *)(intptr_t)i));
	for (int i = 0; i < 4; i++)
		OK(pthread_join(t[i], NULL));
	printf("t_threads: stdio passed\n");
}

/* ------------------------------------------------------------ limit */

static pthread_mutex_t gate2 = PTHREAD_MUTEX_INITIALIZER;

static void *blocked_on_gate(void *a)
{
	(void)a;
	pthread_mutex_lock(&gate2);
	pthread_mutex_unlock(&gate2);
	return NULL;
}

static void limit(void)
{
	enum { MAX = 400 };
	static pthread_t t[MAX];
	int n = 0, r = 0;
	OK(pthread_mutex_lock(&gate2));
	while (n < MAX && (r = pthread_create(&t[n], NULL, blocked_on_gate, NULL)) == 0)
		n++;
	/* ADR-0300: at most 128 threads, the first included; or the room's
	 * figure or memory, whichever is first: always EAGAIN, never a hang. */
	CHECK(n >= 8 && n <= 127);
	CHECK(r == EAGAIN);
	OK(pthread_mutex_unlock(&gate2));
	for (int i = 0; i < n; i++)
		OK(pthread_join(t[i], NULL));
	/* Recovered: the same number again. */
	OK(pthread_mutex_lock(&gate2));
	int m = 0;
	while (m < n && pthread_create(&t[m], NULL, blocked_on_gate, NULL) == 0)
		m++;
	CHECK(m == n);
	OK(pthread_mutex_unlock(&gate2));
	for (int i = 0; i < m; i++)
		OK(pthread_join(t[i], NULL));
	printf("t_threads: limit passed (%d threads)\n", n);
}

/* ------------------------------------------------------------ ends */

static volatile long spun_up;

static void *spin_forever(void *a)
{
	(void)a;
	for (;;)
		spun_up++;
}

static void *exit_seven(void *a)
{
	while (spun_up < 1000)
		thrd_yield();
	printf("t_threads: exiting from a thread\n");
	exit(7);
	return a;
}

static void exit_case(void)
{
	pthread_t s, e;
	OK(pthread_create(&s, NULL, spin_forever, NULL));
	OK(pthread_create(&e, NULL, exit_seven, NULL));
	pthread_join(e, NULL);
	printf("t_threads: not reached FAIL\n");
}

static void *finish_later(void *a)
{
	for (int i = 0; i < 20; i++)
		thrd_yield();
	printf("t_threads: last thread leaving\n");
	return a;
}

static void mainexit(void)
{
	pthread_t t;
	OK(pthread_create(&t, NULL, finish_later, NULL));
	printf("t_threads: main thread leaving\n");
	pthread_exit(NULL);
}

static int blocked_pipe[2];

static void *read_forever(void *a)
{
	char c;
	(void)a;
	/* Blocked in a library wait made outside the library lock (ADR-0302
	 * item 9); the pipe never gets data. */
	read(blocked_pipe[0], &c, 1);
	return NULL;
}

static void blocked(void)
{
	pthread_t t;
	OK(pipe(blocked_pipe));
	OK(pthread_create(&t, NULL, read_forever, NULL));
	settle();
	/* _Exit must end the process with a sibling blocked in read: it does
	 * not wait for, or for the lock of, any thread. (Before ADR-0315 the
	 * reader held the library lock here; that is no longer the premise.) */
	_Exit(3);
}


/* ------------------------------------------------------------ waits */

/* INV-0302-2: a thread blocked in a long library wait does not stop another
 * thread's library calls. Each blocked wait below lasts a visible time; the
 * main thread's calls must take far less. */

static int wpipe[2];
static volatile int w_read_got = -1;
static volatile int w_sleep_done, w_select_done;

static void *w_reader(void *a)
{
	char c = 0;
	(void)a;
	w_read_got = read(wpipe[0], &c, 1) == 1 ? c : -2;
	return NULL;
}

static void *w_sleeper(void *a)
{
	struct timespec ts = {0, 400 * 1000 * 1000};
	(void)a;
	nanosleep(&ts, NULL);
	w_sleep_done = 1;
	return NULL;
}

static void *w_selector(void *a)
{
	int p[2];
	fd_set rs;
	struct timespec tv = {0, 400 * 1000 * 1000};
	(void)a;
	pipe(p);
	FD_ZERO(&rs);
	FD_SET(p[0], &rs);
	CHECK(pselect(p[0] + 1, &rs, NULL, NULL, &tv, NULL) == 0);
	w_select_done = 1;
	close(p[0]);
	close(p[1]);
	return NULL;
}

static void waits(void)
{
	pthread_t r, s, e;
	char path[64];
	struct stat st;
	double t0;

	OK(pipe(wpipe));
	OK(pthread_create(&r, NULL, w_reader, NULL));
	OK(pthread_create(&s, NULL, w_sleeper, NULL));
	OK(pthread_create(&e, NULL, w_selector, NULL));
	settle();
	/* All three are blocked now; the main thread uses the library. */
	t0 = now();
	for (int i = 0; i < 20; i++) {
		int fd;
		CHECK(getpid() >= 0);
		snprintf(path, sizeof path, "/work/w%d", i);
		fd = open(path, O_CREAT | O_WRONLY, 0644);
		CHECK(fd >= 0);
		CHECK(write(fd, "x", 1) == 1);
		CHECK(stat(path, &st) == 0 && st.st_size == 1);
		close(fd);
		unlink(path);
		printf("t_threads: waits main %d\n", i);
	}
	CHECK(now() - t0 < 0.3);
	CHECK(!w_sleep_done && !w_select_done);
	CHECK(w_read_got == -1);
	/* The write that the reader waits for. */
	CHECK(write(wpipe[1], "z", 1) == 1);
	OK(pthread_join(r, NULL));
	CHECK(w_read_got == 'z');
	OK(pthread_join(s, NULL));
	OK(pthread_join(e, NULL));
	CHECK(w_sleep_done && w_select_done);
	printf("t_threads: waits passed\n");
}


/* ------------------------------------------------------------ signals */

static volatile pthread_t usr1_on;
static volatile int usr1_count;
static volatile int usr1_in_worker;

static void on_usr1(int sig)
{
	(void)sig;
	usr1_on = pthread_self();
	usr1_count++;
}

enum { W_READ, W_SLEEP, W_SEM };
static int sig_pipe[2];
static sem_t sig_sem;
static volatile int sig_ready, sig_result, sig_errno;

static void *sig_worker(void *a)
{
	int what = (int)(intptr_t)a;
	char c;
	struct timespec ts = {30, 0};
	sig_ready = 1;
	switch (what) {
	case W_READ:
		sig_result = (int)read(sig_pipe[0], &c, 1);
		break;
	case W_SLEEP:
		sig_result = nanosleep(&ts, NULL);
		break;
	case W_SEM:
		sig_result = sem_wait(&sig_sem);
		break;
	}
	sig_errno = errno;
	return (void *)pthread_self();
}

static void wait_ready(void)
{
	wait_flag(&sig_ready);
	settle();
}

static void signals_case(void)
{
	struct sigaction sa;
	pthread_t t;
	void *self;
	double t0;
	/* The mask on entry is the launcher's (ADR-0190 item 18): start from none. */
	{
		sigset_t none;
		sigemptyset(&none);
		OK(pthread_sigmask(SIG_SETMASK, &none, NULL));
	}
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = on_usr1;
	OK(sigaction(SIGUSR1, &sa, NULL));
	OK(pipe(sig_pipe));
	OK(sem_init(&sig_sem, 0, 0));

	/* A thread in read: pthread_kill makes its read fail with EINTR, the
	 * handler runs on that thread, not on this one. */
	sig_ready = 0;
	OK(pthread_create(&t, NULL, sig_worker, (void *)(intptr_t)W_READ));
	wait_ready();
	t0 = now();
	OK(pthread_kill(t, SIGUSR1));
	OK(pthread_join(t, &self));
	printf("t_threads: signals step 1\n");
	CHECK(now() - t0 < 5);
	CHECK(sig_result == -1 && sig_errno == EINTR);
	CHECK(usr1_count == 1 && pthread_equal(usr1_on, (pthread_t)self));
	CHECK(!pthread_equal(usr1_on, pthread_self()));

	/* nanosleep: EINTR well before its 30 seconds. */
	sig_ready = 0;
	OK(pthread_create(&t, NULL, sig_worker, (void *)(intptr_t)W_SLEEP));
	wait_ready();
	t0 = now();
	OK(pthread_kill(t, SIGUSR1));
	OK(pthread_join(t, &self));
	printf("t_threads: signals step 2\n");
	CHECK(now() - t0 < 5);
	CHECK(sig_result == -1 && sig_errno == EINTR && usr1_count == 2);

	/* sem_wait: musl's loop treats the interrupted futex wait as a retry
	 * (INV-0304-7): the handler ran, the wait goes on until the post. */
	sig_ready = 0;
	OK(pthread_create(&t, NULL, sig_worker, (void *)(intptr_t)W_SEM));
	wait_ready();
	OK(pthread_kill(t, SIGUSR1));
	wait_count(&usr1_count, 3);
	OK(sem_post(&sig_sem));
	OK(pthread_join(t, &self));
	printf("t_threads: signals step 3\n");
	CHECK(sig_result == 0);

	/* The mask is per thread: this thread blocks SIGUSR1; a signal sent to
	 * it stays pending here (and a thread of the same process is not
	 * affected). */
	{
		sigset_t set, old;
		sigemptyset(&set);
		sigaddset(&set, SIGUSR1);
		OK(pthread_sigmask(SIG_BLOCK, &set, &old));
		OK(pthread_kill(pthread_self(), SIGUSR1));
		CHECK(usr1_count == 3);
		OK(pthread_sigmask(SIG_SETMASK, &old, NULL));
		CHECK(usr1_count == 4);
		CHECK(pthread_equal(usr1_on, pthread_self()));
	}
	printf("t_threads: signals passed\n");
}

/* ------------------------------------------------------------ cancel */

static volatile int cleaned;
static pthread_mutex_t cm = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cc = PTHREAD_COND_INITIALIZER;

static void cleanup(void *a)
{
	(void)a;
	cleaned++;
}

static void *cancel_worker(void *a)
{
	int what = (int)(intptr_t)a;
	char c;
	struct timespec ts = {30, 0};
	pthread_cleanup_push(cleanup, NULL);
	sig_ready = 1;
	switch (what) {
	case W_READ:
		read(sig_pipe[0], &c, 1);
		break;
	case W_SLEEP:
		nanosleep(&ts, NULL);
		break;
	case W_SEM:
		sem_wait(&sig_sem);
		break;
	case 3:
		pthread_mutex_lock(&cm);
		for (;;)
			pthread_cond_wait(&cc, &cm);
	case 4:
		for (;;)
			pthread_testcancel();
	}
	pthread_cleanup_pop(0);
	return NULL;
}

/* A cancel that a non-cancellation-point wait swallowed (ADR-0305 items 1
 * and 3, INV-0315-4): the worker waits for a mutex that main holds, is
 * cancelled meanwhile (the interrupt ends the futex wait, musl's wait loop
 * waits again, the one-shot interrupt flag is spent), gets the mutex, and
 * then calls read on an empty pipe: the pending cancel must act there and
 * not hang. */
static pthread_mutex_t held_m = PTHREAD_MUTEX_INITIALIZER;

static void *mutex_then_read(void *a)
{
	char c;
	(void)a;
	pthread_cleanup_push(cleanup, NULL);
	sig_ready = 1;
	pthread_mutex_lock(&held_m);
	pthread_mutex_unlock(&held_m);
	read(sig_pipe[0], &c, 1);
	pthread_cleanup_pop(0);
	return NULL;
}

/* Cancel disabled: the cancel waits for the enable. */
static volatile int survived_disabled, past_enable;

static void *disabled_then_enabled(void *a)
{
	int old;
	(void)a;
	pthread_cleanup_push(cleanup, NULL);
	OK(pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &old));
	sig_ready = 1;
	sem_wait(&sig_sem);
	survived_disabled = 1;
	OK(pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, &old));
	pthread_testcancel();
	past_enable = 1;
	pthread_cleanup_pop(0);
	return NULL;
}

static volatile int quick_done;

static void *quick(void *a)
{
	quick_done = 1;
	return a;
}

static void cancel_extras(void)
{
	pthread_t t;
	void *res = NULL;
	double t0;

	/* A cancel taken by a mutex wait. */
	cleaned = 0;
	sig_ready = 0;
	OK(pthread_mutex_lock(&held_m));
	OK(pthread_create(&t, NULL, mutex_then_read, NULL));
	wait_ready();
	OK(pthread_cancel(t));
	settle();
	t0 = now();
	OK(pthread_mutex_unlock(&held_m));
	OK(pthread_join(t, &res));
	printf("t_threads: cancel step mutex-then-read\n");
	CHECK(res == PTHREAD_CANCELED && cleaned == 1 && now() - t0 < 5);

	/* Disabled, then enabled. */
	cleaned = 0;
	sig_ready = 0;
	OK(pthread_create(&t, NULL, disabled_then_enabled, NULL));
	wait_ready();
	OK(pthread_cancel(t));
	settle();
	OK(sem_post(&sig_sem));
	OK(pthread_join(t, &res));
	printf("t_threads: cancel step disabled\n");
	CHECK(res == PTHREAD_CANCELED && survived_disabled && !past_enable && cleaned == 1);

	/* A thread that has ended (not joined yet): the cancel is harmless and
	 * the join returns its own value. */
	quick_done = 0;
	OK(pthread_create(&t, NULL, quick, (void *)7));
	wait_flag(&quick_done);
	settle();
	OK(pthread_cancel(t));
	OK(pthread_join(t, &res));
	printf("t_threads: cancel step gone\n");
	CHECK(res == (void *)7);
}

static void cancel_case(void)
{
	static const char *names[] = {"read", "nanosleep", "sem_wait", "cond_wait", "testcancel"};
	OK(pipe(sig_pipe));
	OK(sem_init(&sig_sem, 0, 0));
	for (int what = 0; what < 5; what++) {
		pthread_t t;
		void *res = NULL;
		double t0;
		cleaned = 0;
		sig_ready = 0;
		OK(pthread_create(&t, NULL, cancel_worker, (void *)(intptr_t)what));
		wait_ready();
		t0 = now();
		OK(pthread_cancel(t));
		OK(pthread_join(t, &res));
		printf("t_threads: cancel step %d\n", what);
		if (res != PTHREAD_CANCELED || cleaned != 1 || now() - t0 > 5) {
			printf("t_threads: cancel of %s: res %p cleaned %d FAIL\n", names[what],
			       res, cleaned);
			exit(1);
		}
	}
	cancel_extras();
	printf("t_threads: cancel passed\n");
}

/* ------------------------------------------------------------ selfkill */

/* pthread_kill and raise hold musl's killlock and block every signal around
 * the tkill of oneself; the handler must run only after both are released.
 * A handler that kills its own thread again, or exits it, deadlocks (the
 * lock is not recursive) if it runs inside. */
static volatile int sk1, sk2, sk_exit;

static void on_sk1(int sig)
{
	(void)sig;
	sk1++;
	pthread_kill(pthread_self(), SIGUSR2);
}

static void on_sk2(int sig)
{
	(void)sig;
	sk2++;
}

static void on_sk_exit(int sig)
{
	(void)sig;
	sk_exit++;
	pthread_exit((void *)9);
}

static void *selfkill_exit_thread(void *a)
{
	(void)a;
	pthread_kill(pthread_self(), SIGURG);
	return (void *)1; /* not reached: the handler ends the thread */
}

static void selfkill(void)
{
	struct sigaction sa;
	pthread_t t;
	void *res = NULL;
	sigset_t none;
	sigemptyset(&none);
	OK(pthread_sigmask(SIG_SETMASK, &none, NULL));
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = on_sk1;
	OK(sigaction(SIGUSR1, &sa, NULL));
	sa.sa_handler = on_sk2;
	OK(sigaction(SIGUSR2, &sa, NULL));
	sa.sa_handler = on_sk_exit;
	OK(sigaction(SIGURG, &sa, NULL));

	/* The handler of USR1 kills its own thread with USR2. */
	OK(pthread_kill(pthread_self(), SIGUSR1));
	CHECK(sk1 == 1 && sk2 == 1);
	/* The same through raise, from a second thread's point of view too. */
	OK(raise(SIGUSR1));
	CHECK(sk1 == 2 && sk2 == 2);
	/* Blocked: pending until unblocked, then delivered at once. */
	{
		sigset_t set, old;
		sigemptyset(&set);
		sigaddset(&set, SIGUSR1);
		OK(pthread_sigmask(SIG_BLOCK, &set, &old));
		OK(pthread_kill(pthread_self(), SIGUSR1));
		CHECK(sk1 == 2);
		OK(pthread_sigmask(SIG_SETMASK, &old, NULL));
		CHECK(sk1 == 3 && sk2 == 3);
	}
	/* A handler that ends the thread. */
	OK(pthread_create(&t, NULL, selfkill_exit_thread, NULL));
	OK(pthread_join(t, &res));
	CHECK(res == (void *)9 && sk_exit == 1);
	printf("t_threads: selfkill passed\n");
}

static void *read_for_exit(void *a)
{
	char c;
	(void)a;
	read(blocked_pipe[0], &c, 1);
	return NULL;
}

static void blockedexit(void)
{
	pthread_t t;
	static char buf[4096];
	OK(pipe(blocked_pipe));
	OK(pthread_create(&t, NULL, read_for_exit, NULL));
	settle();
	/* Fully buffered: exit() must write it with the sibling blocked. */
	setvbuf(stdout, buf, _IOFBF, sizeof buf);
	printf("t_threads: flushed at exit\n");
	exit(4);
}

/* The frame is 1 MiB, so its lowest byte is far below the thread's stack
 * mapping in one jump: this tests the layout (the mapping gap below the
 * stack is unmapped, INV-0302-6), not a guard page. musl's guard pages are
 * made with mprotect, which is ENOSYS here, so they stay writable; a thread
 * that overruns by less than the gap would run on. */
static void *overrun(void *a)
{
	volatile char big[1 << 20];
	for (size_t i = 0; i < sizeof big; i += 1024)
		big[i] = (char)i;
	(void)a;
	return NULL;
}

static void overflow(void)
{
	pthread_t t;
	OK(pthread_create(&t, NULL, overrun, NULL));
	pthread_join(t, NULL);
	printf("t_threads: not reached FAIL\n");
}

/* ------------------------------------------------------------ rounds
 * The 1000-round forms of INV-0304-3, -7, -8 and INV-0305-2, -4, -5. */

#define RR 1000

static volatile int hl_count;
static volatile pthread_t hl_on;
static int hl_pipe[2], hl_rd[2];

/* A handler that calls the library and the heap (INV-0304-3). */
static void hl_handler(int sig)
{
	(void)sig;
	int fd = open("/bin/t_threads", O_RDONLY);
	if (fd >= 0)
		close(fd);
	char c = 'h';
	if (write(hl_pipe[1], &c, 1) != 1)
		_exit(70);
	void *p = malloc(32);
	free(p);
	hl_on = pthread_self();
	hl_count++;
}

/* Waits (bounded, 10 s) for *v to reach n, with 1 ms sleeps. */
static void wait_fast(volatile int *v, int n)
{
	for (int i = 0; i < 10000 && *v < n; i++) {
		struct timespec ts = {0, 1000 * 1000};
		nanosleep(&ts, NULL);
	}
	CHECK(*v >= n);
}

static void install_hl(void)
{
	struct sigaction sa;
	sigset_t none;
	sigemptyset(&none);
	OK(pthread_sigmask(SIG_SETMASK, &none, NULL));
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = hl_handler;
	OK(sigaction(SIGUSR1, &sa, NULL));
	OK(pipe(hl_pipe));
	OK(pipe(hl_rd));
	hl_count = 0;
}

static void *hl_reader(void *a)
{
	char c;
	sig_ready = 1;
	for (;;) {
		ssize_t r = read(hl_rd[0], &c, 1);
		if (r == 1)
			break;
	}
	return a;
}

static void handlerlock(void)
{
	pthread_t t;
	install_hl();
	sig_ready = 0;
	OK(pthread_create(&t, NULL, hl_reader, NULL));
	wait_ready();
	for (int i = 0; i < RR; i++) {
		OK(pthread_kill(t, SIGUSR1));
		wait_fast(&hl_count, i + 1);
		if (i % 100 == 0) {
			/* The main thread's own calls complete meanwhile. */
			int fd = open("/bin/t_threads", O_RDONLY);
			CHECK(fd >= 0);
			close(fd);
		}
	}
	CHECK(pthread_equal(hl_on, t));
	CHECK(write(hl_rd[1], "x", 1) == 1);
	OK(pthread_join(t, NULL));
	printf("t_threads: handlerlock passed\n");
}

static pthread_mutex_t rc_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_mutex_t rc_m2 = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t rc_c = PTHREAD_COND_INITIALIZER;
static volatile int rc_pred, rc_done, rc_got, rc_wakes;

static void *rc_waiter(void *a)
{
	OK(pthread_mutex_lock(&rc_m));
	sig_ready = 1;
	while (!rc_pred) {
		OK(pthread_cond_wait(&rc_c, &rc_m));
		rc_wakes++;
	}
	rc_done = 1;
	OK(pthread_mutex_unlock(&rc_m));
	return a;
}

static void *rc_locker(void *a)
{
	sig_ready = 1;
	OK(pthread_mutex_lock(&rc_m2));
	rc_got = 1;
	OK(pthread_mutex_unlock(&rc_m2));
	return a;
}

/* INV-0304-8: a handler runs on a thread blocked in pthread_cond_wait or
 * pthread_mutex_lock; both go on waiting (the predicate and the lock decide). */
static void condsig(void)
{
	pthread_t t;
	install_hl();
	sig_ready = 0;
	OK(pthread_create(&t, NULL, rc_waiter, NULL));
	wait_ready();
	for (int i = 0; i < RR; i++) {
		OK(pthread_kill(t, SIGUSR1));
		wait_fast(&hl_count, i + 1);
		CHECK(!rc_done);
		if (i % 250 == 0)
			printf("t_threads: condsig cond round %d\n", i);
	}
	OK(pthread_mutex_lock(&rc_m));
	rc_pred = 1;
	OK(pthread_cond_signal(&rc_c));
	OK(pthread_mutex_unlock(&rc_m));
	OK(pthread_join(t, NULL));
	CHECK(rc_done);

	hl_count = 0;
	OK(pthread_mutex_lock(&rc_m2));
	sig_ready = 0;
	OK(pthread_create(&t, NULL, rc_locker, NULL));
	wait_ready();
	for (int i = 0; i < RR; i++) {
		OK(pthread_kill(t, SIGUSR1));
		wait_fast(&hl_count, i + 1);
		CHECK(!rc_got);
	}
	OK(pthread_mutex_unlock(&rc_m2));
	OK(pthread_join(t, NULL));
	CHECK(rc_got);
	printf("t_threads: condsig passed\n");
}

static void *opener(void *a)
{
	(void)a;
	for (;;) {
		int fd = open("/bin/t_threads", O_RDONLY);
		if (fd >= 0)
			close(fd);
	}
	return NULL;
}

/* INV-0305-2 and INV-0305-4. */
static void cancelrounds(void)
{
	struct sigaction sa;
	memset(&sa, 0, sizeof sa);
	sa.sa_handler = hl_handler;
	for (int n = 32; n <= 34; n++) {
		errno = 0;
		CHECK(sigaction(n, &sa, NULL) == -1 && errno == EINVAL);
	}
	for (int i = 0; i < RR; i++) {
		pthread_t t;
		void *r = NULL;
		OK(pthread_create(&t, NULL, opener, NULL));
		struct timespec ts = {0, (i % 4) * 300 * 1000};
		if (ts.tv_nsec)
			nanosleep(&ts, NULL);
		OK(pthread_cancel(t));
		OK(pthread_join(t, &r));
		CHECK(r == PTHREAD_CANCELED);
		int fd = open("/bin/t_threads", O_RDONLY);
		CHECK(fd >= 0);
		close(fd);
	}
	printf("t_threads: cancelrounds passed\n");
}

static pthread_mutex_t mc_m = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t mc_c = PTHREAD_COND_INITIALIZER;
static volatile int mc_ready;

static void *masked_waiter(void *a)
{
	int old;
	OK(pthread_setcancelstate(PTHREAD_CANCEL_MASKED, &old));
	OK(pthread_mutex_lock(&mc_m));
	mc_ready = 1;
	int r = pthread_cond_wait(&mc_c, &mc_m);
	pthread_mutex_unlock(&mc_m);
	return (void *)(intptr_t)r;
}

/* INV-0305-5: with masked cancellation a cancel is never lost at the wait:
 * pthread_cond_wait returns ECANCELED, and the thread is not terminated. */
static void maskedcancel(void)
{
	for (int i = 0; i < RR; i++) {
		pthread_t t;
		void *r = NULL;
		mc_ready = 0;
		OK(pthread_create(&t, NULL, masked_waiter, NULL));
		if (i % 2) {
			wait_fast(&mc_ready, 1);
			struct timespec ts = {0, 300 * 1000};
			nanosleep(&ts, NULL);
		}
		OK(pthread_cancel(t));
		OK(pthread_join(t, &r));
		if (r == PTHREAD_CANCELED || (intptr_t)r != ECANCELED) {
			printf("t_threads: maskedcancel round %d returned %ld FAIL\n", i, (long)(intptr_t)r);
			exit(1);
		}
	}
	printf("t_threads: maskedcancel passed\n");
}

/* ------------------------------------------------------------ attr */

static void *attr_thread(void *a)
{
	pthread_attr_t at;
	void *addr;
	size_t size;
	char name[16];
	pthread_t self = pthread_self();
	OK(pthread_getattr_np(self, &at));
	OK(pthread_attr_getstack(&at, &addr, &size));
	/* The thread's own stack holds a local. */
	CHECK((char *)&at >= (char *)addr && (char *)&at < (char *)addr + size);
	OK(pthread_attr_destroy(&at));
	OK(pthread_setname_np(self, "worker-1"));
	OK(pthread_getname_np(self, name, sizeof name));
	CHECK(strcmp(name, "worker-1") == 0);
	return a;
}

static void attr(void)
{
	pthread_attr_t at;
	void *addr;
	size_t size;
	char name[16];
	/* The main thread: the stack holds this frame. */
	OK(pthread_getattr_np(pthread_self(), &at));
	OK(pthread_attr_getstack(&at, &addr, &size));
	CHECK(size >= 16384);
	CHECK((char *)&at >= (char *)addr && (char *)&at < (char *)addr + size);
	OK(pthread_attr_destroy(&at));
	pthread_t t;
	OK(pthread_create(&t, NULL, attr_thread, NULL));
	/* Another thread's name, set from here (after it ran: it is joined). */
	OK(pthread_join(t, NULL));
	OK(pthread_setname_np(pthread_self(), "main"));
	OK(pthread_getname_np(pthread_self(), name, sizeof name));
	CHECK(strcmp(name, "main") == 0);
	CHECK(pthread_setname_np(pthread_self(), "0123456789abcdef") == ERANGE);
	CHECK(pthread_getname_np(pthread_self(), name, 8) == ERANGE);
	printf("t_threads: attr passed\n");
}

int main(int argc, char **argv)
{
	setvbuf(stdout, NULL, _IOLBF, 0);
	const char *mode = argc > 1 ? argv[1] : "all";
	int all = strcmp(mode, "all") == 0, known = 0;
#define CASE(name, fn)                                                     \
	if (all || strcmp(mode, name) == 0) {                              \
		known = 1;                                                 \
		fn();                                                      \
	}
	CASE("create", create)
	CASE("mutex", mutex)
	CASE("cond", cond)
	CASE("once", once_case)
	CASE("tls", tls)
	CASE("cycles", cycles)
	CASE("locks", locks)
	CASE("stdio", stdio_case)
	CASE("limit", limit)
	if (strcmp(mode, "exit") == 0) {
		exit_case();
		return 0;
	}
	if (strcmp(mode, "mainexit") == 0) {
		mainexit();
		return 0;
	}
	CASE("waits", waits)
	CASE("signals", signals_case)
	CASE("selfkill", selfkill)
	CASE("cancel", cancel_case)
	CASE("attr", attr)
	CASE("handlerlock", handlerlock)
	CASE("condsig", condsig)
	CASE("cancelrounds", cancelrounds)
	CASE("maskedcancel", maskedcancel)
	if (strcmp(mode, "blockedexit") == 0) {
		blockedexit();
		return 0;
	}
	if (strcmp(mode, "blocked") == 0) {
		blocked();
		return 0;
	}
	if (strcmp(mode, "overflow") == 0) {
		overflow();
		return 0;
	}
	if (!known) {
		printf("t_threads: unknown case %s\n", mode);
		return 2;
	}
	return 0;
}
