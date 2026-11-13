# ADR-0312: OpenSSL 3 for the POSIX side of TLS, in place of BearSSL

- **Status:** accepted
- **Date:** 2026-11-13
- **Depends on:** ADR-0100, ADR-0109, ADR-0227, ADR-0228, ADR-0310
- **Evidence:** CP-27

## Context
ADR-0228 item 2 says: "POSIX: BearSSL as a port in the sysroot (MIT), configured with the time from `time()` and entropy
from `getrandom`, so that curl builds with `--with-bearssl`; Mbed TLS (under its Apache-2.0 option) is the port to add
when a program needs TLS 1.3, since BearSSL has none." TASK-27 (the architect's, after ADR-0228) names OpenSSL 3, static,
`no-shared`, `no-dso`, `threads` (musl pthreads in libposix; OpenSSL is rebuilt with them in 9a, ADR-0310 follow-up), `no-engine`, for curl and wget in 9a, CPython's `_ssl` in 9c and NetSurf's
libcurl in 9d. This ADR records the change so that ADR-0228 and the code agree; it does not touch ADR-0228's native side
(`zoraya-tls` over rustls), its roots, its names rule or its invariants.

Measured (9a): OpenSSL 3.5.9 cross-builds with `zcc` with seven configuration options and three small patches (ADR-0310),
links into curl and wget against 22 added names, and offers TLS 1.2 and 1.3.

## Decision drivers
1. The programs of stage 9 need TLS 1.3 (servers prefer it), HTTP clients that exist (curl, wget, CPython's `ssl`).
2. The fewer TLS libraries, the fewer trust stores and bugs (ADR-0228's own driver for one native crate; CP-27 asks the surface to stay small).
3. Only permissive licences (CLAUDE.md): OpenSSL 3 is Apache-2.0.

## Considered options
### Option A: OpenSSL 3.5 LTS for every C program (the recommendation)
### Option B: BearSSL for curl, Mbed TLS for what needs TLS 1.3 (ADR-0228 item 2 as written)
Two libraries and two sets of patches; wget has no BearSSL or Mbed TLS support; CPython's `ssl` module needs OpenSSL's API.
### Option C: rustls-ffi as the C library (the 2026-11-12 note of TASK-26's report)
One TLS implementation across native and C; curl supports it (experimental), wget and CPython do not.

## Recommendation
**Recommendation (not a decision): Option A.** The C TLS library is OpenSSL 3.5 (LTS), static, built as ADR-0310 item 1 says;
programs that need another API than OpenSSL's are a later question. ADR-0228 item 2 is superseded for its
choice of library only; item 3 (the roots) and every other item of ADR-0228 stand.

## Consequences
- **Positive.** One C library; TLS 1.3; the library curl, wget, CPython and NetSurf all support.
- **Negative.** A large dependency (about 28 MB of archive, 6.7 MB linked into each program); two TLS implementations
  now exist in the system (rustls for native programs, OpenSSL for C), each with its own verifier reading the same
  roots; a vulnerability in either needs its own update.
- **Follow-up.** `docs/dependencies.md` lists OpenSSL and zlib as ported libraries; OpenSSL is built with threads (ADR-0310 follow-up).

## Invariants
- **INV-0312-1.** A C program built on this OpenSSL verifies the chain against `/system/tls/roots`, the time of the system's clock and the name it connected to, and sends no request byte after a failed verification. *Check:* `make test-apps` (expired and wrong-name servers: the server's log shows `ended after 0 request bytes`).

## Open questions
- Whether native programs' rustls and C programs' OpenSSL should one day share a verifier (rustls-ffi); not now.

## Revisit triggers
- OpenSSL 3.5's end of support; a program that needs a TLS API OpenSSL lacks.

## Architect's decision
Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.

Option A. OpenSSL 3.5 LTS, static, built per ADR-0310 item 1. ADR-0228 item 2 is superseded for the choice of library only; item 3 (the roots) and the rest of ADR-0228 stand.
