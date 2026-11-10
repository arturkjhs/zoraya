# ADR-0222: The protocol between programs and the stack: streams for connections, messages for datagrams

- **Status:** accepted
- **Date:** 2026-11-04
- **Depends on:** ADR-0006, ADR-0050, ADR-0051, ADR-0052, ADR-0055, ADR-0056, ADR-0101, ADR-0103, ADR-0106, ADR-0165, ADR-0220, ADR-0221
- **Evidence:** CP-08, CP-11, CP-14, DM-074, DM-077, DM-160, X-5, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno, alternatives-postmortem.md#redox, network/survey.md#fuchsias-socket-provider, network/survey.md#plan-9s-net-and-the-connection-server, network/survey.md#posix-sockets-what-curl-and-wget-call, network/survey.md#smoltcp

## Context

`netd` (ADR-0221) must give a program a connection it can read and write, a way to send and receive datagrams, and a
way to listen. ADR-0101 made byte streams a kernel object with `READABLE`, `WRITABLE` and `PEER_CLOSED`, duplicable
ends, and `PEER_CLOSED` from handle counts, and said in its vision section that a `StreamRead`/`StreamWrite` pair "is
what a user-space network stack would hand a POSIX process for a connected socket". ADR-0050's channels carry
messages of up to 64 KiB with handles. ADR-0006's port is the one wait, and stage 6d's `pselect` already subscribes
stream ends (ADR-0106 item 3). ADR-0052 makes protocols Rust declarations with total decoders. The programs Zoraya
will want are stream clients with a readiness call (`network/survey.md#posix-sockets-what-curl-and-wget-call`).

**What is measured.** smoltcp's socket buffers are the caller's slices, sized at creation
(`network/survey.md#smoltcp`). Stream transfer under HVF is 1.0 to 1.8 GB/s and 265 MB/s under TCG (`docs/design.md`,
stage 6 objects as built). Nothing on Zoraya for a connection end to end.

**What is assumed.** That one 64 KiB ring per direction is a fit socket buffer for QEMU user-mode networking; that
UDP datagrams of up to 64 KiB per message suit the one measured UDP user (DNS, done inside `netd` anyway).

## Decision drivers

1. **One wait** (CP-08, ADR-0006): a program waits on its connections, its children and its terminal on one port, so a
   connection's readiness must be a handle signal the port knows.
2. **A connection is bytes, a datagram is a message** (DM-074's complaint that TCP forces every application to frame
   its own messages is about the protocol, not the API; here the API matches each protocol's nature).
3. **No new kernel object**: ADR-0101's streams and ADR-0050's channels suffice (CP-11: typed through ADR-0052, no
   untyped `invoke`).
4. **`pselect` and `poll` from stage 6d work unchanged** on sockets (ADR-0106 item 3, ADR-0225).
5. **Bounded buffers per connection, committed at creation** (CP-14, ADR-0101's rule for streams).
6. **The peer's close and reset are visible as values** (DM-077's `SIGPIPE` stays in the personality, ADR-0106 item 8).

## Considered options

### Option A: Sockets as kernel objects with socket system calls (the Unix baseline)
- **Description:** `socket`, `connect`, `read`, `write`, `sendto` as system calls on a kernel socket object backed by
  the kernel's stack.
- **Evidence for:** One copy; exactly what C programs call.
- **Evidence against:** The stack is in user space (ADR-0221), so a kernel socket would be a kernel proxy for a service
  call, one more object type for no isolation gain; `sockaddr` casts in the ABI (DM-074).
- **Cost:** *(estimate)* A kernel object and a proxy path.
- **Precedent:** Unix.

### Option B: One channel per socket, bytes in messages (Plan 9's `data` file, 9P-style)
- **Description:** A connection is a channel to `netd`; `write { bytes }` and `read` replies carry the data, as Plan 9's
  `/net/tcp/N/data` carries it through 9P (`network/survey.md#plan-9s-net-and-the-connection-server`,
  `alternatives-postmortem.md#plan-9--inferno`).
- **Evidence for:** One object kind for everything; messages are framed and typed; nothing to decide about rings.
- **Evidence against:** Every byte is a message with a 16-byte header and a round trip; a `read` is a request and a
  reply, two system calls each side, where a stream is one; `pselect` would wait on channel `READABLE`, which means "a
  reply arrived", not "data is there", so the library would have to keep a request outstanding per socket (the
  problem ADR-0106 item 3 notes for file connections). Partial writes and `WRITABLE` need protocol state that streams
  give for free.
- **Cost:** *(estimate)* Smallest protocol; the highest per-byte cost and the most library state.
- **Precedent:** Plan 9 (production, research scale), 9P over networks.

### Option C: Stream ends for connection bytes, a control channel per connection, messages for datagrams (Fuchsia's socket plus FIDL)
- **Description:** `connect` returns a `StreamRead` end, a `StreamWrite` end and a *control* channel end; `netd` keeps
  the other ends and copies between smoltcp's buffers and the rings. Datagram sockets are a channel whose messages
  are datagrams. Listening is a channel whose messages carry new connections' three handles. Fuchsia's socket protocols
  describe a Zircon socket "on which data is sent and received" beside the FIDL channel that carries the calls
  (`network/survey.md#fuchsias-socket-provider`), the shape of a stack that is a component
  (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence for:** Drivers 1 to 6. The port already knows stream signals, so `pselect` works on day one; a read is one
  system call when data is there; `PEER_CLOSED` on the read end is end of file and on the write end a reset or a closed
  peer, by ADR-0101's handle counts, so a dead `netd` looks like a reset without any protocol (ADR-0221 item 5).
  Datagrams fit ADR-0050's all-or-nothing messages with their bound as the receive buffer.
- **Evidence against:** Three handles per connection; two 64 KiB rings per connection in kernel memory until
  ADR-0056 charges them; a second copy in `netd` (smoltcp buffer to ring); FIN and RST must be mapped onto handle
  closes and a control message.
- **Cost:** *(estimate)* A protocol crate of 300 lines; the copy loop in `netd`; thin `libposix` wrappers over calls
  that exist.
- **Precedent:** Fuchsia (shipping); ADR-0101's own forecast.

### Option D: A shared-memory ring per socket (Redox rings)
- **Description:** Program and `netd` share a memory object with a ring; readiness through an event object.
- **Evidence for:** Fewest copies; Redox's gain (`alternatives-postmortem.md#redox`).
- **Evidence against:** A ring in shared memory is hostile input to both sides (DM-160, X-5), liveness needs a kernel
  object anyway, and the measured need is a page fetch, not bulk.
- **Cost:** *(estimate)* A protocol per ring and a double-fetch rule everywhere.
- **Precedent:** Redox (bulk I/O).

## Recommendation

**Recommendation (not a decision): Option C**, declared as `zoraya-proto-net` under ADR-0052, one crate.

**On the network connection** (the scope channel of ADR-0220):

1. `status() → { link, address, gateway, dns[] }` (right `status`).
2. `resolve(name) → { addresses[] }` (right `reach`, name in scope; ADR-0223).
3. `connect(target: Name | Address, port, protocol: Tcp, server_end) → ()`: `netd` checks the scope and attaches a
   *connection* to `server_end` (the client made the channel, as ADR-0055's `open`), and replies at once, with
   `NotGranted` if the scope refuses; the scope channel is then free for the next request (ADR-0055's one request at
   a time). It then resolves a name (ADR-0223) and opens the TCP connection, and sends on the connection channel
   either `connected { read_end (moved), write_end (moved), peer }` or `failed { reason: Unresolvable | Refused |
   Unreachable | TimedOut }`. So a blocking `connect` waits on the connection channel's `READABLE`, a non-blocking one
   returns and polls it, and no program waits on a stream end it has not received. Two 64 KiB rings are made per
   connection when it is established (ADR-0101's capacity; the TCP window `netd` advertises is the free room in the
   receive ring plus smoltcp's own buffer).
4. `datagram(protocol: Udp, local_port: Any | n, server_end)`: attaches a *datagram socket* (right `reach`); a named
   local port needs a listen rule, `Any` is an ephemeral port; each `send` is checked against the reach rules.
5. `listen(protocol: Tcp, port, server_end)`: attaches a *listener* (right `listen`, port in a listen rule).
6. `narrow(scope, rights, server_end)` (ADR-0220 item 5).

**On a connection channel:** from `netd` first `connected` or `failed` (item 3); then `peer() → { address, port }`,
`local()`, `shutdown(write)`, `keep_alive(on)`, `no_delay(on)`, `share(server_end)` (a second control channel to the
same connection, for `fork` and for a parent that keeps a socket it passes to a child, as ADR-0103's `share` for
files), and from `netd`: `reset { reason }` when the peer resets or the connection fails, after which `netd` closes
its stream ends. A control channel that shows `PEER_CLOSED` with no `reset` means `netd` is gone (ADR-0221 item 5). The peer's FIN closes `netd`'s write end to the program, so `stream_read` returns 0
(ADR-0101); the program's close of its write end makes `netd` send FIN once its ring drains; closing every handle to
the connection channel and both stream ends closes the connection in `netd`. A reset while unread data remains is
delivered after the data, as TCP does `[assumption: smoltcp's order]`.

**On a datagram socket channel:** `send { to: Name | Address, port, bytes }`: a `Name` is resolved under the scope as
`connect`'s is (ADR-0223), so a scope of names grants UDP too; an `Address` needs an address rule. `bytes` is at most
`DATAGRAM_MAX`, 65,000 bytes `[assumption: ADR-0050's 65,536 less the 16-byte header, the target field of up to 256
bytes, the port and the length of `bytes` under ADR-0052's layout, rounded down]`, which is below UDP's 65,507; a longer payload is `InvalidArgument` (`EMSGSIZE` in `libposix`). `received { from: Address, port,
bytes }` is pushed by `netd`; the channel's queue bound (64 messages, 256 KiB) is the receive buffer, and a datagram
that does not fit is dropped and counted, which is UDP. Readiness is channel `READABLE`.

**On a listener channel:** `accepted { read_end, write_end, control_end, peer }` pushed by `netd` for each accepted
connection; the queue bound is the backlog; when it is full `netd` stops accepting (the SYN waits in smoltcp's
backlog); `share(server_end)` gives a second listener channel on the same listening socket, to which later
connections are delivered too, for a child that inherits a listening socket while the parent keeps its pending
connections (ADR-0050 forbids moving a channel with handles queued). Readiness is channel `READABLE`.

**Readiness** (driver 1): stream ends are subscribed as `pselect` subscribes them today (ADR-0106 item 3); datagram
sockets and listeners as channels for `READABLE | PEER_CLOSED`. `poll` and `select` in `libposix` are the same port
wait (ADR-0225).

**Copies.** Device to slot (DMA), slot to smoltcp buffer and buffer to ring in `netd`, ring to the program in the
kernel: three copies in, three out. Not measured; the revisit trigger names the measurement.

**Relation to ADR-0056.** Each connection is two kernel rings (128 KiB, charged to `netd` as their maker) plus three
handles in `netd`'s table and smoltcp's buffers in `netd`'s memory; a datagram socket is one channel and its queue; a
listener its queue. `netd` bounds connections per scope (`[assumption]` 64) until ADR-0056's accounts carry a budget.

**Examples.** From a native program, in Rust, what the crate's client looks like (not typed by a person):

```
let net = Net::from_startup()?;                       // the NET slot, or NotGranted
let conn = net.connect(Target::name("example.org"), 443, Tcp)?;   // waits for `connected`: stream ends + control
conn.write_all(b"GET / HTTP/1.1\r\nHost: example.org\r\n\r\n")?;  // stream_write and WRITABLE waits
let n = conn.read(&mut buf)?;                          // stream_read, 0 at FIN
```

**Tensions with accepted ADRs.**
- *ADR-0101* lists sockets among byte streams in its vision section and ADR-0006 rule 1 lists "sockets later" among
  byte streams: this ADR makes that true for TCP, and makes UDP a channel, which rule 1 did not foresee; accepting
  this ADR adds the row.
- *ADR-0050*'s rule that an endpoint with handles queued cannot be moved: a listener channel with `accepted` messages
  queued cannot be forwarded unread; a server drains it first, as ADR-0050 says of servers.
- *ADR-0055*'s one-request-at-a-time rule per connection holds for the control and scope channels; the stream ends
  carry no requests.

*Note, 2026-11-10 (the architect's decision on ADR-0262, recorded by an agent):* `connect` and `accept` take the
client's far ends of two streams (ADR-0101) that the client created, instead of `netd` making them, so the 128 KiB of a
connection is the client's room's memory (ADR-0261); the reply `connected { peer }` carries no handles; `netd` checks
both handles and uses them only with non-blocking calls; and `connect`'s failure reasons gain `OverQuota`, for the
room's account bound in `netd` (ADR-0262 item 2). This answers this ADR's open question on who creates the streams.

### Effect of the vision
For target 2 nothing changes: the scope (ADR-0220) is checked at `connect`, `datagram` and `listen`; an established
connection is three handles an agent can pass to a helper, which is Option D of ADR-0220. Threads: streams and
channels are atomic per call. Target 3: a desktop program's client loop (`zoraya-app`, ADR-0165) adds connection
signals to the port it already waits on. Moving to Option D's rings later is per connection and opt-in, as ADR-0051
reserved.

## Consequences
- **Positive.** `pselect`, `poll` and `select` need nothing new; a dead stack is a reset; UDP and listening reuse
  channels; no kernel change.
- **Negative.** Three handles and 128 KiB of kernel memory per connection; two extra copies in `netd`.
- **Follow-up.** `zoraya-proto-net`; the copy loop and its host tests in `zoraya-net`; the `libposix` mapping
  (ADR-0225); the `zoraya-net` client crate for native programs.

## Invariants
- **INV-0222-1.** A connected stream's bytes arrive in order and complete: a host test streaming random data through
  `netd`'s logic over a loopback device sees exactly what was sent, in both directions, with reads and writes of
  random sizes. *Check:* a host test of `zoraya-net` with smoltcp's loopback.
- **INV-0222-2.** `connected` arrives on the connection channel before any stream end exists for the program; the
  peer's FIN makes `stream_read` return 0 after the data; the peer's reset asserts `PEER_CLOSED` on both ends and
  delivers `reset` on the control channel; closing the program's write end sends FIN after the ring drains. *Check:* host tests of `zoraya-net` against a scripted peer over the loopback device, observing the handle
  states through the fake kernel.
- **INV-0222-3.** Every message of `zoraya-proto-net` decodes totally, round-trips, and refuses extra handles or
  nonzero padding. *Check:* the generated round-trip and mutation tests of INV-0052-1 and INV-0052-2.
- **INV-0222-4.** A datagram that does not fit the socket's queue is dropped and counted, never blocks `netd` and
  never ends the socket. *Check:* a host test sending more datagrams than the queue holds to a socket nobody reads,
  then reading `status`'s counters.

## Open questions
- The per-connection ring size: 64 KiB per direction is ADR-0101's; a smaller ring for many connections or a
  larger one for throughput, measured.
- Whether the program should create the two streams and pass `netd` their far ends in `connect`, so that its own
  job pays for them (ADR-0056's rule for connections) and `netd` holds no ring it did not ask for; the recommendation
  has `netd` make them, which is simpler for the client and leaves a per-scope bound to `netd`.
- `MSG_PEEK`, out-of-band data and `SO_LINGER`: not offered; a program in scope that needs one reopens this.

## Revisit triggers
- A measured transfer where the two copies in `netd` dominate: a shared ring per connection (ADR-0051).
- A program in scope needing raw sockets or ICMP (a `ping` command): a `raw` request behind a right of its own.
- A connection count that makes three handles each too many for `netd`'s table (ADR-0055's 896 lesson).

## Architect's decision

Accepted 2026-11-04. Decided by the architect; recorded by an agent at the architect's request.
