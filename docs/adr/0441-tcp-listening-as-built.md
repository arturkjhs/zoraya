# ADR-0441: TCP `bind`, `listen` and `accept` as built (plan part F7b): the loopback is `netd` handing a client's stream ends to a listener, not a second smoltcp interface

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0220, ADR-0221, ADR-0222, ADR-0223, ADR-0224, ADR-0225, ADR-0262, ADR-0287, ADR-0434, ADR-0439, ADR-0440
- **Evidence:** CP-01, GR-008

## Context

Plan part F7's second half, "TCP `bind`/`listen`/`accept` over `netd`", is done when `python3 -m http.server` serves a file to
`curl` in the same room. The proposed ADR-0440 designed it (Option A: a `listen` request, `Accepted` on a listener channel, a
second smoltcp interface for loopback) and named a spike for the second interface. This ADR records what F7b built and why it
differs in one point, so that the architect decides on what exists.

**Read (the spike, by source, not by experiment).** smoltcp 0.14.0, `iface/interface/mod.rs`: `socket_egress` visits every socket of the
set and sends a socket's packet only if `has_neighbor(dst)` holds on *this* interface, which is `route(dst)` (the interface's
own networks and routes) and, for Ethernet, a neighbour entry. A SYN to 127.0.0.1 is therefore skipped by the Ethernet
interface and would be dispatched by a loopback interface that has 127.0.0.0/8; two interfaces over one `SocketSet` look
feasible. **Not measured:** no two-interface exchange was run, and the costs below were judged before it.
**Measured:** `make test-net-posix` (this part's step) runs the design that was built.

The listener grant is not open any more: ADR-0224 (accepted) decides `+listen PORTS`, and ADR-0220 item 2 has the listen rule
in the scope; the shell did not build the word before this part (ADR-0224's notes of 2026-11-11 and 2026-11-12). `netd` has no
notion of a room: a scope connection belongs to an *account* (ADR-0262), and the shell gives each POSIX program an account of
its own, carved from the shell's (`POSIX_NET_BUDGET`), so `python3` and `curl` of one line are two accounts.

## Decision drivers

1. **A listener is authority** (CP-01): a program listens only if its scope says so, on the ports it says; the library holds nothing
   (a hostile program that edits its own state gains nothing, GR-008).
2. **Loopback is not the Internet** (ADR-0223 item 4): reaching it is a rule a person writes; `web` never covers it.
3. **Bounded in `netd`** (ADR-0262, ADR-0434): listeners per account, pending connections per listener, nothing per connection.
4. **No new state that can leak**: `netd` is one thread with 16 connection slots; a loopback connection that held a slot, a
   smoltcp socket and two copy loops would take the room's connections (ADR-0262) for a local exchange.
5. **The acceptance must run on the machine of the task**: one QEMU guest, no host network but the existing user-mode one.

## Considered options

### Option A: `netd` pairs the streams (built)

- **Description:** `listen(port)` is one request that carries the listener channel's server end. A `connect` to 127.0.0.0/8 that the
  scope allows is not opened in the stack: `netd` writes `Accepted { address, port }` with the client's own two stream ends (the
  read end it holds of the stream the client writes, the write end of the stream the client reads) to the listener channel, tells
  the client `Connected` and closes its end of the connection channel. Nothing of the connection stays in `netd`.
- **Evidence for:** drivers 3, 4, 5. Zero copies, no frame, no ARP, no DHCP lease needed; the connection is the client's two rings
  and nothing else. A peer that closes is the end of the other's stream, as for a pipe.
- **Evidence against:** it is not TCP: no reset message (`ECONNRESET` for a dead peer is an end of file or `EPIPE`), no
  `SO_LINGER`, no TIME_WAIT; and **only the loopback is served**: a listener is not reachable from the NIC (the QEMU host, the
  Internet), so `bind(0.0.0.0)` means the loopback. ADR-0440 item 5's second interface would have shown the same TCP exchange
  to a program over smoltcp and, with a listening smoltcp socket, would have accepted connections from the NIC too.
- **Cost:** *(estimate)* small: one request, one message, 60 lines in `netd`'s loop; the host-tested decision is in `Service`.
- **Precedent:** Unix domain sockets of ADR-0439 (a pair's far end forwarded to a listener); a Linux `socketpair`.

### Option B: a second smoltcp interface for 127.0.0.0/8 (ADR-0440 item 5)

- **Description:** a loopback `Device`, a second `Interface` polled in the same loop, a smoltcp listening socket per listener and
  a server-side copy loop (`Conn`'s mirror) per accepted connection.
- **Evidence for:** real TCP semantics on the loopback, and a path to listening on the NIC's address.
- **Evidence against:** drivers 4 and 5: an accepted connection needs a slot, a smoltcp socket (the 8 of the stack are global) and
  128 KiB of streams that the *listener* pays (ADR-0440 item 3) plus the client's own; the poll loop must also bound a second
  device; a `netd` that is down or has no lease still has to serve the loopback.
- **Cost:** *(estimate)* several days with its own host tests over the loopback device, and a QEMU step.
- **Precedent:** smoltcp's own `loopback` example (one interface, `Medium::Ip`).

### Option C: the baseline, as POSIX has it

`bind` and `listen` always succeed for any port above 1023 and the loopback is ambient (every program reaches every port).
Against drivers 1 and 2 (CP-01): authority from nowhere.

### Option D: no listening; servers use Unix sockets only

What F7a built (ADR-0439): `http.server` cannot run, and neither can any program that serves a browser or another program.

## Recommendation

**Recommendation (not a decision): Option A, as built, with Option B recorded as the way to listen on the NIC if a part ever needs
it.**

1. **The grant.** ADR-0440 item 1 stands: the right `LISTEN` and a listen rule that covers the port and TCP. The word is
   ADR-0224's `+listen PORT` or `+listen LOW-HIGH` (TCP and UDP, as the rule is), built in the shell by this part
   (`zoraya_shell::netgrant::parse_listen`); `grants` prints it as `net: port 8000  listen (grant +listen)`. A connect to the
   loopback needs a reach rule that *names* the address (`names_address`: `+net 127.0.0.1[:PORT]`, an `ADDRESS/BITS` prefix or
   `any`), exactly as ADR-0223 item 4 has it for any local address; `web` never covers it. A `curl http://127.0.0.1:PORT/` of a
   line gets that rule from its literal URL operand (an address keeps its own rule, ADR-0328).
2. **The messages.** `listen` (request 5, one handle: the server end of the listener channel the client made; `port` 0 asks for a
   port above 49151 that a listen rule covers) answered `Listening { status, port }`: `NOT_GRANTED`, `BUSY` (the port is in use,
   by anyone, on the machine: `netd` has no room, ADR-0262's accounts are not rooms), `OVER_QUOTA`, `UNAVAILABLE` (`netd` could not
   serve the channel). `Accepted` (19, two handles) on the listener channel, from `netd`: the *read* end of the stream the peer
   writes and the *write* end of the stream the peer reads; its `address` is 127.0.0.1 and its `port` a number `netd` made up
   (49152 and above, from a counter). A listener ends with its channel: `netd` watches `PEER_CLOSED` and frees the port and the
   account's charge; connections still queued go with the channel's last end (the kernel closes their handles, their clients
   read the end of the stream).
3. **Bounds.** Listeners (the code's account table, row changed from 4, 0, 0 to 8, 1, 4; ADR-0262's rule "0 listeners unless
   `+listen`" is kept: a carve whose rights have `listen` takes one, any other none): 8 on the machine, one per carved account
   with the right (the shell's per-program account), the column of carved accounts 4, the session's the rest. Pending connections per listener:
   the listener channel's 64 messages (`zoraya_channel::MAX_QUEUED`, as for ADR-0439), not a number of `netd`'s: the
   65th `connect` is refused with `Failed { REFUSED }` and every handle it gave closed. `listen`'s `backlog` is a hint and
   changes nothing. A loopback connection is charged to no account and holds nothing in `netd` after the request, so `connect`
   past the account's connections is not a bound here; what bounds it is the client's own two streams (its room's memory)
   and the 64.
4. **Which listener a connect reaches.** The one on that port, whoever's account: the loopback is the machine's. A reach rule of
   one program and a listen rule of another is two grants, each made by a person for its own program; `netd` cannot tell a room, and
   `python3` and `curl` of one line are different accounts, so an account-scoped loopback would fail the acceptance. A room that must be
   closed from the others' loopback is one without a reach rule for 127.0.0.1.
5. **Names.** A name that resolves to 127.0.0.0/8 (a `net.hosts` entry) is `UNREACHABLE` at the candidate (it must not leave by the
   NIC); only an address operand reaches the loopback. `localhost` is not built.
6. **The library.** `bind` records a `sockaddr_in` (the wildcard or 127.0.0.0/8, else `EADDRNOTAVAIL`; AF_INET6 is
   `EAFNOSUPPORT` from `socket`); `listen` is the request (an unbound socket listens on the port `netd` picks); `accept` and
   `accept4` read the listener channel and make a connected `Socket` fd from the two handles (no connection channel, so no
   `Reset`), blocking, or `EAGAIN`; the new fd is blocking unless `SOCK_NONBLOCK` is given; `EMFILE` leaves the connection
   queued; `getsockname` is the bound address and the port `netd` made of it (0.0.0.0 reads back as 0.0.0.0); `SO_ACCEPTCONN`; `poll`
   is `POLLIN` while the channel has a message; `SO_REUSEADDR` is accepted and ignored. A TCP socket does not cross `fork` or `spawn` (`ENOTSUP`), as before.
7. **What is not built.** Listening on the NIC (a connection from outside); UDP; `localhost`; `getsockname` of a socket
   `connect` made (the wildcard and 0); per-connection `ECONNRESET`; a `Reset` for a listener's death mid-connection.

### Effect of the vision

A listener and a loopback connection are capabilities of the scope: a hostile program that edits its library state gains nothing, `netd`
checks every port and every rule; an untrusted line has neither unless a person wrote them (CP-01, GR-008). The streams a client hands over are
its own: a listener cannot do more to the client than the client's ends allow, and a client cannot give a listener more than it holds
(the kernel's rights containment on the forwarded entries).

## Consequences

- **Positive.** `python3 -m http.server` serves a file to `curl`; a test server of ours is possible; ADR-0440's follow-ups
  (the message, the bounds, the word) are built and tested; the loopback works with no lease.
- **Negative.** The loopback is not TCP; a program that expects a real peer address of a connection sees 127.0.0.1 and a made-up
  port; the NIC cannot reach a listener; the accounts table's listener row changed.
- **Follow-up.** The architect decides whether the loopback must be per-account or per-room once rooms are known to `netd`;
  Option B for the NIC; `localhost`.

- **Decisions for the architect, before accepting this ADR.**
  1. **The loopback belongs to the whole machine.** A program whose scope has a reach rule naming 127.0.0.1 (or `any`) can
     connect to *any* listener on the machine, another room's server included, can tell which ports are listened on (a refused
     connect against an accepted one), and can fill a listener's 64-slot queue so that its owner's own clients are refused.
     `netd` cannot tell rooms (the shell carves an account per POSIX program, so `python3` and `curl` of one line are two
     accounts), which is why it is not narrower. This is a **condition of the untrusted-room gate (ADR-0268)**: an untrusted
     room must not be given a reach rule for the loopback until one of these exists: (a) a per-room loopback once `netd` knows
     rooms; (b) an ACL on a listener, by account (the listening account names the accounts that may connect). Until then an
     untrusted line has no `+net 127.0.0.1` word to give, and a trusted one is the person's own grant.
  2. Whether the NIC is listenable (Option B).
- **Known limitations of the build.** `bind` of port 0 followed by `getsockname` *before* `listen` reads port 0: the port is
  picked by `netd` at `listen` (CPython's `socketserver` reads it after `listen`, but `test.support`'s `find_unused_port`,
  xmlrpc and some servers read it before). Fixing it needs a port reserved at `bind` (a new request or a library-side pick that
  `netd` may refuse), so it is not built; recorded in `docs/backlog.md`. `netd` keys listeners on the port alone (not
  address): `bind` of 127.0.0.1 and of 0.0.0.0 on one port are the same listener. If the holders of the session account (a
  line's own programs that join it instead of carving) take all 8 listeners, a `+listen` line of a carved account has its
  listen refused (`ENOBUFS`) and the network of that line is useless for serving.

## Invariants

- **INV-0441-1.** A loopback `connect` leaves nothing in `netd`: the two stream ends are in the listener's queue or closed, the
  connection channel's end is closed, and no account is charged; a refused one (no listener, a full queue, a listener gone) is told
  `REFUSED` with every handle closed. *Check:* `make test-net-posix` (`tcptest`: the 65th connect, a closed listener with 64
  connections queued) and `cargo test -p zoraya-net` (the loopback's decision charges nothing).
- **INV-0441-2.** Listeners are bounded: 8 on the machine, one per carved account whose rights have `listen`, a port has at most one, and a closed or
  unserved listener gives the port and the charge back exactly once. *Check:* `cargo test -p zoraya-net` (`listeners_are_bounded_per_account_at_exactly_the_bound`,
  `a_port_in_use_is_busy_for_anyone_and_closing_frees_it_and_the_charge`).
- **INV-0441-3.** A name that resolves to 127.0.0.0/8 opens no socket: nothing addressed to the loopback leaves by the NIC.
  *Check:* `cargo test -p zoraya-net` (`a_name_that_resolves_to_loopback_never_opens_a_socket`).
- **INV-0441-4.** `accept` takes from the listener channel only an `Accepted` with exactly two handles; anything else is closed and
  skipped, and a full descriptor table leaves the connection queued. *Check:* `cargo test -p zoraya-posix --test tcplisten`.

## Open questions

- Whether the loopback should be scoped to an account or a room once `netd` knows rooms (item 4), and whether a program with a listen rule and no reach
  rule should reach its own listener.
- Whether the NIC's address should be listenable (Option B), and the address a listener reports (the wildcard now).
- The `localhost` name, and `SO_REUSEPORT`.

## Revisit triggers

- A part that needs a connection from outside the machine (hostfwd, a test server on the host side).
- A program that needs `ECONNRESET` or `SO_LINGER` semantics on the loopback.
- `netd` learns what a room is.

## Architect's decision
