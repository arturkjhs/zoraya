# ADR-0287: What N4 decided where ADR-0225 is silent: the connection to `netd`, the surface of the stream client, a POSIX program's `url` operand, and what is not built

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0005, ADR-0100, ADR-0107, ADR-0141, ADR-0153, ADR-0220, ADR-0222, ADR-0224, ADR-0225, ADR-0262, ADR-0286
- **Evidence:** CP-27, CP-28, DM-074, DM-075, DM-077, X-1, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, network/survey.md#posix-sockets-what-curl-and-wget-call, network/survey.md#capsicum-and-casper

## Context

N4 of TASK-26 is the second program that uses the network: a C program, built with `zcc` against `libc.a`, fetches the
page that N3's `fetch` fetches (`getaddrinfo`, `socket`, `connect`, `send`, `recv`). ADR-0225 decides the whole socket
surface (a stream client, listeners, datagrams, `socketpair`, `poll`, inheritance of three handles through `spawn` and
`fork`) and the plan lists curl and wget as W11 and W13. This part builds the stream client end to end and the
calls a client needs; the rest of ADR-0225 is listed below as not built, with the reason. The code needed choices
ADR-0225 does not make; none is against it. **Measured:** the cost of the path in `docs/design.md` ("TCP, names and
`fetch`", N4 rows). **Assumed:** the first POSIX network program in the tests is a small C client; curl and wget need
`poll`, `getnameinfo`, `gethostbyname` and TLS (ADR-0228), which come later
(`network/survey.md#posix-sockets-what-curl-and-wget-call` counts the calls of both).

## Decision drivers

1. A POSIX program has the network only through what its launcher gave it (ADR-0220, INV-0225-1): no second policy
   in the library (ADR-0225 driver 2; CP-28; Capsicum's `socket` that succeeds and a `connect` that fails,
   `network/survey.md#capsicum-and-casper`). A program runs unmodified, with `sockaddr` as C expects (DM-074, X-1).
2. The two rings of a connection are paid by the client (ADR-0222's note, INV-0262-4): a socket fd holds the three
   handles `connect` makes, in the process, not in the broker (INV-0002-3).
3. The surface grows only by ADR, with a workload behind each row (CP-27, INV-0005-1).
4. A thing not built fails closed with a defined `errno`, never as a description with no handles behind it.
5. A name is resolved by `netd`, never by a UDP socket of the library (DM-075), and a write to a closed connection is
   `EPIPE` with `SIGPIPE` unless `MSG_NOSIGNAL` (DM-077).

## Considered options

### Option A: the choices below (the recommendation)

### Option B: the whole of ADR-0225 in one part

Listeners, datagrams, `socketpair`, `poll`, inheritance and curl in N4, the whole personality shape of Starnix
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`). Rejected here only for size: `netd` has no
`listen` or `datagram` request yet (ADR-0286 builds `connect` and `resolve`), so most of it could not be tested against
the real service. It stays the follow-up.

### Option C: a fixed slot for the connection, beside `REPORT`

Built first and found wrong: a handle that arrives in a message takes the slot after the last one `spawn` filled, so
the first directory entry of a program without a network landed in the slot after `FD0` and the library took a
file service's channel for `netd` (the test of a program started without the grant found it: `connect` answered
`EIO`). A slot cannot say "absent".

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The connection comes in the second bootstrap message**, beside `SELF`: a message `ZPBN`
   (`zoraya_abi::posix::SELF_NET_MESSAGE`) with two handles, `SELF` and a channel, where a launcher that gives no
   network sends `ZPBS` and one handle as before. `libposix` takes the channel for `Posix::net` and closes anything
   else; a process with no such message has `net == None`, and its first call that names a destination fails with
   `EACCES` (ADR-0225 item 2). `ZPB1` is unchanged and no slot is reserved. *This differs from the plan's "`NET` in
   `ZPB1`"; it needs no record version, and the connection cannot be mistaken for a table entry.*
2. **The shell gives it** from a literal `url` operand of a POSIX program the image describes (ADR-0224 item 2, ADR-0286
   item 5): the description `user/describe/posix/zfetch.cmd` names the operand, `zosh` makes `narrow` from its own
   connection (right `reach`, one rule for the host and port) and sends the result in that message. A program the image does
   not describe, or a line without the operand, gets none. The `+net` words are still not built (ADR-0286 item 5), so a
   POSIX program cannot be given a network by a grant on the line yet.
3. **A child gets no network.** A POSIX program's `spawn` sends `ZPBS`, so the child has no connection; a socket fd named in a child's fd map is
   `EBADF`; `fork` while a socket is open is `ENOTSUP` (ADR-0225 item 9's three-handle inheritance is not built).
   A pipeline of programs that each open their own socket works only for programs the shell started itself.
4. **The surface built**: `socket` (`AF_INET`, `SOCK_STREAM`, protocol 0 or TCP, `SOCK_NONBLOCK`, `SOCK_CLOEXEC`),
   `connect`, `send`, `sendto` (the address is ignored, as TCP does), `recv`, `recvfrom`, `shutdown`, `setsockopt`,
   `getsockopt`, `getsockname`, `getpeername`, `getaddrinfo`, `freeaddrinfo`, `gai_strerror`, `htons`, `ntohs`, `ntohl`
   (`htonl` was a tier row), `inet_pton`, `inet_ntop`, `inet_aton`, `inet_addr`, `inet_ntoa`; `read`, `write`, `close`,
   `fstat` (`S_IFSOCK`), `fcntl` (`O_NONBLOCK`) and `select` on a socket fd. They are the block at the end of this ADR,
   which `posix-surface-check` reads beside the tier list. `socket` and `getaddrinfo` leave the check's deny list as
   ADR-0225 item 1 says; ADR-0005 keeps its text until the architect adds its superseding note.
5. **Options**: `TCP_NODELAY`, `SO_KEEPALIVE`, `SO_REUSEADDR` are kept in the library and read back; the buffer sizes
   read as the ring; `SO_RCVTIMEO`/`SO_SNDTIMEO` are accepted and **not honoured**; `SO_ERROR` finishes a pending connect.
   `getsockname` gives `0.0.0.0:0` (`netd` has no `local` request). `MSG_PEEK` and `MSG_OOB` are `EOPNOTSUPP`.
6. **`inet_pton` and `getaddrinfo` read an address strictly** (four decimal numbers, no leading zeros): `127.1`,
   `0177.0.0.1` and `1.2.3` are not addresses there and, ending in digits, not names either, so they are `EAI_NONAME`
   without a request; `inet_aton` and `inet_addr` keep the historical forms for programs that call them. A name that is
   not an address by the scope's grammar (`0x7f000001`) is a name and goes to `netd`, whose resolver says no such name.
7. **`getaddrinfo` returns `AF_INET`, `SOCK_STREAM`, TCP**, one `addrinfo` per address (at most 8) in one `malloc` block
   with its `sockaddr_in` and the canonical name; `freeaddrinfo` walks the list. Errors are musl's `EAI_*` numbers.
8. **Waiting**: `connect` blocks with the process's `wait` on the connection channel (a wait point, `EINTR` retried
   by the library as for a file read); `select` waits through the port on the control channel while connecting, on the
   read end for reads, on the write end for writes (ADR-0225 item 8).
9. **Not built**, each with `ENOSYS` or its symbol absent: `bind`, `listen`, `accept`, `accept4`, datagram sockets,
   `socketpair`, `poll`, `sendmsg`, `recvmsg`, `getnameinfo`, `gethostbyname`, `getservbyname`, `if_nametoindex`,
   `getrandom` (ADR-0227), the REPORT message and hint of ADR-0224 rule 5, inheritance through `spawn` and `fork`,
   curl and wget (W11 to W13). INV-0225-1 (the report) and INV-0225-4, -5 stay open.

```posix-surface-added
socket
connect
send
sendto
recv
recvfrom
shutdown
setsockopt
getsockopt
getsockname
getpeername
getaddrinfo
freeaddrinfo
gai_strerror
htons
ntohs
ntohl
inet_pton
inet_ntop
inet_aton
inet_addr
inet_ntoa
```

## Consequences
- **Positive.** A C program fetches a page with the usual calls; the grant is the native scope; no byte of a page is a
  message or touches the broker; every parser of an address from a program is total and host tested.
- **Negative.** Nothing that listens, sends datagrams or polls; no network for a child; a timeout option that does
  nothing; the surface rows are admitted from a proposed ADR (the checker prints a note).
- **Follow-up.** `listen`/`datagram` in `netd`, then the rest of ADR-0225; `poll`; the three-handle inheritance; the REPORT
  hint; curl as a port with BearSSL (N5).

## Invariants
- **INV-0287-1.** Every handle a socket fd holds is closed with it, and a `connect` that fails leaves the fd with none.
  *Check:* `cargo test -p zoraya-posix --test net` (closing and refused connects, handle counts).
- **INV-0287-2.** Every status and reason that `netd` gives has an `errno` row, and a value outside the table is `EIO`.
  *Check:* `cargo test -p zoraya-posix` (`socket` tests and the connect tests of `tests/net.rs`).
- **INV-0287-3.** A POSIX program and a native one fetch the same page with the same bytes: the checksum `zfetch --sum`
  prints is `fetch --sum`'s for the same URL. *Check:* `make test-net-posix`.

## Open questions
- Whether the connection should become a record entry when a second kind of connection (a listener's) arrives.
- `SO_RCVTIMEO`: a library deadline needs the wait with a timeout on the connection channel (`wait_any` has one).
- The cost of the C path against `fetch`: `docs/design.md` has the first numbers.

## Revisit triggers
- `netd` gains `listen` or `datagram`: build ADR-0225 items 4, 5, 9 and the rest of the surface.
- A program in the workloads needs `poll`: item 9 reopens with a workload behind the row.

## Architect's decision

(Left empty for the architect.)
