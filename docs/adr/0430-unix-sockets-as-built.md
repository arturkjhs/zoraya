# ADR-0430: `AF_UNIX` sockets as built (plan part F2): the wire form, the limits where ADR-0412 leaves a number open, what `fork` and `spawn` do, and the seven names

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0050, ADR-0101, ADR-0102, ADR-0103, ADR-0287, ADR-0315, ADR-0342, ADR-0398, ADR-0412, ADR-0414
- **Evidence:** CP-02, DM-022, GR-008, browser-engine/survey.md#the-posix-library

## Context

ADR-0412 (accepted 2026-11-14) decides `AF_UNIX` over channels: a stream pair is two crossed byte streams plus an ancillary
channel, a sequenced-packet or datagram pair is one channel, `SCM_RIGHTS` is handle transfer. Plan part F2 builds items 1 to 4,
6 and 7. The code needed choices that ADR-0412 leaves open: how a descriptor looks on the wire, what a limit counts, which
failure leaves what, and what `fork` and a spawn do with a socket (browser-engine/survey.md#the-posix-library lists the
surface that did not exist). None is against ADR-0412. This ADR records them so that none is silent, carries the
`posix-surface-added` block (ADR-0342 item 1: a part that builds a function before a port imports it names its committed
test program), and names what this part leaves for F4 and F7. **Measured:** the host tests of
`crates/zoraya-posix/tests/unix.rs` and the in-QEMU cases of `tests/posix-core/t_unix.c` (recorded in `docs/design.md`).
**Assumed:** the engines' messages stay below the limits of item 2 (ADR-0412's open question counts them).

## Decision drivers

1. **A refused send changes nothing at either end** (INV-0412-1), and a descriptor never lands in two places or in none
   (GR-008: rights travel with the object and only decrease, CP-02).
2. **A method that waits is run again from its start** (ADR-0315 item 9): nothing may be left half done at a wait.
3. **The kernel's channel is the whole transport** (ADR-0050): 64 KiB and 64 handles a message, 64 queued messages, no
   duplicable endpoints, and the cycle rule that DM-022's `SCM_RIGHTS` garbage collection needed a collector for.
4. **Fail closed where a thing is not built** (ADR-0287 driver 4): an `errno`, never a description with no handles.

## Considered options

### Option A: the choices below (the recommendation)

### Option B: the POSIX shape of Linux for each of them

Linux's own limits (253 descriptors, a message of any size up to the buffer, `SCM_CREDENTIALS`, a receive that always
succeeds past a full table), where the channel cannot carry them. Rejected for the limits the kernel fixes; kept for every
`errno` a Linux program tests.

### Option C: a new kernel call for descriptor passing

A `channel_write` that also reports why it refused, or a socket object in the kernel. Rejected: ADR-0412 chose no kernel
change; the existing all-or-nothing write (INV-0050-1) is enough once the library asks for it.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The wire form.** A descriptor is a 24-byte record (kind, access, socket type, flags, and for a stream socket its two
   byte offsets) with its handles beside the bytes in the channel message. Kinds: file, directory, pipe read end, pipe write
   end, stream socket, packet socket. A packet message is `count`, the records, then the payload. A stream's ancillary
   message is `ZAN1`, the stream offset of the first byte its descriptors belong to, `count` and the records. A decoder takes
   any bytes: a message that is not one of these, or whose handle count differs from its records', is dropped and its handles
   closed. A packet message carries `65,535 - 24 n` payload bytes with `n` descriptors (ADR-0412 item 2 says 65,536: the
   channel's 64 KiB holds the header too); more is `EMSGSIZE`.
2. **What the limit of 64 counts.** Handles, not descriptors: a file, directory, pipe end or packet socket is one, a stream
   socket up to three (read, write, ancillary). `ETOOMANYREFS` above 64 handles.
3. **What each kind becomes.** A file or directory: a new connection by `share` with the descriptor's rights
   (read-only stays read-only; a directory that arrives answers `fstat` and listing but not `openat`, which needs the sender's place in its namespace table: as a directory a spawned child inherits). A pipe end: a copy of the stream handle, the sender keeps its own (FIFOs travel as pipe
   ends; the service connection stays with the sender). A Unix socket: **moved**: the sender's description is left dead
   (a `dup`ed fd of it reads end of file and writes `EPIPE`) and the fd sent is closed by the send. A stream socket with
   descriptors or bytes held in the library, or a packet socket with a message held, is `EBUSY`. A terminal, `/dev/null`,
   a TCP socket and a socket with no peer are `EINVAL`. Memory descriptors join in plan part F3.
4. **All or nothing.** `Sys::channel_send` is the kernel's write with its INV-0050-1 behaviour (`channel_write` closes what
   it is given on failure, which a refused send must not): on any error the handles are still the caller's. Everything that
   can be known beforehand is checked before anything is made: the peer's state, room for the message, the descriptors'
   kinds. The service connections `share` made for files are closed again. **Left open:** a peer that closes between the check
   and the write makes the send `EPIPE` after the check; for a stream socket's descriptors the ancillary message is then
   queued toward a closed peer and its moved sockets are lost with it (a stated narrow window, as ADR-0412 item 3 says the
   move leaves the sender).
5. **Receiving.** A packet message with descriptors that finds the fd table without room stays queued (`EMFILE`, item 3 of
   ADR-0412), held in the library. A stream's descriptors are checked before the bytes are read. A control buffer too small for
   the descriptors closes the extra ones and sets `MSG_CTRUNC`; `read` and `recv` without a control buffer close what comes
   with the byte. **Stream offsets:** the library cuts a read at the next tag past its first byte, and keeps in the library
   any bytes it had already taken past a tag that arrived after it looked (ADR-0412 item 2); a descriptor whose byte a
   plain `read` took is closed.
6. **`fork`.** The parent marks every Unix stream socket shared after the snapshot is taken; the child marks them shared
   and drops the ancillary channel (a channel end cannot be duplicated, ADR-0102 item 3) and whatever the library held for it.
   `SCM_RIGHTS` on a shared socket is `EOPNOTSUPP` in both; a peer that still sends descriptors to a shared holder has them closed
   as they arrive, so that its ancillary queue never fills. A packet or datagram socket open in a `fork` is `ENOTSUP` (ADR-0412
   item 6). Bytes the library held for a socket go to the parent only.
7. **`spawn` and `exec` do not carry a Unix socket yet.** An open socket without `SOCK_CLOEXEC` makes them fail with `EBADF`,
   as a TCP socket does (ADR-0287 item 3). ADR-0412 item 6 moves a socket to a spawned child; that needs the bootstrap
   record to carry two or three handles for one fd, which is the record plan part F4 (`posix_spawn` with a grant list,
   ADR-0411 item 1) changes. **This part does not build it.** The consequence for the checks: the in-QEMU half of INV-0412-1
   ("each kind between two processes") runs in one process on the real kernel, and the two-process half is the host tests'
   over a fake kernel; F4 closes it.
8. **What is not built here.** `bind`, `listen`, `accept` and a path for `connect` are plan part F7's (ADR-0412 item 5):
   `socket(AF_UNIX)` gives a socket with no peer, `connect` on it is `ENOENT` and on a connected one `EISCONN`. INV-0412-3 stays
   `not yet applicable`. `getsockname` and `getpeername` of a Unix socket give the family alone (an unnamed socket, length 2).
   `SO_PEERCRED`, `SO_PASSCRED` and the other options of ADR-0412 item 4 are `ENOPROTOOPT`; `MSG_PEEK` and `MSG_OOB` `EOPNOTSUPP`;
   `MSG_WAITALL` is accepted and gives what a read gives.
9. **Shutdown and ends.** A stream socket closes its byte streams as TCP does. A packet socket has one channel and no half
   close: `shutdown(SHUT_WR)` ends its own sends with `EPIPE` and `SHUT_RD` its reads with end of file, and the peer sees
   nothing until the second half is shut or the fd closes (a limit; `SHUT_RDWR` closes the channel). A datagram socket whose
   peer has closed is `EPIPE`, where Linux says `ECONNREFUSED`.
10. **Readiness** (ADR-0414 item 1): a packet socket is its channel's signals, a stream socket the two streams'; bytes or a
    message the library holds are readable at once. A full packet queue (64 messages or 256 KiB) is not writable.
11. **The small calls that came with it** (the backlog's generic requests): `pread` and `pwrite` are the file protocol's
    `Read` and `Write` at an offset, in pieces of one request, leaving the position (a description opened `O_APPEND` writes at
    the offset, where Linux appends); `usleep` is `nanosleep` of microseconds. `mkdtemp` is musl's own code (`src/temp`, a `mkdir` over
    `__randname`), outside the tier list until now: the block names it.
12. **The names.**

```posix-surface-added
socketpair t_unix
sendmsg t_unix
recvmsg t_unix
pread t_unix
pwrite t_unix
usleep t_unix
mkdtemp t_unix
```

### Effect of the vision

Unchanged from ADR-0412: a socket is a capability, and a descriptor sent over it is the object's handle with its rights.
This part adds no authority: a program can pass only what it holds, and a refused send leaves its table as it was.

## Consequences

- **Positive.** `socketpair` and descriptor passing work for CPython's `asyncio`, `multiprocessing` over pipes of its own, and
  the engines' IPC once F4 lets a socket cross a spawn. Every refusal is an `errno` a Linux program tests.
- **Negative.** A packet message carries 24 bytes fewer than a Linux program may expect per descriptor; a moved socket is lost
  in the one window of item 4; a packet socket cannot be half closed; a socket cannot yet go to a child.
- **Follow-up.** Plan part F4 gives a spawned child a Unix socket (the bootstrap record) and closes the two-process half of
  INV-0412-1; F7 builds `bind`, `listen`, `accept` and INV-0412-3.

## Invariants

- **INV-0430-1.** A message carrying descriptors that a peer's library did not build (any bytes, any handle count) is dropped
  with its handles closed, and never panics the receiver. *Check:* `cargo test -p zoraya-posix` (`unix::tests` decoders over
  random bytes; `a_hostile_message_is_dropped_and_its_handles_closed`).
- **INV-0430-2.** A Unix socket's handles are closed exactly once: by `close` of its last fd, or by a send that moved it
  (the description left dead), including every descriptor and message the library still held for it. *Check:*
  `cargo test -p zoraya-posix` (`closing_a_socket_closes_what_it_held`, `a_dup_shares_the_description_and_a_moved_one_is_dead`).
- **INV-0430-3.** No `libc.a` call of this part waits with the library in a state it cannot repeat: the call is the same when
  run again after a deferred wait (ADR-0315 item 9). *Check:* an in-QEMU C case from a thread in a process with threads
  (`t_unix block`) and the library's own `PENDING` handling over each wait in `unix_io.rs`.

## Open questions

- The window of item 4 (a peer closing between the check and the write): closing it needs the kernel to report a closed peer
  from `channel_write` before it moves anything (it does: `PeerClosed` with no effect), and the library to treat that as a
  refusal with the moved sockets intact. The ancillary message of a stream is sent before its bytes, so a refusal there is
  already clean; a failure of the *bytes* after it is not. Measure whether a case exists in the engines' use.
- Whether a Unix socket in a `fork` child should keep working for packet sockets (copy the channel is impossible; an
  ancillary-less shared form is what a stream socket gets). The plan's browser launcher spawns, it does not fork.

## Revisit triggers

- A port that sends more than 64 handles in a message, or needs `SCM_CREDENTIALS` (ADR-0412's triggers).
- Channel endpoints become duplicable: a packet socket survives `fork`, and the move of item 3 becomes a copy.

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
