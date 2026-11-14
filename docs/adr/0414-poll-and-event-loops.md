# ADR-0414: `poll` and event loops: `poll` and `ppoll` over every descriptor kind, an in-process `eventfd`, no `epoll`

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0006, ADR-0101, ADR-0225, ADR-0300, ADR-0302, ADR-0315, ADR-0342, ADR-0410, ADR-0412, ADR-0413
- **Evidence:** CP-08, CP-10, DM-023, DM-026, DM-130, X-4, browser-engine/survey.md#ladybird, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#measurements, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#wpe-webkit-in-detail

## Context

WebKit's run loop iterates a `GMainContext` by hand and calls the context's poll function, which is GLib's `g_poll`,
`poll()` where available (browser-engine/survey.md#wpe-webkit-in-detail, browser-engine/survey.md#libraries-and-interfaces).
GLib's wake-up uses `eventfd` where present (else a pipe); WebKit's repeating timers use a `timerfd` under
`HAVE(TIMERFD)`; its `IPC::Semaphore` on Unix is an `eventfd` (browser-engine/survey.md#measurements). Ladybird waits
with `ppoll` on Linux and `poll` elsewhere and wakes itself with a pipe (browser-engine/survey.md#ladybird). libcurl
waits with its own poll or reports sockets to the caller's loop. Neither engine's code uses `epoll` or `kqueue` for its
loop: the one `epoll_` in WebKit is a seccomp table; Ladybird's are a Windows loop and a sandbox list.

Zoraya has `pselect` (streams and sockets wait; files and terminals report ready) and ADR-0342 item 5 has admitted
`poll`, `ppoll` and `select` as the same wait, not yet built; `epoll_*` is on the deny list; there is no `eventfd` or
`timerfd` (browser-engine/survey.md#the-posix-library). In a process with threads every long wait releases the library
lock and waits on a port made for it (ADR-0315).

## Decision drivers

1. **One wait model** (CP-08): the port under every readiness call, as ADR-0225 item 8 decided for sockets.
2. **Readiness for POSIX, without importing epoll** (X-4, DM-023): epoll's descriptor-versus-description semantics are
   a recorded mistake; the engines do not need it. Whether readiness or completion is the better native model is
   contested (DM-026); this ADR decides only the POSIX side.
3. **Waitable events in the descriptor model** (DM-130): a wake-up the loop can poll.
4. **No asynchronous interruption** (CP-10): a wake-up is a packet, not a signal.
5. **Cost per wait** in a threaded Web process (ADR-0315's port per long wait).

## Considered options

### Option A: Linux's set: `epoll`, `eventfd`, `timerfd`, `signalfd`

- **Description:** All four, as Linux has them (browser-engine/survey.md#libraries-and-interfaces).
- **Evidence for:** GLib and WebKit would take their Linux paths unchanged.
- **Evidence against:** DM-023's semantics, DM-130's retrofit list; neither engine needs `epoll`; `signalfd` against
  ADR-0106's signals at wait points.
- **Cost:** *(estimate)* 1,200 lines; a persistent interest set per epoll descriptor over ports.
- **Precedent:** Linux.

### Option B: `poll` and `ppoll` over every descriptor kind; `eventfd` within a process; no `epoll`, `kqueue`, `timerfd`

- **Description:** ADR-0342 item 5's `poll` and `ppoll` cover streams, TCP and Unix sockets, listeners, memory
  descriptors, the terminal and `eventfd`. `eventfd` is a library counter whose readiness wakes the waiting threads
  with a user packet on their wait ports. GLib and WebKit are built without `timerfd` (`HAVE(TIMERFD)` false).
- **Evidence for:** Drivers 1 to 4; what both engines and curl call; GLib needs only `poll` (the survey's reading of
  `gpoll.c`).
- **Evidence against:** An `eventfd` cannot be passed to another process (WebKit's `IPC::Semaphore` then cannot work,
  but it serves the GPU process's stream connections, which are off, ADR-0410 item 2); `poll` builds its subscriptions
  per call (`O(n)` per wait).
- **Cost:** *(estimate)* 500 lines in `libposix` and host tests beyond ADR-0342's `poll`.
- **Precedent:** Fuchsia's waits on many handles and its ports (browser-engine/survey.md#libraries-and-interfaces),
  under its POSIX library's `poll` `[outside evidence: fdio's source, not fetched]`.

### Option C: A native event-loop backend for GLib (a `GMainContext` poll function over ports)

- **Description:** Replace GLib's `g_poll` with a Zoraya function that waits on the handles behind the descriptors, as
  GLib's Windows path waits with `WaitForMultipleObjects` (browser-engine/survey.md#libraries-and-interfaces).
- **Evidence for:** One port per loop kept across iterations, no per-call subscription.
- **Evidence against:** A patch to GLib and every other loop (Ladybird, curl, CPython) still wants `poll`; the cost it
  saves is not measured.
- **Cost:** *(estimate)* 400 lines in GLib.
- **Precedent:** GLib on Windows.

## Recommendation

**Recommendation (not a decision): Option B.**

1. **`poll` and `ppoll`** as ADR-0342 item 5 (one wait with `pselect`; INV-0342-2). Readiness by kind: stream and TCP
   socket as `pselect` today; Unix socket the channel's readable (`POLLIN`), writable (`POLLOUT`) and peer-closed
   (`POLLHUP`, with `POLLIN` while messages remain); a listener readable when a connection waits; a memory descriptor,
   a regular file and `/dev/null` always ready; a terminal by ADR-0331's readiness once built; a closed or unknown
   descriptor `POLLNVAL`. `ppoll`'s signal mask is the ADR-0106 mask for the wait.
2. **`eventfd(initval, flags)`** with `EFD_CLOEXEC`, `EFD_NONBLOCK`, `EFD_SEMAPHORE`: an 8-byte counter in the library.
   A `write` adds (blocks or `EAGAIN` at `0xfffffffffffffffe`); a `read` takes the counter (or 1 with
   `EFD_SEMAPHORE`), blocking at 0; readiness is a non-zero counter. A write that makes the counter non-zero queues a
   user packet (`port_queue`) on the port of every thread of the process waiting on that descriptor (ADR-0315's thread
   table), and a single-threaded process never waits on it while writing. Sending one with `SCM_RIGHTS` or listing it in
   a spawn is `EINVAL` (ADR-0412 item 3); `fork` gives the child a copy of the counter.
3. **Not offered.** `epoll_*` stays denied; `kqueue`, `timerfd_*` and `signalfd` are not exported; a port that probes
   for them is configured without them (GLib's `HAVE_EVENTFD` on, WebKit's `HAVE(TIMERFD)` off).
4. **Cost of a wait.** The first engine part measures the time of an idle Web process's loop iteration and of a
   `poll` over 64 descriptors in a threaded process under HVF. If ADR-0315's port per wait exceeds 10 % of an iteration,
   a reused port per thread is proposed as a change of ADR-0315 item 1, with that number.

### Effect of the vision

For target 2, waiting gives a program no new authority: `eventfd` wakes threads of its own process only. Target 3: the
engine's loops run as written. Networking: TCP and Unix sockets wait the same way. Threads: wake-ups are packets on the
waiting thread's port, never an interruption (CP-10).

### Examples

`strace`-like output of the engine's debug log, as the first engine part records it: `poll([{fd=7, POLLIN}, {fd=9,
POLLIN}], 2, 16)` where 7 is the IPC socket and 9 GLib's `eventfd`; a page's `setTimeout(f, 16)` becomes the timeout.

## Consequences

- **Positive.** GLib, WebKit, Ladybird, curl and CPython's `selectors` share one implementation.
- **Negative.** No cross-process `eventfd`; `poll` is linear in its descriptors; a GLib built with a configuration
  Zoraya fixes.
- **Follow-up.** `poll`/`ppoll` per ADR-0342, `eventfd`, the measurement of item 4.

## Invariants

- **INV-0414-1.** For every descriptor kind, `poll` reports `POLLIN` exactly when a non-blocking read would not fail
  with `EAGAIN`, and `POLLOUT` exactly when a non-blocking write of one byte (or message) would not. *Check:* `cargo test
  -p zoraya-posix` (each kind against the fake kernel) and an in-QEMU C test over pipes, TCP and Unix sockets, memory
  descriptors and `eventfd`.
- **INV-0414-2.** A thread waiting in `poll` on an `eventfd` wakes after another thread's `write` within one wait of
  the scheduler, and no write is lost. *Check:* an in-QEMU threaded C test with 10,000 writes and a counter check.
- **INV-0414-3.** `libc.a` exports no `epoll_*`, `kqueue`, `timerfd_*` or `signalfd`. *Check:* `posix-surface-check`'s
  deny list (ADR-0342 item 12).

## Open questions

- Whether GLib's configure, with `eventfd` present, needs `pipe2` or `F_DUPFD_CLOEXEC` paths Zoraya lacks; the GLib
  port finds out.
- Whether WebKit's network process uses libsoup's own timeouts that expect `timerfd`.

## Revisit triggers

- A port that cannot be built without `epoll` (a server under real load): an ADR on a persistent interest set.
- The GPU process or WebKit's stream connections are wanted: a cross-process event (a kernel object), its own ADR.

## Architect's decision
