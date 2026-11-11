# ADR-0286: What N3 decided where the network ADRs are silent: the shape of `connect` with the client's streams, the copy loop, the resolver, a `url` operand that designates, and a test server

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0101, ADR-0220, ADR-0222, ADR-0223, ADR-0224, ADR-0281
- **Evidence:** CP-08, CP-11, CP-14, network/survey.md#smoltcp, network/survey.md#fuchsias-socket-provider, network/survey.md#qemu-networking-for-development-and-ci

## Context

N3 of TASK-26 is the first program that uses the network: `fetch http://HOST/PATH` makes a TCP connection through `netd`
and prints a page. smoltcp's socket buffers are the caller's slices (`network/survey.md#smoltcp`), and a Zircon socket beside a FIDL
channel is the shape of a stack that is a component (`network/survey.md#fuchsias-socket-provider`); SLIRP's alias of the
host is what makes a server on the host reachable (`network/survey.md#qemu-networking-for-development-and-ci`). The
accepted ADRs fix the capability (ADR-0220), the protocol's shape (ADR-0222: stream ends and a
control channel, with its note of 2026-11-10 that the client makes the streams), the resolver (ADR-0223) and the shell's
words (ADR-0224). The code needed choices that none of them makes; each is small and none is against an ADR. They are
listed so that a reviewer finds them in one place (CLAUDE.md, "Decisions"). **Measured** (`docs/design.md`, "Network
service", N3): the numbers of a 16 MiB fetch. **Assumed:** QEMU's user network is the only network (ADR-0230), one
interface, IPv4.

## Decision drivers

1. Every byte a server or a DNS server sends is parsed by a total, bounded decoder with hostile-input tests (ADR-0052's
   discipline, INV-0223-4; CP-11: typed through a declaration, no untyped call).
2. A connection's two rings are paid by the client (INV-0262-4, ADR-0222's note; CP-14: buffers bounded and committed
   at creation).
3. One slow connection must not hold the single loop of `netd`, and a ready stream that nobody waits for must not wake it
   (the spin the N2 review found in `poll`; CP-08: one wait on a port).
4. The tests need no Internet and no privileges.

## Considered options

### Option A: the choices below (the recommendation)

### Option B: `netd` makes the streams and returns them (ADR-0222's original text)

The client's room does not pay for the rings, which ADR-0262 and the architect's note of 2026-11-10 reject.

### Option C: a socket-like blocking call per read and write on the connection channel (Plan 9's `data`)

ADR-0222 option B: rejected there (every byte a message, `pselect` cannot wait on it).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **`connect` as built** (`zoraya-proto-net`, ordinal 3): `connect { address, port, name }` with three handles, in
   order the server end of the connection channel, the write end of the stream the client reads, the read end of the stream
   the client writes. `name` is a host name of at most 253 bytes, or empty to use `address` (not zero). `netd` checks the
   right `reach`, the scope (a name needs a name rule, an address an address rule: INV-0223-2) and the handle types
   before it does anything, replies `Connecting { status }` on the scope channel at once, then sends `Connected {
   address, port }` or `Failed { reason }` on the connection channel, and `Reset { why }` if an open connection ends
   badly. `Failed` and `Reset` are the last message; the stream ends `netd` holds are closed after them (INV-0286-1).
   The reasons: `Unresolvable`, `Refused`, `Unreachable`, `TimedOut`, `Local`, `OverLimit` (the room for ADR-0262's
   `OverQuota`, until accounts). Not built: `peer`, `local`, `keep_alive`, `no_delay`, `share`, `datagram`, `listen`.
   `shutdown(write)` is the client closing the write end it holds: `netd` reads the ring dry, sends a FIN, and the peer's
   FIN is end of file on the other stream after the data.
2. **The copy loop** (`zoraya_net::conn`). smoltcp's `send` and `recv` closures give the free space and the readable
   bytes; the stream calls (`stream_read` into the free space, `stream_write` from the readable bytes) are made inside
   them, so a byte is copied once in `netd` and never staged. Backpressure is the rings' and the sockets': a full inbound
   ring stops reading the socket (the advertised window closes), an empty outbound ring or a full send buffer stops
   sending. Each turn does at most 16 rounds per direction per connection. `netd` subscribes a stream end only while the
   connection waits for it (outbound readable when the ring was empty, inbound writable when it was full), so nothing
   spins. Socket buffers are 32 KiB each, at most 8 sockets and 16 connections (a bound until ADR-0056's accounts).
   One turn is: frames in, the resolver, the connections, frames out (`Conns::turn`).
3. **A reset delivers what arrived first.** smoltcp keeps the receive buffer when a reset closes the socket, so the
   client reads the data, and then `Reset { Peer }` and end of file. `Reset { TimedOut }` is declared and not produced:
   smoltcp does not say whether a timeout or a reset closed the socket.
4. **The resolver** (`zoraya_net::dns`, `zoraya-dns`): `net.hosts` first (exempt from the local rule), a cache of 64
   names kept for the answer's TTL up to an hour (the entry nearest its expiry goes), then A queries over one UDP socket
   to the servers of `status`, one query per connection that needs the name (two connections to one name send two), attempts at 0, 1, 2 and 4 s to the servers in turn, giving
   up at 8 s, at most 8 queries at once. An answer counts only from a configured server's port 53, with the identifier
   and the question of a query in flight; the decoder takes an address only from a record whose owner is the name asked
   or a CNAME chain from it. The identifier and the source port come from a generator seeded with the time and the MAC,
   which an off-path sender can guess: ADR-0227's service replaces it (N5). Not built: AAAA, the TCP fallback for a
   truncated answer (it is `Unresolvable`), negative caching, a `resolve` request, search domains (ADR-0223 has none).
   The local-address rule (loopback, link-local, multicast, unspecified, the interface's address, the gateway, a DNS
   server) is applied to every answer and refuses with `Local` unless an address rule covers it.
5. **A literal `url` operand designates** (`zoraya-describe` `Kind::Url`, the word `url`; ADR-0224 item 2). For a
   base-system command whose description has such an operand, `zosh` makes the command's network connection with
   `narrow`: right `reach`, one rule for that host (a name, or an address if it is dotted decimal) and port, TCP. This is
   `zoraya_shell::netgrant::net_for`, host-tested. As ADR-0224 item 2 says, only a *literal* operand designates: one that
   came from a variable, a substitution or a glob is refused before the line runs (`zosh: URL: from a value, outside
   the line's network grants`), because data would choose what the command reaches; ADR-0224 lets such an operand
   designate inside a `+net` rule on the line or in the room, and those words are **not built**, so a value is always
   refused until N3b. A URL that does not parse gives no connection and the command says what is wrong with it. Not built
   either: the `+net` and `+listen` words, `grants` output for the scope, the REPORT message and the hint. Exposure
   today: rooms and the panel's shells have no `NET` slot, `--net` refuses `--gui`, so only the console shell, which
   holds the root connection, designates, and only for a URL written on its line.
6. **`fetch` and the HTTP client** (`zoraya-http`): GET only, `Connection: close`, `http://` only, no redirects. The
   decoder is incremental (any split of the input gives the same result), caps the head at 8 KiB and 64 fields, a
   chunk-size line at 256 bytes, the body at 64 MiB whatever the framing says (INV-0286-2), and refuses what could hide a
   second message: both framings, two different lengths, a length that is not digits, any transfer coding but `chunked`,
   a bare LF, folding. The body of a non-2xx response is not printed. `fetch --sum` prints the size, an FNV-1a checksum
   and the time, which is what the network tests compare.
7. **The test server and the test network.** `tools/test-httpd` (Rust, std only) listens on 127.0.0.1, at most 8
   connections, timeouts on every read and write, a head of at most 8 KiB, bodies generated and at most 64 MiB, and it
   exits by itself after its `--lifetime`. The guest reaches it at 10.0.2.2, SLIRP's alias of the host, which needs the
   network without `restrict=on`: `tools/run-qemu.sh --net-open` (with `--net`; refused without it), used only by
   `make test-net`'s fetch step. Nothing but the loopback server is reached by a test; the Internet is reachable only if
   the host has it. `test.zoraya` is a `net.hosts` entry in the test disk's `/config/settings`. The server takes `--port 0` and writes the port it got to a
   `--ready-file`, so worktrees running the step at once do not collide.

## Consequences
- **Positive.** A native program fetches a page; the rings are its own; every parser of outside bytes is in a host-tested
  crate; nothing spins.
- **Negative.** `restrict=off` for one test step; designation without the `+net` rule; no AAAA, no TCP DNS; the resolver's
  identifiers are guessable until N5.
- **Follow-up.** N3b: `+net` words, the REPORT message, the hint, `grants`; N4: `libposix` sockets over `connect`; the
  services channel (ADR-0221 item 6); Q3's per-scope account in `netd`.

## Invariants
- **INV-0286-1.** A `Failed` or `Reset` is the last message of a connection channel and, after it, `netd` holds none of
  the connection's stream ends. *Check:* `cargo test -p zoraya-net` (`conn` tests: refusal, reset, timeout, local).
- **INV-0286-2.** The HTTP decoder never hands more body bytes to its caller than its cap, whatever the framing and
  however the input is split. *Check:* `cargo test -p zoraya-http` (the caps, the splits, 60,000 random inputs).

## Open questions
- When the `+net` words, `grants` output and the REPORT message are built (N3b, or with N6's rooms): until then a `url`
  operand from a value is refused, and `grants fetch URL` prints no network rule (INV-0224-2 stays open, ADR-0224's open
  questions).
- The buffer sizes (32 KiB sockets, 64 KiB rings) against the measured throughput of `docs/design.md`.
- SLIRP's DNS forwarder answers `localhost` from the host's own resolver, which CI does not promise. The two guest steps
  that use it (`tests/shell/net-dns.script`: `localhost` refused as local, `nothing.invalid` unresolvable) run in their
  own boot, locally by default, and are skipped on CI with a visible `[net-dns] SKIP` line unless `NET_DNS=1`; the
  resolver's logic (answers, NXDOMAIN, forged and misdirected answers, the cache, the local rule) is covered by the host
  tests over a scripted DNS server.

## Revisit triggers
- A second user of `connect` that is not `fetch`: the `peer`, `local` and `share` requests of ADR-0222.
- A DNS answer that does not fit 1,232 bytes, or a server that truncates: the TCP fallback.
- IPv6 in the stack: `Connect`'s `address` becomes a family and bytes.

## Architect's decision

(Left empty for the architect.)
