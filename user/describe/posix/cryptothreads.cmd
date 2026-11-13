name     cryptothreads
summary  SHA-256, AES-GCM and random bytes from eight threads at once (a POSIX program, the test of OpenSSL with threads)
usage    cryptothreads
program  /bin/cryptothreads
cwd      none
exit     0  every check held
exit     1  one did not
---
cryptothreads is the first half of the test of ADR-0309 item 4 (TASK-27 9a; tlsthreads is the second). Eight threads
each do 200 rounds of SHA-256, an AES-256-GCM encryption followed by the decryption, and RAND_bytes. The digests,
ciphertexts and tags must equal those that one thread computed before, and the random blocks must be non-zero and
all different. A child made by fork then asks for random bytes and must get none: it has no connection to the random
service (ADR-0310), so OpenSSL's generator fails closed. The program gets no file, no connection and no directory.

EXAMPLES
  cryptothreads
