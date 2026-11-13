/*
 * cryptothreads: OpenSSL's crypto with threads (ADR-0309 item 4, first half;
 * TASK-27 9a).
 *
 *   cryptothreads
 *
 * Eight threads each do 200 rounds of SHA-256, AES-256-GCM (encrypt, then
 * decrypt and compare) and RAND_bytes. The digests and the ciphertexts and
 * tags are compared with a reference computed by one thread before the others
 * start; the random blocks of all the threads must be non-zero and pairwise
 * distinct. Last, a fork child asks for random bytes: it has no connection to
 * the random service (ADR-0310), so OpenSSL's generator must refuse (0 from
 * RAND_bytes), not go on with the parent's state. Prints one line, exits 0
 * only when every check holds.
 */
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include <openssl/evp.h>
#include <openssl/rand.h>

#define THREADS 8
#define ROUNDS 200
#define RB 32
#define MSG 100

struct result {
	unsigned char digest[32];
	unsigned char ct[MSG];
	unsigned char tag[16];
};

static struct result ref[ROUNDS];
static struct result got[THREADS][ROUNDS];
static unsigned char rnd[THREADS][ROUNDS][RB];
static int failed[THREADS];
static char why[THREADS][96];

static void fail(int t, const char *w)
{
	if (!failed[t]++)
		snprintf(why[t], sizeof why[t], "%s", w);
}

/* The input of round i: the message is a pattern of i, the key the digest of it. */
static void inputs(int i, unsigned char msg[MSG], unsigned char key[32], unsigned char iv[12])
{
	for (int k = 0; k < MSG; k++)
		msg[k] = (unsigned char)(i * 7 + k * 13 + 1);
	EVP_Digest(msg, MSG, key, NULL, EVP_sha256(), NULL);
	memset(iv, 0, 12);
	iv[0] = (unsigned char)i;
	iv[1] = (unsigned char)(i >> 8);
}

/* One round; returns 0 on success. Fills r; with check, also decrypts and compares. */
static int round_(int i, struct result *r)
{
	unsigned char msg[MSG], key[32], iv[12], back[MSG];
	int len, total = 0;
	EVP_CIPHER_CTX *c = EVP_CIPHER_CTX_new();

	inputs(i, msg, key, iv);
	if (EVP_Digest(msg, MSG, r->digest, NULL, EVP_sha256(), NULL) != 1 || !c) {
		EVP_CIPHER_CTX_free(c);
		return -1;
	}
	if (EVP_EncryptInit_ex(c, EVP_aes_256_gcm(), NULL, key, iv) != 1 ||
	    EVP_EncryptUpdate(c, r->ct, &len, msg, MSG) != 1)
		goto bad;
	total = len;
	if (EVP_EncryptFinal_ex(c, r->ct + total, &len) != 1 ||
	    EVP_CIPHER_CTX_ctrl(c, EVP_CTRL_AEAD_GET_TAG, 16, r->tag) != 1)
		goto bad;
	EVP_CIPHER_CTX_free(c);
	c = EVP_CIPHER_CTX_new();
	if (!c || EVP_DecryptInit_ex(c, EVP_aes_256_gcm(), NULL, key, iv) != 1 ||
	    EVP_DecryptUpdate(c, back, &len, r->ct, MSG) != 1 ||
	    EVP_CIPHER_CTX_ctrl(c, EVP_CTRL_AEAD_SET_TAG, 16, r->tag) != 1 ||
	    EVP_DecryptFinal_ex(c, back + len, &len) != 1 || memcmp(back, msg, MSG) != 0)
		goto bad;
	EVP_CIPHER_CTX_free(c);
	return 0;
bad:
	EVP_CIPHER_CTX_free(c);
	return -1;
}

static void *worker(void *arg)
{
	int t = (int)(long)arg;

	for (int i = 0; i < ROUNDS; i++) {
		if (round_(i, &got[t][i]) != 0)
			fail(t, "digest or cipher call failed");
		else if (memcmp(&got[t][i], &ref[i], sizeof ref[i]) != 0)
			fail(t, "result differs from the single-threaded one");
		if (RAND_bytes(rnd[t][i], RB) != 1)
			fail(t, "RAND_bytes failed");
	}
	return NULL;
}

static int cmp(const void *a, const void *b)
{
	return memcmp(a, b, RB);
}

/* A child of fork has no random connection: RAND_bytes must fail, not reuse the parent's state. */
static int fork_check(void)
{
	pid_t p;
	int st;
	unsigned char b[RB];

	fflush(stdout);
	p = fork();
	if (p < 0)
		return -1;
	if (p == 0)
		_exit(RAND_bytes(b, RB) == 1 ? 1 : 0);
	if (waitpid(p, &st, 0) != p || !WIFEXITED(st))
		return -1;
	return WEXITSTATUS(st) == 0 ? 0 : -1;
}

int main(void)
{
	pthread_t th[THREADS];
	int started = 0, bad = 0;
	const char *first = "";
	static unsigned char all[THREADS * ROUNDS][RB];

	for (int i = 0; i < ROUNDS; i++)
		if (round_(i, &ref[i]) != 0) {
			printf("cryptothreads: FAIL: the reference round %d failed\n", i);
			return 1;
		}
	for (int t = 0; t < THREADS; t++) {
		if (pthread_create(&th[t], NULL, worker, (void *)(long)t) != 0)
			break;
		started++;
	}
	for (int t = 0; t < started; t++)
		pthread_join(th[t], NULL);
	if (started != THREADS) {
		printf("cryptothreads: FAIL: pthread_create\n");
		return 1;
	}
	for (int t = 0; t < THREADS; t++)
		if (failed[t]) {
			if (!bad)
				first = why[t];
			bad += failed[t];
		}
	if (bad) {
		printf("cryptothreads: FAIL: %d checks failed (%s)\n", bad, first);
		return 1;
	}
	for (int t = 0; t < THREADS; t++)
		for (int i = 0; i < ROUNDS; i++) {
			static const unsigned char zero[RB];

			if (!memcmp(rnd[t][i], zero, RB)) {
				printf("cryptothreads: FAIL: an all-zero random block\n");
				return 1;
			}
			memcpy(all[t * ROUNDS + i], rnd[t][i], RB);
		}
	qsort(all, THREADS * ROUNDS, RB, cmp);
	for (int i = 1; i < THREADS * ROUNDS; i++)
		if (!memcmp(all[i - 1], all[i], RB)) {
			printf("cryptothreads: FAIL: two random blocks are equal\n");
			return 1;
		}
	if (fork_check() != 0) {
		printf("cryptothreads: FAIL: a fork child got random bytes (or fork failed)\n");
		return 1;
	}
	printf("cryptothreads: %d threads x %d rounds of SHA-256, AES-256-GCM and RAND_bytes as the single-threaded; a fork child gets no random bytes\n",
	       THREADS, ROUNDS);
	return 0;
}
