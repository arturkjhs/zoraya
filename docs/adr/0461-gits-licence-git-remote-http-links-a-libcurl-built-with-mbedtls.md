# ADR-0461: git's licence: git-remote-http links a libcurl built with mbedTLS

- **Status:** accepted
- **Date:** 2026-11-16
- **Depends on:** ADR-0228, ADR-0310, ADR-0312, ADR-0339, ADR-0347
- **Evidence:** CP-27, alternatives-postmortem.md#plan-9--inferno, stage-10/survey.md#git-licence-and-alternatives

## Context

`git` is GPL-2.0-only: its `COPYING` says that only version 2 is valid (`stage-10/survey.md#git-licence-and-alternatives`).
`git-remote-http` is a program of the git package that fetches and pushes over HTTPS, and it links libcurl. The curl port
of ADR-0310 is built against OpenSSL 3.5 (Apache-2.0, ADR-0312). The FSF holds GPL-2.0-only and Apache-2.0 not
combinable, so a static `git-remote-http` that links libcurl and OpenSSL cannot be shipped as built. The question was
recorded in ADR-0347's open questions and in `docs/architect-queue.md` section D, and `docs/dependencies.md` says git is
built and in no image until it is decided. ADR-0339 decided that git needs `+net` for a remote and that its HTTPS runs
through `git-remote-http`; part 10h wires git into the image.

What is known (checked in this session's sources, not measured): mbedTLS 3.6 LTS is dual-licensed Apache-2.0 OR
GPL-2.0-or-later; its GPL-2.0-or-later option can be combined with GPL-2.0-only code (the combination is GPL-2.0-only,
as git is). libcurl's licence is curl's own MIT-like one and can link either. `CLAUDE.md` allows a GPL program as a
separate program (GNU make, wget) and says an external crate or library has to be listed in `docs/dependencies.md`.
The rest of the system's native side uses rustls (`zoraya-tls`, ADR-0228), and the C side (curl, wget, OpenSSH) uses
OpenSSL 3.5.9 (ADR-0310, ADR-0312). **Not measured:** the size of `git-remote-http` with mbedTLS, or the speed of a TLS
handshake with it.

**The architect decided, on 2026-11-16, Option A below.** The ADR stays `proposed`: it is for the architect to accept,
and it must be done before part 10h wires git into the image.

## Decision drivers

1. git's licence is GPL-2.0-only and is not to be changed (it is the program's own; patches stay GPL-2.0-only, ADR-0347
   item 3).
2. `CLAUDE.md`: only permissively licensed libraries in the kernel and `no_std` crates; a GPL program is allowed as a
   separate program; an LGPL library may be linked statically into a program `ports/Makefile` builds. A GPL-licensed
   library linked into one GPL-2.0-only program of the ports is a different case, which this ADR decides.
3. TLS must verify certificates against the system's roots (ADR-0228, ADR-0310): no way to turn verification off, no other
   roots, no proxy (ADR-0339, ADR-0347 item 3's patches).
4. Scope stays small (CP-27): one library more for one program, not for the system.

## Considered options

### Option A: libcurl built with mbedTLS 3.6 LTS, for `git-remote-http` only (the architect's decision)

- **Description:** a second build of libcurl, with mbedTLS 3.6 LTS as its TLS backend, linked by `git-remote-http` and by
  nothing else. mbedTLS is used under its GPL-2.0-or-later option; git's combination is GPL-2.0-only. The certificate
  check is on, the roots are `/system/tls/roots` (as for the curl port, ADR-0310 items on roots), and curl's insecure
  options are removed as for the curl port itself (no `--insecure`, no CA path from the environment, no proxy: ADR-0310
  INV-0310-4; git's own `http.ssl*` settings are ignored by ADR-0347's patch 0003). `curl`, `wget`, OpenSSH and CPython
  keep OpenSSL; the native side keeps rustls.
- **Evidence for:** it keeps `git` and its HTTPS transport; the licences are compatible; it touches one program (CP-27).
- **Evidence against:** a second TLS library in the tree, to build and to keep patched for security; its size is added to
  `git-remote-http`; the licence of that one binary differs from the rest (GPL-2.0-only, through a GPL option of
  mbedTLS).
- **Cost:** *(estimate)* a port of mbedTLS 3.6 (its own build, its `config.h` set for TLS 1.2 and 1.3 client only), a
  second libcurl configure, a check for the roots and the insecure options, and the pins.
- **Precedent:** distributions that build git against GNU TLS or mbedTLS for licence reasons `[assumption]`: not checked in
  this session.

### Option B: `git-remote-http` as a program not linked to OpenSSL (a helper process)

- **Description:** `git-remote-http` does not link a TLS library; it starts `/bin/curl` or another process that holds the
  connection and passes it bytes (the pattern of `got`'s helpers, `stage-10/survey.md#git-licence-and-alternatives`).
- **Evidence for:** no second TLS library; each program keeps its licence.
- **Evidence against:** a different protocol between two programs, a patch of git's `remote-curl.c` bigger than ADR-0347's
  patches, and a program that must be granted two roots for one fetch.
- **Cost:** *(estimate)* high: a patch of git, a design of the helper's grants.
- **Precedent:** got (ISC), a different tool.

### Option C: no HTTPS transport (ssh only), or an OpenSSL exception

- **Description:** build git without `git-remote-http` and say HTTPS clone does not exist (ssh from part 10g stays), or ask
  the copyright holders of git for an exception to link OpenSSL (not available).
- **Evidence for:** no licence question at all; less code.
- **Evidence against:** `git clone https://` is what most people type; ADR-0339 decided on it.
- **Cost:** *(estimate)* none in code; a smaller feature.
- **Precedent:** Plan 9's tools used their own protocols, not the world's (`alternatives-postmortem.md#plan-9--inferno`);
  the same cost: the ecosystem's addresses do not work.

## Recommendation

**The architect's decision is Option A (2026-11-16).** The agent's analysis agrees: it is the smallest change that keeps
the program and the transport, and the licences are compatible. This is a proposal for the architect to accept before 10h.

## Consequences

- **Positive.** git clones and pushes over HTTPS with certificate verification against the system's roots; the rest of the
  system's TLS does not change.
- **Negative.** mbedTLS is a second TLS library for the ports (a second source to pin and to update when it has a
  security release), used under the GPL-2.0-or-later option and for git only: `docs/dependencies.md` says so.
- **Follow-up work.**
  - `ports/sources.lock`: the pin of mbedTLS 3.6 LTS, with its SHA-256, is made by the part that builds it (10h's part).
    Nothing is added to `sources.lock` by this ADR.
  - `docs/dependencies.md`: rows for mbedTLS (3.6 LTS; Apache-2.0 OR GPL-2.0-or-later, the GPL-2.0-or-later option used,
    for git only) and for a second libcurl build with mbedTLS; git's row says the licence question is decided by this ADR.
  - `docs/architect-queue.md`: the licence question of section D is removed (decided 2026-11-16).
  - ADR-0347: its open-question note on git's licence points to this ADR; "until decided git does not enter the image"
    holds until this ADR is accepted and 10h builds the transport.
  - The part that builds it adds the checks of the invariants below.

## Invariants

- **INV-0461-1.** `git-remote-http`'s link line names mbedTLS's libraries and not `libssl` or `libcrypto`, and no other
  program of the image links mbedTLS. *Check:* `ports/check-early.sh` by symbol (`nm`) on `git-remote-http` and on the
  other ports, in `make check-ports`, added by the part that builds it.
- **INV-0461-2.** `git-remote-http` refuses a certificate not made by a root of `/system/tls/roots`, and has no option,
  variable or configuration that turns verification off or changes the roots. *Check:* a test of the apps' kind
  (`make test-apps`) with the test PKI: an untrusted certificate is refused, as INV-0310-4 does for curl.
- **INV-0461-3.** `docs/dependencies.md` lists mbedTLS with the licence option used, and git's licence line. *Check:*
  `make check` (a script that every pinned port of `ports/sources.lock` has a row), added by the part that pins it.

## Open questions

- mbedTLS's size and a TLS 1.3 handshake's time next to OpenSSL's, once built.
- Whether the second libcurl is configured from the curl port's script with one switch or is a second script.
- Whether OpenSSH, which links OpenSSL for its own crypto (ADR-0340), is affected: it is BSD-licensed and not GPL.

## Revisit triggers

- git changes its licence, or OpenSSL's licence is declared compatible with GPL-2.0-only.
- A mbedTLS release with a licence change, or its 3.6 LTS line ending.
- A decision to move all C-side TLS to mbedTLS.

## Architect's decision

Accepted 2026-11-18. Decided by the architect; recorded by an agent at the architect's request.

Option A.
