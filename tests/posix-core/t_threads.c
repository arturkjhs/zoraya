/*
 * Part 9b's threads through libposix (ADR-0302, ADR-0303, ADR-0309 item 3):
 * a static C program, built with zcc against libc.a, run by posixrun. One
 * binary with named cases; each prints "t_threads: NAME passed" or a line ending in FAIL and exits 1. Cases that need signals, cancellation of a blocked
 * thread, fork or exec with siblings come with step 3.
 *
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
 *   t_threads overflow  a thread overruns its stack: the process faults (INV-0302-6)
 *   t_threads all       every case but the last three, which end the process
 */
#include <errno.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>
#include <time.h>
#include <unistd.h>

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

static void destructor(void *v)
{
	destructed += (int)(intptr_t)v;
}

static void *tls_thread(void *a)
{
	int me = (int)(intptr_t)a;
	CHECK(tl_counter == 5);
	for (int r = 0; r < 2000; r++) {
		errno = me * 1000 + r;
		tl_array[r & 63] = me + r;
		tl_counter++;
		if ((r & 31) == 0)
			thrd_yield();
		CHECK(errno == me * 1000 + r);
		CHECK(tl_array[r & 63] == me + r);
	}
	CHECK(tl_counter == 5 + 2000);
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
