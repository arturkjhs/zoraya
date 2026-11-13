# ADR-0320: CPython 3.15 as a port: a static build with `zcc`, the shims, five patches, the environment policy

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0005, ADR-0100, ADR-0109, ADR-0153, ADR-0224, ADR-0228, ADR-0302, ADR-0309, ADR-0310, ADR-0311, ADR-0312, ADR-0315, ADR-0319
- **Evidence:** CP-27

## Context
TASK-27 part 9c asks for CPython as the first language runtime of the stage: `python3` from the volume, with `sqlite3`,
`ssl` and `asyncio`, and a subset of its own test suite passing. What exists: `zcc` and the ports machinery (ADR-0109), OpenSSL
3.5 with threads (ADR-0310, ADR-0312), pthreads (ADR-0302), record locks for SQLite (ADR-0319), `EXTRA_ROOT` for programs that
are not in the repository (ADR-0310), and descriptions that give a POSIX program only what its line names (ADR-0224).

Measured (9c): CPython 3.15.2 cross-builds static (15.5 MB executable, 3 MB of it ours: no shared modules, no `dlopen`), with
`libc.a` missing ~170 names (block below), 15 of them shimmed; the standard library is 37 MB trimmed (the image part is 52 MB with
the executable, 1226 files). `python3 -V` and `-c` start in under a second under HVF.

Facts found by running it: a Zoraya process starts on a 64 KiB stack (`zoraya_abi::spawn::USER_STACK_SIZE`), and CPython (3.14 and
later) reserves 32 KiB at the top of the stack for its overflow guard and raises `RecursionError` once under it, so the first
`import` of the standard library failed (`RecursionError` while evaluating `getpath`).

## Decision drivers
1. The program must run unchanged where it can: the fewer changes to CPython, the cheaper the next version (CP-27).
2. A Python program is a POSIX program: it gets what its line gives it and nothing the environment says (ADR-0224, ADR-0310 item 6).
3. The pin must be checkable (ADR-0109 item 6) and the build reproducible without the network after `make fetch-ports`.
4. Honesty about what is absent (no `dlopen`, no `AF_UNIX`, no server sockets, no `socketpair`).

## Considered options
### Option A: a static build, a small shim library, five patches (the recommendation)
- **Description:** `ports/Makefile` builds the host Python of the same version (for `--with-build-python` and to compile the
  library's byte code), SQLite for the target, `libpyshim.a` (`ports/apps/pyshim.c`), and CPython with `MODULE_BUILDTYPE=static`.
- **Evidence for:** drivers 1 to 4; the build ran end to end, `python3 -c` runs in the guest.
- **Evidence against:** 15 MB per image that holds it; the shims return an error where the call has no meaning, and a program
  that depends on it fails at run time.
- **Cost:** *(measured)* 224 lines of C, 5 patches of 20 to 50 lines each.
- **Precedent:** Alpine, Termux and Redox's port build CPython static with a similar list of `ac_cv_*`.

### Option B: no patches, only `libpyshim` and configure answers
- **Evidence against:** `_ssl` needs a socket BIO (OpenSSL is `no-sock`, ADR-0310), the environment would decide `sys.path` (driver 2) and `asyncio` could not start (no `socketpair`).

### Option C: shared extension modules loaded by `dlopen`
- **Evidence against:** `dlopen` is not in libposix and ADR-0100 keeps the loader out; a second ABI to maintain.

### Option D: a smaller interpreter (MicroPython)
- **Evidence against:** the task names CPython and its test suite; `sqlite3`, `ssl` and `asyncio` are the point.

## Recommendation
Option A. Marked as a recommendation, not a decision. Items, each of which the invariants below name where checkable:

1. **Pin.** CPython 3.15.2, `Python-3.15.2.tar.xz`, SHA-256 `8b5536d2...797de` in `ports/sources.lock`. It is the SHA-256 python.org
   publishes for that file, and `make fetch-ports` refuses another (INV-0109-4). python.org also publishes a Sigstore bundle
   for it; checking it is optional and is not done by the tools.
2. **Build.** Static, `MACHDEP=zoraya`, `--host=aarch64-unknown-linux-musl`, `--disable-shared`, `--disable-ipv6` (ADR-0225),
   `--without-ensurepip`, `--without-mimalloc`, `--disable-test-modules`, with the functions the headers declare and `libc.a` lacks
   said absent (`PYTHON_ABSENT`: `posix_spawn`, `epoll`, `eventfd`, `socketpair`, `fchdir`, `link`, ...). The built-in modules: `_socket`, `select`,
   `_ssl`, `_hashlib` (OpenSSL 3.5, threads), `_sqlite3` (SQLite 3.54, `THREADSAFE=1`, `OMIT_LOAD_EXTENSION`, `OMIT_WAL`, FTS5),
   `zlib`, `_asyncio`, `_thread`. Not built: `ctypes`, `readline`, `curses`, `tkinter`, `bz2`, `lzma`, `syslog`, `_posixsubprocess`'s `posix_spawn` path.
3. **Shims.** `libpyshim.a` supplies `execv`, `madvise`, `msync`, `sched_yield`, `pause`, `dirfd` (`ENOTSUP`), `gethostbyname` (through
   `getaddrinfo`), `getprotobyname`, `getservbyname`/`getservbyport` over a small table, `hstrerror` and a few more; each returns a
   value a caller handles, or an error, never a silent success of an action that did not happen.
4. **Names `libc.a` adds** (the block below): libm, `atol`, the wide-character string functions and a few others that CPython's
   configure finds in the headers; they follow ADR-0310's rule that `libc.a` exports what musl's own source gives and `libposix` already serves.
5. **Patches** (`ports/patches/python/`, every change under `__zoraya__` or in a file only this port reads):
   `0001` `_ssl` reads and writes the socket's descriptor through a file-descriptor BIO (OpenSSL is `no-sock`);
   `0002` the interpreter starts with `use_environment=0`, `write_bytecode=0` and `user_site_directory=0` (item 6);
   `0003` `asyncio`'s self-pipe is the two ends of `os.pipe()` where `_socket` has no `socketpair`;
   `0004` `main` runs the interpreter on a thread with an 8 MiB stack and joins it (a process starts on 64 KiB; the interpreter
   assumes 4 MB); the thread library's stack of that size is the process's own memory, taken when used;
   `0005` `getpath` does not look for `pyvenv.cfg` (there are no virtual environments, and the search read `/pyvenv.cfg` and `/bin/pyvenv.cfg`, which a program is not given).
6. **Environment policy.** `PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `PYTHONWARNINGS`, `SSL_CERT_FILE` and the others decide nothing (the
   program runs as with `-E`); no byte code is written; no per-user site directory. The standard library's byte code is compiled once
   by the host's Python, `unchecked-hash` (the volume keeps no modification time to check against). `ssl.create_default_context()`
   loads no file the environment names; a program names `/system/tls/roots` itself (the system's roots, given with the network, ADR-0311 item 2).
7. **Description policy** (`user/describe/posix/python3.cmd`): a POSIX program with the toolchain set: `/bin /usr /etc` to read, a `/tmp` of its
   own, the current directory to read, what its operands and `+r`, `+w`, `+net` give; the network only with `+net HOST:PORT`, and the roots
   with it. Options offered: `-c -m -b -B -I -O -P -q -s -u -V -W`; not `-E -X -i -d -v -x` and no interactive session (no terminal from zosh, ADR-0153).
8. **Sizes and files.** The image part is `build/rootfs-python` (`/bin/python3`, 15.5 MB, and `/usr/lib/python3.15`, 37 MB, 1226 files, 52 MB in all);
   `make python` builds it, `make test-python` boots it. `tools/run-disk.sh` takes several `EXTRA_ROOT` directories separated by colons and a
   `DISK_SIZE` (default 64M; curl, wget and the Python part need 192M): the volume of `make run` is made with
   `EXTRA_ROOT=build/rootfs-apps:build/rootfs-python DISK_SIZE=192M`.
9. **Tests.** `tests/python/smoke.py` (the flags, `sqlite3`, `ssl`, `asyncio`, threads, `fork`), `tests/python/https_client.py` (an `asyncio` HTTPS client
   against `tools/test-httpd`'s TLS server, and the refusal of an expired and a wrong-name certificate with 0 request bytes), and
   `tests/python/regr.py`, which runs CPython's `regrtest` over the modules of `tests/python/regr-modules.txt` (the interpreter's core types,
   `re`, `json`, `zlib`, `hashlib`, `sqlite3`, `threading`, `asyncio`'s tasks, futures and locks). Not run: modules that need `AF_UNIX`, a server socket, `ctypes`,
   shared modules or a terminal; each is recorded in `docs/design.md` where known.


### Names `libc.a` adds

```posix-surface-added
acosf
acosh
acoshf
acoshl
acosl
asinf
asinh
asinhf
asinhl
asinl
atan
atan2f
atan2l
atanf
atanh
atanhf
atanhl
atanl
atol
cbrt
cbrtf
cbrtl
ceilf
ceill
copysign
copysignf
copysignl
cosf
cosh
coshf
coshl
cosl
erf
erfc
erfcf
erfcl
erff
erfl
exp2
exp2f
exp2l
expf
expl
expm1
expm1f
expm1l
fabsf
fabsl
fdim
fdimf
fdiml
floorf
floorl
fma
fmaf
fmal
fmax
fmaxf
fmaxl
fmin
fminf
fminl
fmodf
fmodl
frexpf
frexpl
hypot
hypotf
hypotl
ilogb
ilogbf
ilogbl
ldexpf
lgamma
lgammaf
lgammal
llrint
llrintf
llrintl
llround
llroundf
llroundl
log10f
log10l
log1p
log1pf
log1pl
log2f
log2l
logb
logbf
logbl
logf
logl
lrint
lrintf
lrintl
lround
lroundf
lroundl
mbstowcs
modf
modff
modfl
nan
nanf
nanl
nearbyint
nearbyintf
nearbyintl
nextafter
nextafterf
nextafterl
nexttoward
nexttowardf
nexttowardl
powf
powl
remainder
remainderf
remainderl
remquo
remquof
remquol
rint
rintf
rintl
round
roundf
roundl
scalbln
scalblnf
scalblnl
scalbn
scalbnf
scalbnl
sinf
sinh
sinhf
sinhl
sinl
sqrtf
sqrtl
tanf
tanh
tanhf
tanhl
tanl
tgamma
tgammaf
tgammal
timegm
trunc
truncf
truncl
wcscat
wcscmp
wcscoll
wcscpy
wcsftime
wcslen
wcsncmp
wcsncpy
wcsrchr
wcstok
wcstol
wcstombs
wcsxfrm
wmemchr
wmemcmp
```

## Consequences
- **Positive.** Python programs run from the volume; the standard library's tests exercise threads, `sqlite3` and `ssl` in a way no C test of ours does.
- **Negative.** 52 MB on a volume; a process with `main` on a thread has an 8 MiB stack of virtual memory (only touched pages cost); `socket.socketpair`,
  `AF_UNIX`, servers and `posix_spawn` do not exist; a program that Python starts has no connection to the random service (ADR-0310), so `subprocess.run([sys.executable, ...])` fails at start with "failed to get random numbers"; a program that sets `PYTHONPATH` is not obeyed.
- **Follow-up.** `bind`, `listen`, `accept` (a Python server) when the network service has them; a larger process stack in the kernel would make patch 0004 unnecessary.

## Invariants
- **INV-0320-1.** The interpreter runs with the environment ignored, no byte code written and no user site: `sys.flags.ignore_environment` is 1, `sys.dont_write_bytecode` is true, `sys.flags.no_user_site` is 1. *Check:* `make test-python` (`smoke.py` prints them).
- **INV-0320-2.** `python3` starts from the volume and imports the standard library, `sqlite3`, `ssl` and `asyncio`, runs a thread and a `fork` child. *Check:* `make test-python` (`smoke.py`).
- **INV-0320-3.** An `asyncio` HTTPS client verifies the chain, the time and the name against `/system/tls/roots` and sends no request byte after a failed verification. *Check:* `make test-python` (expired and wrong-name servers: the server's log shows `ended after 0 request bytes`).
- **INV-0320-4.** The line gives the program the network only with a `+net` word, and the roots only with the network. *Check:* `make test-python` (`grants python3 ...` with and without `+net`).
- **INV-0320-5.** CPython's own tests of the listed modules pass in the guest. *Check:* `make test-python` (`regr.py`).

## Open questions
- Whether the roots should be loaded by `ssl.create_default_context()` without a `cafile` (a patch to `_ssl`'s default paths); today a program names the file.
- Whether the modules of `regr-modules.txt` can grow once `AF_UNIX` and server sockets exist.
- The Sigstore bundle of the archive: verification by `make fetch-ports` (needs a tool that is not on the host).

## Revisit triggers
- A larger initial stack for processes (patch 0004 goes), or `dlopen` (shared modules).
- A Python release that changes its stack guard, `getpath` or the `_ssl` socket code (patches 0001, 0004 and 0005).

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
