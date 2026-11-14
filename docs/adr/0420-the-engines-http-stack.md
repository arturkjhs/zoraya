# ADR-0420: The engine's HTTP stack: libsoup 3 over GIO with glib-networking's OpenSSL backend linked in

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0228, ADR-0310, ADR-0311, ADR-0312, ADR-0324, ADR-0328, ADR-0410, ADR-0411, ADR-0421
- **Evidence:** CP-28, DM-007, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#the-window-text-and-the-network, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WPE requires libsoup 3 (browser-engine/survey.md#wpe-webkit-in-detail). libsoup does TLS through GIO's
`GTlsBackend`, which on Linux is glib-networking loaded as a GIO module at run time `[outside evidence: GIO's module
loading; not fetched by the survey]`. WebKit also has a curl network backend, used by its PlayStation and Windows ports;
the PlayStation port's network layer is cURL (browser-engine/survey.md#how-other-systems-got-a-browser). Zoraya has
curl 8.23.0 and OpenSSL 3.5.9 ported, TLS verification that cannot be turned off (INV-0312-1), roots in
`/system/tls/roots` (ADR-0311), and no `dlopen` (browser-engine/survey.md#the-posix-library,
browser-engine/survey.md#the-window-text-and-the-network). The `web` grant admits public hosts on ports 80 and 443
(ADR-0324, ADR-0328).

## Decision drivers

1. **The network as a capability** (CP-28): the network process alone holds it (ADR-0411); names resolved by `netd`.
2. **No loading at run time** (DM-007, `dlopen` denied): every GIO module linked in and registered by code.
3. **One TLS library** (ADR-0312): OpenSSL, already reviewed, with verification forced on.
4. **The combination upstream builds**: WPE with libsoup.

## Considered options

### Option A: libsoup with glib-networking loaded as a module and GnuTLS (the Linux distribution way)

- **Description:** As GNOME ships it.
- **Evidence for:** No change.
- **Evidence against:** Module loading (driver 2); a second TLS library, GnuTLS (driver 3).
- **Cost:** Not available without `dlopen`.
- **Precedent:** Linux distributions.

### Option B: libsoup 3 with glib-networking's OpenSSL backend built static and registered at start

- **Description:** glib-networking built static with its OpenSSL backend; the network process registers the backend
  with GIO before its first request and sets a TLS database from `/system/tls/roots`; libsoup with libpsl and nghttp2,
  without proxy or GSSAPI support.
- **Evidence for:** Drivers 1 to 4.
- **Evidence against:** Two more LGPL ports (ADR-0421); whether glib-networking's static build registers cleanly is not
  verified.
- **Cost:** *(estimate)* Four ports (libsoup, glib-networking, libpsl, nghttp2), a 50-line registration patch.
- **Precedent:** Static GIO modules in GLib's own tests `[outside evidence: not fetched]`.

### Option C: WebKit's curl network backend in the WPE port

- **Description:** Build WPE with `USE_CURL` in place of libsoup, as the PlayStation and Windows ports do.
- **Evidence for:** Reuses the curl and OpenSSL ports and their tests; no glib-networking.
- **Evidence against:** WPE does not build with curl upstream: a combination nobody tests, carried by Zoraya alone
  (driver 4).
- **Cost:** *(estimate)* CMake and source patches across WebKit's network process.
- **Precedent:** WebKit on the PlayStation (a console system with a cURL network layer); Haiku's WebKit, which did
  its networking through Haiku's own API (browser-engine/survey.md#how-other-systems-got-a-browser).

## Recommendation

**Recommendation (not a decision): Option B**, with Option C as the fallback if glib-networking cannot be linked in.

1. **libsoup 3** with libpsl, nghttp2, brotli and zlib; SQLite for its cookie jar in the network process's directory
   (ADR-0411 item 2); no proxy resolver, no GSSAPI, no `SOUP_DEBUG` effect.
2. **glib-networking** with only the OpenSSL backend, static; a patch that refuses to turn verification off
   (`G_TLS_CERTIFICATE_*` flags ignored, as ADR-0312 patched curl), and a TLS database from `/system/tls/roots` only.
3. **Name resolution.** GIO's resolver calls `getaddrinfo`, which `libposix` sends to `netd` (ADR-0225), so the `web`
   grant's checks of answers apply (ADR-0324).
4. **What the person sees** when a host is refused: WebKit's error page with the refusal's wording of ADR-0325, made by
   the browser from the network process's error.

### Effect of the vision

For target 2 an agent's page reaches only what the `web` grant admits, through one process, with certificates checked
against the image's roots and no switch to skip them. Target 3: HTTP/2 and cookies as other browsers have them.
Networking: as ADR-0324. Threads: libsoup runs in the network process's threads.

### Examples

`https://expired.badssl.com/` shows WebKit's TLS error page; there is no "proceed anyway" that disables verification.

## Consequences

- **Positive.** The upstream combination; one TLS library.
- **Negative.** libsoup and glib-networking are LGPL (ADR-0421); a registration patch.
- **Follow-up.** The four ports; a test with the TLS test certificates of `tests/net/tls/`.

## Invariants

- **INV-0420-1.** No engine process completes a TLS handshake with a certificate that does not chain to
  `/system/tls/roots`, whatever the page or the configuration asks. *Check:* the engine's test step against
  `tools/test-httpd` with the expired and wrong-name certificates, asserting zero request bytes at the server.
- **INV-0420-2.** Only `WPENetworkProcess` opens TCP connections. *Check:* the same step reads `netd`'s connection list
  and asserts every connection's process.

## Open questions

- Whether glib-networking's OpenSSL backend links statically and registers without a module loader (the HTTP part
  finds out first, before libsoup).

## Revisit triggers

- WPE gains an upstream curl option, or drops libsoup.

## Architect's decision
