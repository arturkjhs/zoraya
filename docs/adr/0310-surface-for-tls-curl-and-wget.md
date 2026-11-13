# ADR-0310: TLS, curl and wget as ports: the names libc.a adds, `getrandom` for C programs, and where the programs live

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0100, ADR-0109, ADR-0141, ADR-0153, ADR-0225, ADR-0227, ADR-0228, ADR-0287, ADR-0288, ADR-0289
- **Evidence:** CP-27

## Context
TASK-27 part 9a asks for OpenSSL 3, zlib, libcurl and `curl`, and `wget` as ports built by `zcc`, `getrandom` in
`libposix` over `rngd`, and the roots from `/system/tls/roots`. N4 (TASK-26) deferred curl and wget "to the next stage".
Four questions the accepted ADRs do not answer:

1. **Names.** `libc.a` exports the tier list and the added blocks (ADR-0141, ADR-0287). The four programs, linked,
   name more that musl has (hidden as `__zoraya_*`) or that libposix leaves out. Measured by linking: OpenSSL's
   libcrypto and libssl need `gmtime`, `strerror_r`, `getrandom`, `setbuf`, `tzset`; wget's gnulib and sources need
   `strncat`, `rand`, `srand`, `regcomp`, `regexec`, `regerror`, `regfree`, `sigismember`, `fnmatch` and five
   `*_unlocked` stdio names; curl needs `select`, `tcsetattr`, `bind` and `accept`; wget needs `link`, `setsid`,
   `setresuid`, `setresgid`, `clock_getres`, `rewinddir` and glibc's `program_invocation_name`.
2. **Entropy.** ADR-0227 item 4 says `getrandom` is one request on a connection every launcher gives every program;
   ADR-0288 built it for native programs and the shell only. A POSIX program starts from a bootstrap record and a second
   message (`ZPBS`, or `ZPBN` with the connection to `netd`, ADR-0287); neither carries a random connection.
3. **The TLS library.** ADR-0228 item 2 recommends BearSSL for POSIX programs, with Mbed TLS for TLS 1.3. TASK-27 (the
   architect, after that ADR) names OpenSSL 3. BearSSL has no TLS 1.3, which servers offer first. See ADR-0312.
4. **Where the programs live.** `rootfs/` is committed source; a C port cannot be. The tests must boot the image a
   person boots (`make run-net`) and find `curl` there.

## Decision drivers
1. The surface of `libc.a` changes only by ADR, with a program behind each name (INV-0005-1, CP-27).
2. Randomness designates nothing, so it needs no grant word (ADR-0227 item 3); when absent it must not become a
   fallback to anything weaker (ADR-0227 item 1: "stated, never silent").
3. A POSIX program is given what the shell gives it and nothing else (INV-0002-5, ADR-0153 rule 10).
4. The tests run in CI under `make test*` and need no network once `make fetch-ports` has run (CLAUDE.md).
5. A port is a pinned, hashed archive and patches (ADR-0109 item 6), never a committed copy.

## Considered options
### Option A: export the names, give the random connection in a new bootstrap flag, put the programs on the volume from `build/rootfs-apps` (the recommendation)
Add the names musl or libposix has to the exports in a `posix-surface-added` block; keep the names that only a rarely
called path reaches in a small archive linked into those two programs; extend the second bootstrap message; give
`tools/run-disk.sh` an `EXTRA_ROOT`.
- **Evidence for:** drivers 1 to 5. Each name has its program; nothing is exported for a name no path of the
  programs calls.
- **Evidence against:** a second place a name can live (the shim); exports grow.
- **Cost:** *(estimate)* 150 lines of Rust, 120 of C, a build section of 150 lines.
- **Precedent:** ADR-0210 (a block of names for products), ADR-0287 (the sockets).
### Option B: export nothing more and patch the programs until they name nothing outside the list
- **Description:** Patch curl, wget and OpenSSL to name only what `libc.a` exports.
- **Evidence against:** wget's gnulib replaces a function whenever a configure test cannot run, and the replacements
  name more (`rewinddir`, `setresuid`); the patches would be gnulib's, in volume.
- **Cost:** *(estimate)* large and re-done at every wget version.
### Option C: BearSSL and Mbed TLS as ADR-0228 item 2 says
- **Evidence against:** BearSSL has no TLS 1.3 and no release since 2018; Mbed TLS supports curl but not wget; TASK-27
  chose OpenSSL (ADR-0312).
### Option D: the Linux baseline: `getrandom` as a system call, `/dev/urandom` as a device, programs baked into the image
- **Evidence against:** no kernel `getrandom` (ADR-0288 option B); programs baked into `rootfs/` need the sources committed.
- **Precedent:** every distribution.

## Recommendation
**Recommendation (not a decision): Option A.**

1. **Ports.** `ports/sources.lock` pins OpenSSL 3.5.9 (the 3.5 LTS line, Apache-2.0), zlib 1.3.1 (the zlib licence),
   curl 8.23.0 (the curl licence, MIT-like) and wget 1.25.0 (GPL-3.0-or-later, as make and hello already are). `make
   apps` (`ports/Makefile`) builds them with `zcc` into `build/ports/deps` and the two programs into
   `build/rootfs-apps/bin`. OpenSSL: `./Configure linux-generic64 no-shared no-dso threads no-engine no-asm
   no-async no-sock no-dgram no-quic no-secure-memory no-comp no-legacy no-autoload-config no-ssl3 no-tls1 no-tls1_1
   no-dtls` (and their `-method` forms) `--with-rand-seed=getrandom`: TLS 1.2 and 1.3 are the only protocols and
   `getrandom` the only entropy source, with no `/dev/urandom` or `/dev/random` code compiled in. `linux-generic64` is a
   build recipe's name only: `zcc` defines nothing of Linux (INV-0143-1). `no-sock` removes the socket BIOs, and with
   them the unoffered `bind`, `listen`, `accept`, `gethostbyname` and `poll`; curl uses its own BIO. Four patches
   (`ports/patches/openssl/`, each under `__zoraya__`): `getrandom` as the seed source (`rand_unix.c`: no
   weak-symbol or `dlsym` lookup), no `posix_memalign` (`mem.c`), `/system/tls/roots` as the default
   certificate file (`common.h`), and no environment and no configuration file (item 6). curl: HTTP and HTTPS only, synchronous name resolution (`--disable-threaded-resolver`, although pthreads exist now: the programs stay single-threaded), no proxy, cookies,
   netrc or configuration file (item 6), CA bundle `/system/tls/roots`. wget: OpenSSL, zlib, IPv6 code built in (`getaddrinfo`
   answers IPv4 only), no libpsl or PCRE; one patch replaces `SSL_set_fd` by a plain-fd BIO (OpenSSL has no sockets
   here). Its configure runs with the host triple `aarch64-unknown-linux-musl` for gnulib's guesses only (gnulib
   answers the run tests it cannot run by the C library's name, and Zoraya's is musl; with the triple `zoraya` its
   `mbstate_t` replacement conflicts with musl's `uchar.h`), and with `ac_cv_header_uchar_h=no` and
   `ac_cv_func_rewinddir=yes`.
2. **Surface.** The block below adds 22 names: 21 that musl has and `libc.a` hides (`gmtime`, `strerror_r`, `setbuf`,
   `tzset`, `strncat`, `rand`, `srand`, `regcomp`, `regexec`, `regerror`, `regfree`, `sigismember`, `fnmatch` and the
   `*_unlocked` stdio names) and `getrandom`, which libposix now implements (item 3). None touches a handle. `rand` and
   `srand` are musl's ISO C generator, not random enough for anything that matters; OpenSSL never calls them and wget uses
   them for a temporary-file suffix.
3. **`getrandom`.** `zoraya-posix`'s `Posix` has a `random` connection beside `net`. `getrandom(buf, n, flags)` is one
   `Bytes` request per 2 KiB on it (`zoraya-rng`'s protocol), the whole of `n` or `-1`: no connection is `ENOSYS`
   (nothing weaker is substituted), an unknown flag is `EINVAL`, a short or wrong answer or a gone service is `EIO`
   and the buffer is untouched, a signal to deliver during the wait is `EINTR` (the call restarts or reports as any other). A `fork` child has none (a channel end cannot be duplicated, ADR-0102 item 3), as it has
   no network. The bootstrap's second message gains a form `ZPBX` plus one flag byte (`EXT_NET`, `EXT_RANDOM`) with the
   handles in the order SELF, net, random; `ZPBS` and `ZPBN` are unchanged. `zosh` makes one `Share` of its random
   connection for each POSIX program it starts, with no word on the line (ADR-0227 item 3). The service lets one connection make 64 others (`zoraya_rng::MAX_CHILDREN`): the shell's 65th live program connection is refused, that program starts without one and its `getrandom` is `ENOSYS`. Other launchers (the
   broker's `spawn` from a POSIX parent) give none yet: a `curl` started by `dash` has no entropy and fails to seed.
4. **The shim.** `ports/apps/zshim.c` is `libzshim.a`, linked last into `curl` and `wget` only, never into `libc.a`:
   `select` (= `pselect`, which `libc.a` exports), `bind`, `listen`, `accept`, `link`, `setsid`, `setresuid`,
   `setresgid`, `clock_getres` (`ENOSYS`), `tcsetattr` (`ENOTTY`), a no-op `rewinddir` that gnulib's `getcwd`
   fallback would reach only if musl's `getcwd` failed, and the two program-name variables. Each fails the way the
   unoffered call does (ADR-0287 item 9). They are not a surface: a program that does not name them does not link them.
5. **Where the programs live.** `tools/run-disk.sh` adds the files of `EXTRA_ROOT` (default `build/rootfs-apps` if it
   exists; `EXTRA_ROOT=` for none; a root that holds `/system` is refused) to the new volume over `rootfs/`'s, and puts
   their checksum in the disk's key, so a new `curl` makes a new disk and the old one is kept as `.old`. `make run`,
   `run-net`, `run-gui` and `make test-hand-run` therefore boot the same image with `curl` and `wget` once `make apps`
   has run. `make test-apps` (a step of `make test`) is the full path with the test PKI's root in `/system/tls/roots`;
   `make test-hand-run` runs the production image to the unknown-issuer point, as for `fetch`. The descriptions are
   the image's own (`user/describe/posix/curl.cmd`, `wget.cmd`, INV-0153-6); a program on the volume under any other
   name gets none. This is also the generic way to add programs built outside the repository that TASK-27 asks for
   (`EXTRA_ROOT`), without descriptions for programs this repository does not know.
6. **Nothing from the environment or a file decides what a program trusts** (review of 9a, finding 1; INV-0228-1). Under
   `__zoraya__`: OpenSSL's `ossl_safe_getenv` returns nothing (so `OPENSSL_CONF`, `SSL_CERT_FILE`, `SSL_CERT_DIR`,
   `RANDFILE` and `OPENSSL_MODULES` are not read; OpenSSL reads no proxy variable at all; curl and wget are built without a proxy, below) and `ossl_config_int` loads no `openssl.cnf`, whatever
   flag asks (wget passes `OPENSSL_INIT_LOAD_CONFIG`); curl does not look up `CURL_CA_BUNDLE`, `SSL_CERT_DIR` or
   `SSL_CERT_FILE` (`cacertpaths`), does not read `.curlrc` (`CURL_HOME`, `XDG_CONFIG_HOME`, `HOME`), refuses `--config`
   and writes no key log for `SSLKEYLOGFILE`; wget reads no `WGETRC`, `$HOME/.wgetrc`, `SYSTEM_WGETRC` or `/etc/wgetrc`
   (there is none on the image), refuses `--config` and `-e`, has HSTS off (so it writes no `~/.wget-hsts`, which no grant
   names) and no proxy. The descriptions do not offer the options that name roots or turn the check off either; both
   layers hold, and `make test-apps` tests the patches with the home directory readable (`+r .`).

```posix-surface-added
feof_unlocked
fflush_unlocked
fgets_unlocked
fnmatch
fputc_unlocked
fputs_unlocked
fread_unlocked
fwrite_unlocked
getrandom
gmtime
putc_unlocked
rand
regcomp
regerror
regexec
regfree
setbuf
sigismember
srand
strerror_r
strncat
tzset
```

## Consequences
- **Positive.** `curl` and `wget` fetch over TLS 1.2 and 1.3 inside Zoraya against the system's roots, clock and
  randomness; the surface grows by 22 names with a program behind each; the pattern for later C programs (CPython's
  `_ssl`, NetSurf's libcurl) is one OpenSSL and one `getrandom`.
- **Negative.** A second place a name can live (the shim); `docs/design.md` says so. `rand` and `srand` are exported
  although weak. The ports build takes minutes cold (OpenSSL); CI caches it with the other ports.
- **Follow-up.** OpenSSL is rebuilt with `threads` (done in 9a after 9b's libposix pthreads: `CRYPTO_THREAD_*` over `pthread_rwlock_*`, `pthread_once` and thread-local keys of the musl code; curl and wget stay single-threaded, their configure says `--disable-threaded-resolver` and `--disable-threads`); its test is `tests/apps/tlsthreads.c` in `make test-apps` (ADR-0309 item 4: 4 threads, 12 handshakes each against the test PKI: valid served, expired and wrong-name refused with 0 request bytes). The SHA-256/AES-GCM/`RAND_bytes` check of ADR-0309 item 4 (`tests/posix/ossl_threads.c`) is not written. `getrandom` for programs started by a launcher other than `zosh`.
  The `tls.roots` setting of ADR-0229 is not followed by C programs: they read `/system/tls/roots` only.

## Invariants
- **INV-0310-1.** `getrandom` returns bytes only from the random service's reply on the program's own connection: without the connection it fails with `ENOSYS`, a failure before the first chunk (2 KiB) writes nothing to the buffer and one after it returns the number of bytes written, never an error beside written bytes (Linux's `EINTR` rule), and it never falls back to another source, and a `fork` child has no connection. *Check:* `cargo test -p zoraya-posix --test random`; the guest's TLS handshakes in `make test-apps` draw on it (OpenSSL starts no handshake it cannot seed).
- **INV-0310-2.** The files of `EXTRA_ROOT` are on the volume only outside `/system`, and `tools/run-disk.sh` makes a new disk when their checksum changes. *Check:* `make test-hand-run` (the disk is made from `build/rootfs-apps`; a stale key replaces it) and `tools/test-run-disk.sh`.
- **INV-0310-3.** No port of this ADR is compiled with `__linux__` defined, and every change of `ports/patches/openssl/` is under a `__zoraya__` guard. *Check:* the end of `make apps` (a prerequisite of `make test-apps`) reads the compile commands of the logs of `zlib`, `openssl`, `curl` and `wget` and the OpenSSL patches, as `make check-ports` does for the other ports (INV-0109-2, INV-0143-1).
- **INV-0310-4.** curl and wget read no configuration file and no environment variable that names roots, a configuration or a proxy; OpenSSL reads no environment variable and loads no `openssl.cnf`. *Check:* `make test-apps` (an untrusted certificate is refused with `.curlrc`, `.wgetrc`, `WGETRC` and `SYSTEM_WGETRC` giving `insecure` or `check_certificate = off`, the server's log showing 0 request bytes; `CURL_CA_BUNDLE` and `SSL_CERT_FILE` naming the file of a root that signs nothing, `SSL_CERT_DIR` naming an existing hashed directory and `OPENSSL_CONF` naming an existing file that asks for an impossible protocol range change nothing: the valid server is still reached).
- **INV-0310-5.** The OpenSSL of the apps has no TLS before 1.2 and no entropy source but `getrandom`: neither program links `TLSv1_method`, `TLSv1_1_method`, `SSLv3_method` or `DTLSv1_method`, and neither names `/dev/urandom` or `/dev/random`. *Check:* the end of `make apps` (a prerequisite of `make test-apps`), which fails if `nm` fails or does not list `TLS_method` (a control, so that a silent `nm` is not a pass).

## Open questions
- A POSIX program started by `dash` or `make` (posix_spawn through the broker) has no random connection: `getrandom`
  gives `ENOSYS`. The broker could `Share` one into the child's record. Not built: no program of this stage needs it.
- The roots of a POSIX program are the directory `/system/tls` (a POSIX program is given directories only, ADR-0153
  rule 10), not the file; the directory holds the file alone (`make check-tls-roots`). `grants` shows `/system/tls  read
  (with the network)`, not the file as ADR-0228 item 3 words it.
- OpenSSL 4.x exists; 3.5 is the LTS and TASK-27 says 3.
- Note (ADR-0001 rule 6, 2026-11-13): item 6's "both layers hold" did not: the descriptions do not offer the options, but
  the programs took them (the descriptions' "there is no option to turn that off" held only through the shell); a line
  that matches no description ran the programs with default settings and kept its `+net` words, so `-k` and
  `--no-check-certificate` took effect. It now holds in the programs (curl's and wget's option handling refuses them): the
  proposed ADR-0321 states the options and its invariants. The invariants of this ADR are unchanged.

## Revisit triggers
- A program that needs `bind`, `listen` or `accept` (a server): the shim's refusals become real calls in `libc.a`.
- OpenSSL 3.5 reaching end of support, or a vulnerability with no 3.5 fix.

## Architect's decision
Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.

Option A. "`curl` started by `dash` has no `getrandom` connection" stays in the backlog.
