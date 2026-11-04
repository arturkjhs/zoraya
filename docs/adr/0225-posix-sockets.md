# ADR-0225: POSIX sockets in `libposix`: the network tier, mapped onto the scoped connection

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0002, ADR-0005, ADR-0006, ADR-0100, ADR-0101, ADR-0103, ADR-0106, ADR-0107, ADR-0220, ADR-0222, ADR-0223, ADR-0224
- **Evidence:** CP-27, CP-28, DM-074, DM-075, DM-077, GR-008, X-1, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, network/survey.md#capsicum-and-casper, network/survey.md#fuchsias-socket-provider, network/survey.md#name-resolution-as-authority, network/survey.md#posix-sockets-what-curl-and-wget-call, task-02/libc-surface.csv

## Context

ADR-0005 excluded "Sockets and name resolution (`socket`, `getaddrinfo`)" from the POSIX scope with the reason "CP-28
covers the native design later", put `socket`, `getaddrinfo`, `poll` and `select` outside the tier list, and listed
`socket` and `getaddrinfo` on INV-0005-2's deny list, with the revisit trigger "Pressure to add sockets for fetching
sources: CP-28's native design first, then a personality mapping by ADR". ADR-0220 to ADR-0223 are that native
design. ADR-0100 replaced musl's `src/network` with Rust, so every socket function is `libposix`'s to write. This ADR
decides which socket calls `libposix` implements, mapped onto the scoped connection, what `socket()` returns without a
grant, and `getaddrinfo`.

**What is measured.** The socket calls curl and wget use, counted at their call sites
(`network/survey.md#posix-sockets-what-curl-and-wget-call`): both are stream clients with `socket`, `connect`,
`getaddrinfo`, `send`/`recv`, `select` or `poll`, `fcntl` for `O_NONBLOCK`, `setsockopt`; `bind`/`listen`/`accept` only
for FTP's active mode; `sendto`/`recvfrom` only in curl's UDP paths; no `getnameinfo`. In `task-02/libc-surface.csv`
every socket row is `T3` or `unused-applets`: no T0 to T2 workload used the network, and the toybox and busybox
applets that would (`wget`, `nc`, `ping`) were not run. musl's resolver reads `/etc/resolv.conf` and `/etc/hosts` and
speaks UDP with a TCP fallback (`network/survey.md#name-resolution-as-authority`), which is why Capsicum's tcpdump lost DNS (GR-008).

**What is assumed.** That curl built with BearSSL (ADR-0228) is the first POSIX network program, wget the second, and
`git` over HTTPS (libcurl) the third; that none needs raw sockets, `getnameinfo` or `SO_LINGER`.

## Decision drivers

1. **Programs run unmodified** (CP-27's purpose, X-1): curl and wget as built, with `sockaddr` as C expects (DM-074,
   CP-28's stated cost).
2. **The grant is the native scope** (ADR-0220): no second policy in the library; `libposix` holds the `NET` slot and
   nothing else.
3. **Resolution stays in `netd`** (ADR-0223): `getaddrinfo` is a request, never a UDP socket from the library (DM-075).
4. **Readiness stays on the port** (ADR-0006, ADR-0106 item 3): `poll` and `select` on sockets are `pselect`'s mapping.
5. **A refusal names the host** (ADR-0224 rule 5): the call that fails must be the one that knows the destination.
6. **The surface is a list that grows only by ADR** (CP-27, INV-0005-1).

## Considered options

### Option A: musl's `src/network` kept, over a raw datagram and stream capability (the Unix baseline in the library)
- **Description:** Keep musl's socket and resolver C, give `libposix` a "raw" capability to make any connection, and
  let the library do what Linux's libc does.
- **Evidence for:** No mapping to write; the resolver and `getaddrinfo` as every C program expects.
- **Evidence against:** ADR-0100 replaced that directory for the same reason it replaced `unistd`: musl's code calls
  Linux system calls with `sockaddr` and integers, and mapping them one by one is the same work as writing the
  functions; the resolver would need UDP to port 53 (driver 3 fails) and `/etc/resolv.conf` in the table (ADR-0223).
- **Cost:** *(estimate)* Smallest if the native API were Linux's; it is not.
- **Precedent:** Unix.

### Option B: Sockets through the broker (Starnix's shape: a personality server on the socket path)
- **Description:** `socket`, `connect`, `send` and `recv` are requests to the POSIX broker, which holds the network.
- **Evidence for:** One place implements POSIX socket semantics; the library stays thin.
- **Evidence against:** Every byte crosses the broker (ADR-0002 INV-0002-3's reason: the data path stays out of it);
  Starnix's performance warning (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); the broker would need the
  grant, which is per command, not per session.
- **Cost:** *(estimate)* A second protocol and a hop per transfer.
- **Precedent:** Starnix.

### Option C: Rust socket functions in `libposix` over the scoped connection, with a network tier of rows (Fuchsia's fdio over the socket provider; QNX's libc over resource managers)
- **Description:** Below. Fuchsia's POSIX components link `fdio`, which turns libc calls into the socket provider's
  protocols (`network/survey.md#fuchsias-socket-provider`); QNX's libc maps POSIX onto messages
  (`alternatives-postmortem.md#qnx`).
- **Evidence for:** Drivers 1 to 6; it is ADR-0100's shape (the machine-facing half in Rust on native handles) and
  ADR-0103's (an fd is library state over handles). Fuchsia ships it.
- **Evidence against:** A socket fd is three handles, which `spawn` inheritance and `fork` must carry (ADR-0103's
  `share` problem again); some `setsockopt` options can only be accepted and ignored; `getaddrinfo`'s service names
  need a built-in table.
- **Cost:** *(estimate)* 2,000 lines in `zoraya-posix` with host tests against `zoraya-net`'s logic through the fake
  kernel; the rows below.
- **Precedent:** Fuchsia's fdio (shipping), QNX.

## Recommendation

**Recommendation (not a decision): Option C.**

1. **The network tier, TN.** Rows added to the surface by this ADR, each with the workload that justifies it (W11: a
   fetch of a page over HTTP from the test server with curl; W12: the same over HTTPS; W13: wget; each defined in
   ADR-0230): `socket`, `socketpair`, `connect`, `bind`, `listen`, `accept`, `accept4`, `shutdown`, `send`, `sendto`,
   `sendmsg`, `recv`, `recvfrom`, `recvmsg`, `getsockopt`, `setsockopt`, `getsockname`, `getpeername`, `getaddrinfo`,
   `freeaddrinfo`, `gai_strerror`, `gethostbyname` (over `getaddrinfo`), `getservbyname`, `poll`, `select`,
   `inet_pton`, `inet_ntop`, `inet_aton`, `inet_ntoa`, `inet_addr`, `htons`, `htonl`, `ntohs`, `ntohl`,
   `if_nametoindex`, `getnameinfo` (numeric form only: `NI_NUMERICHOST` and numeric services succeed, a reverse lookup
   returns `EAI_FAIL`, since ADR-0223 offers none), `getrandom` (ADR-0227). Not offered, with the reason:
   `gethostbyaddr` (reverse lookup), `getifaddrs`, raw sockets
   (`SOCK_RAW` gives `EPROTONOSUPPORT`), `AF_UNIX` sockets in the filesystem (`EAFNOSUPPORT`; only `socketpair`),
   `recvmmsg`/`sendmmsg`. INV-0005-2's deny list loses `socket` and `getaddrinfo`, and `poll` is no longer "general
   multiplexer out of scope" (ADR-0005's table) for fds the port can wait on; `epoll_*` stay denied.
2. **The fd kind `Socket`** (ADR-0103's table): `{ family, type, state, ctl: channel end, read_end, write_end }` for
   a stream socket; `{ ctl }` for a datagram socket or a listener. `socket()` makes the library object only and needs
   no grant: it succeeds whether or not the process holds `NET`. The first call that names a destination or a port
   consults `NET`: without the slot, or with `NotGranted` from `netd`, it fails with `EACCES` and writes the report of
   ADR-0224 rule 5. (The alternative, failing at `socket()`, was rejected because the report could not name a host,
   which is the whole point of the hint; Capsicum chose the same: `socket` succeeds and `connect` fails,
   `network/survey.md#capsicum-and-casper`.)
3. **`connect(fd, sockaddr)`.** The library keeps, per process, the last `getaddrinfo` results by address (bounded,
   `[assumption]` 64 entries); a `connect` to an address that came from a resolution of name `N` is sent to `netd` as
   `connect(Name(N), port)`, so a scope of names works for the usual `getaddrinfo`-then-`connect` sequence (ADR-0223
   item 3), and `netd` reconnects by the name; a `connect` to an address that came from nowhere is `connect(Address)`.
   The request is answered at once with the scope's verdict (ADR-0222 item 3); a blocking `connect` then waits on the
   connection channel for `connected` or `failed` (a wait point, ADR-0106); a non-blocking one returns `EINPROGRESS`,
   and `poll`'s `POLLOUT` on that fd is the connection channel's `READABLE` until `connected` arrives, after which it
   is the write end's `WRITABLE`; `SO_ERROR` reports a `failed`. Statuses: `NotGranted` → `EACCES` (+ report),
   `Unresolvable` → `EHOSTUNREACH` (+ report when its reason is `Local`), `Refused` → `ECONNREFUSED`, `Unreachable` →
   `ENETUNREACH`, `TimedOut` → `ETIMEDOUT`; a scope channel that shows `PEER_CLOSED` (the stack was restarted,
   ADR-0221 item 5) → `ENETDOWN`.
4. **Data.** `read`, `write`, `send`, `recv`, `sendmsg`, `recvmsg` on a stream socket are `stream_read` and
   `stream_write` with ADR-0101's blocking loop; `O_NONBLOCK` gives `EAGAIN`; `MSG_DONTWAIT` likewise per call;
   `MSG_PEEK` is `EOPNOTSUPP`. End of file is `stream_read` returning 0; `PeerClosed` on write is `EPIPE`, with
   `SIGPIPE` by ADR-0106 item 8 unless `MSG_NOSIGNAL` (DM-077). A `reset` message on `ctl`, or `ctl` showing
   `PEER_CLOSED` with no `reset` (a dead `netd`, ADR-0221 item 5), becomes `ECONNRESET` on the next call. A datagram
   longer than `DATAGRAM_MAX` is `EMSGSIZE`. `shutdown(SHUT_WR)` closes the write end; `SHUT_RD` the read end. `sendto`/`recvfrom` on a datagram
   socket are `send` and `received` messages (ADR-0222); an unbound datagram socket is bound to an ephemeral port on
   its first `sendto`.
5. **`bind`, `listen`, `accept`.** `bind` records the address and port; for a datagram socket it is `datagram(port)`;
   `listen` is `listen(port)` with the recorded port (a port of 0 asks `netd` for an ephemeral one, reported by
   `getsockname`); `accept` reads the listener channel, blocking on `READABLE`, and makes a `Socket` fd from the
   message's three handles; `accept4`'s `SOCK_NONBLOCK` and `SOCK_CLOEXEC` set the fd's flags.
6. **`getaddrinfo`.** Numeric hosts (`AI_NUMERICHOST` or a literal) and `AI_PASSIVE` with no node are answered in the
   library; otherwise `resolve(name)` on `NET`, with the results remembered for item 3. Service names are a built-in
   table (`http`, `https`, `ftp`, `ssh`, `domain`, `ntp`, `smtp`, `imap`, `pop3`, `[assumption]` the set) or a number.
   Errors: `NotGranted` → `EAI_SYSTEM` with `errno` `EACCES` (+ report); `Unresolvable` → `EAI_NONAME`; a transient
   failure → `EAI_AGAIN`; no `NET` slot → `EAI_SYSTEM` with `EACCES`. `/etc/hosts` and `/etc/resolv.conf` are never
   read: local names are `netd`'s (ADR-0223 item 5). `gethostbyname` is `getaddrinfo` with `AF_INET` into a static
   `hostent`.
7. **Options.** `setsockopt`: `TCP_NODELAY` and `SO_KEEPALIVE` are control messages; `SO_REUSEADDR` and `SO_REUSEPORT`
   are accepted and change nothing (one program per port anyway); `SO_RCVBUF`/`SO_SNDBUF` are accepted and reported
   as the ring's 64 KiB; `SO_ERROR` reads the pending error; `SO_RCVTIMEO`/`SO_SNDTIMEO` are library deadlines;
   `IPV6_V6ONLY` accepted; anything else `ENOPROTOOPT`. `getsockname` and `getpeername` are `local()` and `peer()`.
8. **Readiness.** `poll`, `select` and `pselect` over any mix of stream fds, socket fds, listeners, datagram sockets
   and the terminal are one `port_wait` (ADR-0106 item 3's subscriptions, plus `READABLE | PEER_CLOSED` on listener
   and datagram channels); regular files report ready at once, as today.
9. **Inheritance.** A `Socket` fd in `spawn` puts its read end at the slot and its write end and a *shared* `ctl`
   (a second control channel from `share`, ADR-0222, so the parent keeps its own: channels have no `DUPLICATE`,
   ADR-0050) among the bootstrap record's handles with indices (the `Tty` kind's pattern, ADR-0107 item 3); a
   listener fd is inherited as a `share`d listener channel, its pending connections staying with the parent; a
   datagram socket likewise, its queued datagrams staying. `fork` duplicates the stream ends and carries a `share`d
   `ctl` per socket in the move list (ADR-0103 item 8's mechanism for channels). `FD_CLOEXEC` as for any fd.
10. **`socketpair(AF_UNIX, SOCK_STREAM)`** is two byte streams crossed, no `netd`: each fd holds one read end and one
    write end; `AF_UNIX` with a path is `EAFNOSUPPORT`.
11. **Errors to `errno`.** The new rows of ADR-0100's table: `NotGranted` → `EACCES`, `Unresolvable` → `EHOSTUNREACH`,
    `Refused` → `ECONNREFUSED`, `Unreachable` → `ENETUNREACH`, `TimedOut` → `ETIMEDOUT`; and two conditions that are
    not statuses: a scope channel with `PEER_CLOSED` → `ENETDOWN`, a control channel with `PEER_CLOSED` and no
    `reset` → `ECONNRESET`.

**Examples.** What a C program's user sees is ADR-0224's; what the program sees:

```c
int fd = socket(AF_INET, SOCK_STREAM, 0);          /* succeeds, grant or not */
getaddrinfo("example.org", "443", &hints, &res);  /* EAI_SYSTEM, errno EACCES without +net example.org */
connect(fd, res->ai_addr, res->ai_addrlen);       /* netd: connect(Name("example.org"), 443) */
```

**Tensions with accepted ADRs.**
- *ADR-0005*: its scope table's socket row and its `poll`/`select` row are reopened, under its own revisit trigger;
  INV-0005-1's generator rule gains the TN rows with their workloads (W11 to W13), and INV-0005-2's deny list loses
  `socket` and `getaddrinfo`. A superseding note in ADR-0005 is needed if this is accepted.
- *ADR-0100 item 9*'s `errno` table gains item 11's rows; its "`src/network` is on the replaced list" is carried out.
- *ADR-0103*'s fd kinds gain `Socket` with three handles; its inheritance rule (slot = fd number) is extended as the
  `Tty` kind extended it.
- *ADR-0107 item 3*'s record gains the `Socket` kind and the `NET` slot (ADR-0220).
- *ADR-0106 item 8* (`SIGPIPE` on `PeerClosed`) applies to sockets as it foresaw.

### Effect of the vision
For target 2 nothing in the library is trusted: the scope is checked in `netd`, and a hostile program that edits its
`getaddrinfo` cache gains nothing (a `connect(Name)` is still checked by name, a `connect(Address)` by address).
Target 3: desktop programs are native and use ADR-0222's crate. Threads: the fd table gains a lock, as ADR-0100 said.

## Consequences
- **Positive.** curl and wget as built; one policy; `select` and `poll` on sockets for free from stage 6d's port
  mapping; a refusal with a hint.
- **Negative.** Three handles per socket fd; a few options accepted and ignored; `getaddrinfo` cannot resolve what the
  grant does not cover, which programs report as a resolver failure until the hint explains.
- **Follow-up.** The TN rows in `docs/adr/posix-surface-T1.txt`'s generator (a `TN` column or file); the socket module
  of `zoraya-posix` with host tests through the fake kernel against `zoraya-net`; curl's port with BearSSL
  (ADR-0228); W11 to W13 (ADR-0230).

## Invariants
- **INV-0225-1.** `socket()` succeeds without a grant, and the first `connect`, `bind`, `sendto` or `getaddrinfo`
  without one fails with `EACCES` (or `EAI_SYSTEM`/`EACCES`) and produces exactly one report per target and port.
  *Check:* an in-QEMU `libposix` test with and without `+net`, under the report-reading launcher.
- **INV-0225-2.** A `connect` to an address returned by `getaddrinfo` for a name is sent to `netd` as a connection to
  that name. *Check:* a host test of `zoraya-posix` under a counting fake `netd` that records request kinds.
- **INV-0225-3.** `libposix` opens no UDP socket and reads no file for resolution: `getaddrinfo` makes exactly one
  `resolve` request or none. *Check:* the counting fake `netd` and INV-0002-3's file-only counter.
- **INV-0225-4.** `poll` and `select` over a socket, a pipe and the terminal are one `port_wait`, and a readable
  socket wakes them. *Check:* an in-QEMU test with a peer that writes after a delay.
- **INV-0225-5.** Every TN symbol `libc.a` exports is in the tier list with a workload, `gethostbyaddr` and
  `getifaddrs` are not exported, and `getnameinfo` returns `EAI_FAIL` for any non-numeric request. *Check:*
  INV-0100-1's tool with the TN rows added, and a host test of `getnameinfo`.

## Open questions
- Whether `connect` should carry the name only when the address came from the *same* process's resolution, or
  whether `netd` should accept `connect(Address)` under a name rule when the address is in its cache for that name
  (looser, simpler for the program, weaker for the scope).
- The service-name table and whether `/etc/services` should be a setting instead.
- How many addresses to try on `connect(Name)` and in which order (Happy Eyeballs is DM-074's example of what an
  application should not have to do).

## Revisit triggers
- A program in scope using `getnameinfo`, raw sockets, filesystem `AF_UNIX` sockets or `MSG_PEEK`.
- A measured cost of the three handles per socket in the broker's or `netd`'s tables.
- `git` over HTTPS or `ssh` in scope: their calls are checked against the tier.

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
