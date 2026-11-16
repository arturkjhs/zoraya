# ADR-0440: TCP `bind`, `listen` and `accept` over `netd`: what listening needs, and why plan part F7 stops short of it

- **Status:** accepted
- **Date:** 2026-11-16
- **Depends on:** ADR-0220, ADR-0221, ADR-0222, ADR-0225, ADR-0262, ADR-0287, ADR-0434, ADR-0439
- **Evidence:** CP-01, GR-008

## Context

Plan part F7's second half is "TCP `bind`/`listen`/`accept` over `netd`", done when `python3 -m http.server` serves a file to `curl`
in the same room. ADR-0225 item 5 decides the library's half (`bind` records the address, `listen` is `listen(port)` to `netd`,
`accept` reads the listener channel and makes a socket from three handles) and says how a person grants a listener is not decided.
Reading what exists: the scope already has what a grant needs (`zoraya_abi::net::ListenRule`, ports and protocols, `allows_listen`,
rule counts in `encode`, ADR-0220 item 2), so a listener grant is a rule in a scope that a connection already carries and `Narrow`
already narrows; `netd`'s protocol has no `listen` message (the header of `zoraya-proto-net` says "`datagram`, `listen` and `resolve`
come later"; `resolve` came); `netd`'s stack is smoltcp over one Ethernet link with no loopback interface, so a connection from a
program to `127.0.0.1` would leave through the NIC and, under QEMU's user network, not return. ADR-0223 item 4 makes loopback a
local address that no rule of the web grant covers. **Measured:** nothing; this part ran no network step. **Assumed:** that one
program in a room both listens and connects to itself over `127.0.0.1` is the case the plan names, and that the first listening
programs are `python3 -m http.server` and a test server of ours.

## Decision drivers

1. **A listener is authority** (CP-01): a program listens only if its scope says so, on the ports it says; the library holds nothing (a hostile program that
   edits its own state gains nothing, GR-008).
2. **The grant exists**: the least committing mechanism is the rule already in the scope, with no new right or message for the
   grant itself.
3. **Bounded in `netd`** (ADR-0262, ADR-0434): listeners and pending connections per account.
4. **Loopback is not the Internet** (ADR-0223 item 4): reaching it is a rule a person writes, never covered by `web`.
5. **Nothing in this design may be built without a network step that runs** (`make test-net`: QEMU with `restrict=on`), which was judged out of reach on the 8 GB machine of this task
   alongside the Unix half.

## Considered options

### Option A: listening is a `listen` request on the scope channel; accepted connections come on a listener channel with their three handles

`netd` checks `allows_listen(port, TCP)` against the connection's scope, opens a smoltcp listening socket, and sends each
established connection to the client on the listener channel as `Accepted` with the connection channel and the two stream ends (the
shape of `Connect` reversed). A loopback interface in `netd`'s stack carries 127.0.0.0/8.

### Option B: a `listen` that hands the program a bound socket of its own over a second protocol

A second kind of connection in `netd` for sockets, with their own state; more than the stream client needs.

### Option C: the baseline, as POSIX has it

`bind` and `listen` always succeed for any port above 1023, and a loopback every program reaches.

### Option D: no listening; servers use Unix sockets only

What F7 builds (ADR-0439): `http.server` then cannot run, and anything that serves a browser from a program cannot.

## Recommendation

**Recommendation (not a decision): Option A, to be built as its own plan part with a network step; not built in F7.**

1. **The grant.** A program listens on port `p` over TCP if its connection to `netd` has the `LISTEN` right (ADR-0220) and its scope
   has a listen rule covering `p` and TCP. Nothing new is added to the scope format or to `GRANT_NET`: a child that is given the
   network (`POSIX_SPAWN_GRANT_NET`, ADR-0434) has the parent's listen rules, and a narrower one is a `Narrow` first. How a person
   writes the rule (a word for the line, a room's setting) is the architect's, as ADR-0225 item 5 said; the test grants it with a
   scope built in the test.
2. **The message.** `listen` (request 5, one handle: the server end of the listener channel the client made) with the port (0 for
   an ephemeral port above 49151, reported by the answer) answered `Listening { status, port }`. Statuses: `NOT_GRANTED`, `BUSY`
   (the port is in use, by anyone), `OVER_QUOTA` (the account's bound), `UNAVAILABLE` (no address yet). One listener per port in
   `netd`; a listener ends with its channel.
3. **Accepting.** For each established connection `netd` makes the connection channel and the two streams, keeps its ends, and
   writes `Accepted { address, port }` with the three client-side handles to the listener channel; the room that listens pays for
   their 128 KiB through the account's connection bound (ADR-0262), and a listener whose queue holds 8 unaccepted connections is
   not served another (smoltcp's backlog of 8 per listener, ADR-0434's bound).
4. **Bounds per account.** 4 listeners and 8 pending connections; a connection counted for the account while it lives.
5. **Loopback.** A second smoltcp interface on a loopback device with `127.0.0.1/8`, polled by the same loop; a connect whose
   address is in 127.0.0.0/8 goes to it and is checked against a reach rule that names it (`net reach 127.0.0.1:8000`; `web` never
   covers it), and the listening side against a listen rule. A program that listens and connects to itself needs both.
   **Open:** smoltcp 0.12 dispatches a socket's packets through the interface whose route matches, with sockets shared by two
   interfaces; a spike must show that a TCP exchange over the loopback interface completes while the Ethernet interface carries
   the DHCP client and DNS, before the design is accepted.
6. **The library.** `bind` records a `sockaddr_in` (port 0 allowed, any address that is 0.0.0.0 or the interface's or 127.0.0.1);
   `listen` is the request; `accept` and `accept4` read the listener channel and build a socket from the three handles (the
   `connect` code's constructor); `getsockname` of a listener is its port; readiness is the listener channel's, as ADR-0225 item 8
   has it; `SO_REUSEADDR` stays accepted and ignored.
7. **What F7 did.** `bind`, `listen` and `accept` on a TCP socket are `EOPNOTSUPP`, so that programs link and report it; the
   Unix half is ADR-0439.

### Effect of the vision

A listener is a capability of the scope: a hostile program that edits its library state gains nothing, `netd` checks every port; an
untrusted line has no listen rule unless a person wrote one.

## Consequences

- **Positive.** `http.server`, a test server, `ssh`'s port forwards and a browser's local servers become possible in the same
  grant that already carries the rest of the network.
- **Negative.** A second smoltcp interface and a path for loopback in `netd`; a QEMU step to show it.
- **Follow-up.** The spike of item 5; the message and its host tests against a scripted peer over smoltcp's loopback (ADR-0222's
  method); `libposix`'s `accept`; `test-python` with `http.server` and `curl` in one room; the architect's word for the rule.

## Invariants

- **INV-0440-1.** A connection to `netd` listens on a port only if its scope has a listen rule that covers the port and TCP, and
  `listen` past an account's bound of listeners is refused with nothing held. *Check:* host tests of `zoraya-net` over smoltcp's
  loopback (scope cases, the bound).
- **INV-0440-2.** A connection from a program to 127.0.0.0/8 is made only if its scope has a reach rule that names the address, and
  `web` never covers it. *Check:* the same host tests, and `make test-net` with a listener and a client in one room.

## Open questions

- Whether smoltcp's two-interface dispatch carries a loopback exchange (item 5's spike).
- Whether the address a listener reports should be the interface's or 0.0.0.0.
- The word a person writes for a listen rule.
- *Note, 2026-11-16 (plan part F7b, recorded by an agent; ADR-0001 rule 6):* F7b built Option A's request and bounds and the library
  of item 6, without the second interface of item 5: the spike was a reading of smoltcp 0.14's `socket_egress` (a socket's
  packet is sent only by an interface that routes its destination), not an experiment, and the loopback became `netd` handing
  the client's two stream ends to the listener (proposed ADR-0441). Item 3's `Accepted` has two handles, not three. Item 4's
  bounds are 8 listeners, one per carved account with the `listen` right, and 64 queued (the listener channel's), not 4 and 8.
  INV-0440-1 and INV-0440-2 are checked by the host tests of `Service` and `make test-net-posix`, not over smoltcp's loopback.

## Revisit triggers

- The architect chooses the user-facing grant: this ADR's item 1 becomes a decision.
- A program that needs UDP listening (`datagram`).

## Architect's decision

Accepted 2026-11-16. Decided by the architect; recorded by an agent at the architect's request.

Accepted as built.
