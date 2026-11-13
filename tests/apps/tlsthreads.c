/*
 * tlsthreads: OpenSSL with threads (ADR-0309 item 4, TASK-27 9a).
 *
 *   tlsthreads [-r expired|name] URL...
 *
 * Four threads run TLS handshakes at the same time, each of them ROUNDS times
 * for every URL (https://HOST:PORT/), over one shared SSL_CTX that trusts
 * /system/tls/roots and checks the certificate's name: the locks of the
 * context, its store, the error queue and the provider init are all taken from
 * four threads at once. Without -r every handshake must succeed and the
 * server's page must come back; with -r REASON every handshake must be refused
 * for that certificate reason (expired: X509_V_ERR_CERT_HAS_EXPIRED; name:
 * X509_V_ERR_HOSTNAME_MISMATCH) before the program sends a byte. Prints one line and
 * exits 0 only when all the handshakes went as expected.
 */
#include <errno.h>
#include <netdb.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#include <openssl/err.h>
#include <openssl/ssl.h>
#include <openssl/x509v3.h>

#define THREADS 4
#define ROUNDS 3
#define MAX_URLS 4

struct target {
	char host[128];
	char port[8];
};

static struct target targets[MAX_URLS];
static int ntargets;
static int expect_refusal;
static long expect_error; /* the X509_V_ERR_* that -r asks for */
static SSL_CTX *ctx;

static pthread_mutex_t report_lock = PTHREAD_MUTEX_INITIALIZER;
static int good, bad;
static char first_bad[160];

static void note(int ok, const char *why)
{
	pthread_mutex_lock(&report_lock);
	if (ok)
		good++;
	else {
		if (!bad++)
			snprintf(first_bad, sizeof first_bad, "%s", why);
	}
	pthread_mutex_unlock(&report_lock);
}

/* A dial that failed only because the shell's connection account is full: try again. */
static __thread int dial_busy;

static int dial(const struct target *t)
{
	struct addrinfo hints, *res;
	int fd = -1;

	memset(&hints, 0, sizeof hints);
	hints.ai_family = AF_INET;
	hints.ai_socktype = SOCK_STREAM;
	dial_busy = 0;
	{
		int e = getaddrinfo(t->host, t->port, &hints, &res);

		if (e != 0) {
			dial_busy = e == EAI_AGAIN;
			return -1;
		}
	}
	fd = socket(res->ai_family, res->ai_socktype, 0);
	if (fd >= 0 && connect(fd, res->ai_addr, res->ai_addrlen) != 0) {
		/* ENOBUFS: over the account's quota; EAGAIN: the stack is busy. */
		dial_busy = errno == ENOBUFS || errno == EAGAIN;
		close(fd);
		fd = -1;
	}
	freeaddrinfo(res);
	return fd;
}

/* One handshake; with success expected, one request and the page's marker. */
static void one(const struct target *t)
{
	int fd = dial(t);
	SSL *ssl;
	/* The shell gives a POSIX program a connection account of POSIX_NET_BUDGET (2)
	   sockets at a time: a thread that finds none free waits and tries again. */
	for (int tries = 0; fd < 0 && dial_busy && tries < 300; tries++) {
		nanosleep(&(struct timespec){0, 50 * 1000 * 1000}, NULL);
		fd = dial(t);
	}
	char buf[2048];
	size_t have = 0;
	int n;

	if (fd < 0) {
		note(0, "connect failed");
		return;
	}
	ssl = SSL_new(ctx);
	if (!ssl) {
		close(fd);
		note(0, "SSL_new failed");
		return;
	}
	{
		/* This OpenSSL has no-sock: a plain descriptor BIO, as wget's patch uses. */
		BIO *bio = BIO_new_fd(fd, BIO_NOCLOSE);

		if (!bio) {
			SSL_free(ssl);
			close(fd);
			note(0, "BIO_new_fd failed");
			return;
		}
		SSL_set_bio(ssl, bio, bio);
	}
	SSL_set_tlsext_host_name(ssl, t->host);
	SSL_set1_host(ssl, t->host);
	if (SSL_connect(ssl) != 1) {
		long v = SSL_get_verify_result(ssl);
		if (expect_refusal && v == expect_error)
			note(1, "");
		else if (expect_refusal)
			note(0, "refused for another reason than the one expected");
		else
			note(0, "handshake failed");
		goto out;
	}
	if (expect_refusal) {
		note(0, "handshake succeeded");
		goto out;
	}
	{
		const char req[] = "GET / HTTP/1.1\r\nHost: test.zoraya\r\nConnection: close\r\n\r\n";

		if (SSL_write(ssl, req, sizeof req - 1) != (int)sizeof req - 1) {
			note(0, "write failed");
			goto out;
		}
	}
	while (have < sizeof buf - 1 && (n = SSL_read(ssl, buf + have, (int)(sizeof buf - 1 - have))) > 0)
		have += (size_t)n;
	buf[have] = 0;
	note(strstr(buf, "Zoraya test page") != NULL, "page not in the reply");
out:
	SSL_shutdown(ssl);
	SSL_free(ssl);
	close(fd);
}

static void *worker(void *arg)
{
	(void)arg;
	for (int r = 0; r < ROUNDS; r++)
		for (int i = 0; i < ntargets; i++)
			one(&targets[i]);
	return NULL;
}

static int parse(const char *url, struct target *t)
{
	const char *p = strstr(url, "https://");
	const char *colon, *slash;
	size_t hl;

	if (!p || p != url)
		return -1;
	p += 8;
	colon = strchr(p, ':');
	slash = strchr(p, '/');
	if (!colon || (slash && slash < colon))
		return -1;
	hl = (size_t)(colon - p);
	if (hl == 0 || hl >= sizeof t->host)
		return -1;
	memcpy(t->host, p, hl);
	t->host[hl] = 0;
	{
		size_t pl = slash ? (size_t)(slash - colon - 1) : strlen(colon + 1);

		if (pl == 0 || pl >= sizeof t->port)
			return -1;
		memcpy(t->port, colon + 1, pl);
		t->port[pl] = 0;
	}
	return 0;
}

int main(int argc, char **argv)
{
	pthread_t th[THREADS];
	int started = 0;

	for (int i = 1; i < argc; i++) {
		if ((!strcmp(argv[i], "-r") || !strcmp(argv[i], "--refuse")) && i + 1 < argc) {
			const char *w = argv[++i];

			expect_refusal = 1;
			if (!strcmp(w, "expired"))
				expect_error = X509_V_ERR_CERT_HAS_EXPIRED;
			else if (!strcmp(w, "name"))
				expect_error = X509_V_ERR_HOSTNAME_MISMATCH;
			else {
				fprintf(stderr, "tlsthreads: -r takes expired or name\n");
				return 2;
			}
		} else if (ntargets < MAX_URLS && parse(argv[i], &targets[ntargets]) == 0)
			ntargets++;
		else {
			fprintf(stderr, "tlsthreads: usage: tlsthreads [-r expired|name] https://HOST:PORT/...\n");
			return 2;
		}
	}
	if (!ntargets) {
		fprintf(stderr, "tlsthreads: no URL\n");
		return 2;
	}
	ctx = SSL_CTX_new(TLS_client_method());
	if (!ctx || SSL_CTX_load_verify_locations(ctx, "/system/tls/roots", NULL) != 1) {
		fprintf(stderr, "tlsthreads: cannot set up the context\n");
		return 2;
	}
	SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);
	for (int i = 0; i < THREADS; i++) {
		if (pthread_create(&th[i], NULL, worker, NULL) != 0) {
			fprintf(stderr, "tlsthreads: pthread_create failed\n");
			break;
		}
		started++;
	}
	for (int i = 0; i < started; i++)
		pthread_join(th[i], NULL);
	if (started != THREADS || bad || good != THREADS * ROUNDS * ntargets) {
		printf("tlsthreads: FAIL: %d as expected, %d not (%s)\n", good, bad, bad ? first_bad : "threads");
		return 1;
	}
	printf("tlsthreads: %d handshakes from %d threads, all %s\n", good, THREADS,
	       expect_refusal ? "refused" : "verified and served");
	return 0;
}
