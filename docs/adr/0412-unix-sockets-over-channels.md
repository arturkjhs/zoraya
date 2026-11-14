# ADR-0412: Unix sockets in `libposix` over channels: `socketpair`, `SCM_RIGHTS` as handle transfer, no socket file outside a program's own places

- **Status:** proposed
- **Date:** 2026-11-14
- **Depends on:** ADR-0050, ADR-0101, ADR-0103, ADR-0107, ADR-0225, ADR-0261, ADR-0287, ADR-0315, ADR-0341, ADR-0342, ADR-0410, ADR-0411
- **Evidence:** CP-01, CP-02, DM-019, DM-022, DM-074, GR-001, GR-008, alternatives-postmortem.md#haiku--beos, browser-engine/survey.md#how-other-systems-got-a-browser, browser-engine/survey.md#ladybird, browser-engine/survey.md#libraries-and-interfaces, browser-engine/survey.md#measurements, browser-engine/survey.md#processes-handles-and-channels, browser-engine/survey.md#the-posix-library, browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach

## Context

WebKit's IPC is a `socketpair(AF_UNIX, SOCK_SEQPACKET)` per helper; each message carries a header and an inline body
of at most 4096 bytes, bigger bodies through shared memory, and up to 254 descriptors in one `SCM_RIGHTS` message; a
send that would block waits in `poll(POLLOUT)` (browser-engine/survey.md#measurements). Ladybird uses a
`SOCK_STREAM` pair, `SCM_RIGHTS` and frames of its own (browser-engine/survey.md#ladybird). GIO's `GSocket` wraps
the descriptor. Beyond the browser, `socketpair` is CPython `asyncio`'s self-pipe, and tmux binds a named Unix socket.

Zoraya has none of it (browser-engine/survey.md#the-posix-library). ADR-0225 decided `socketpair(AF_UNIX,
SOCK_STREAM)` as two crossed byte streams and refused `AF_UNIX` with a path; ADR-0342 item 11 admits
`sendmsg`/`recvmsg` "without ancillary data". The kernel has what the engine needs underneath: a channel carries up to
64 KiB and 64 handles a message, keeps message boundaries, signals readable, writable and peer-closed, and charges the
writer (browser-engine/survey.md#processes-handles-and-channels). Channel endpoints cannot be duplicated, and an
endpoint with handles queued in it cannot be moved (ADR-0050).

## Decision drivers

1. **Descriptor passing is handle transfer** (CP-02, GR-008; descriptors stay one uniform handle, GR-001): rights travel with the object and only decrease; nothing
   new for the kernel. DM-022 records `SCM_RIGHTS`' in-flight garbage collection bugs on Linux; channels refuse the
   cycles that cause them (ADR-0050).
2. **No global names** (CP-01): System V's separate key namespace is the mistake to avoid (DM-019); a socket file in a
   shared directory is a global rendezvous.
3. **The engines unchanged at this layer.** Both use plain `socketpair`, `sendmsg`, `recvmsg` and `poll`; matching
   them keeps the port's patches to the launcher.
4. **Other ports profit** (the plan): CPython's `asyncio`, `multiprocessing`, tmux.
5. **The BSD socket API's sharp edges** (DM-074) stay where the standard puts them; no new ones.

## Considered options

### Option A: Unix sockets as Linux and FreeBSD have them, socket files anywhere

- **Description:** Stream, datagram and sequenced-packet sockets, `bind` to any path, the abstract namespace,
  `SCM_RIGHTS` and `SCM_CREDENTIALS` (browser-engine/survey.md#libraries-and-interfaces).
- **Evidence for:** Everything ported works unmodified.
- **Evidence against:** A path in a shared directory is ambient rendezvous (driver 2); credentials mean nothing with one
  identity (ADR-0104); the abstract namespace is a global name space by definition.
- **Cost:** *(estimate)* As Option B plus a rendezvous object in every file service.
- **Precedent:** Linux, FreeBSD.

### Option B: `AF_UNIX` in `libposix` over channels; `socketpair` and descriptor passing; socket files only in a program's own directory or its line's `/tmp`

- **Description:** A Unix socket descriptor is one channel endpoint. `socketpair` makes a channel. `SOCK_SEQPACKET` and
  `SOCK_DGRAM` send one channel message per `sendmsg`; `SOCK_STREAM` sends bytes in messages and a reader may take
  them across message boundaries. `SCM_RIGHTS` turns each descriptor into the handle (or connection) behind it, sent
  in the same message. `bind` with a path works only under a program's own directory (ADR-0341) or its line's `/tmp`
  (ADR-0107), where the file service keeps a rendezvous object that hands out channels; elsewhere `EACCES`.
- **Evidence for:** Drivers 1 to 5; no kernel change; Haiku's WebKit2 port moved from native messaging to Unix sockets
  because it cut maintenance (alternatives-postmortem.md#haiku--beos,
  browser-engine/survey.md#how-other-systems-got-a-browser).
- **Evidence against:** Deviations the engines may notice: a socket end given to another process leaves the sender
  (endpoints cannot be duplicated); 64 descriptors a message, not 253; a message of at most 64 KiB; no
  `SCM_CREDENTIALS`.
- **Cost:** *(estimate)* 900 lines in `libposix` with host tests; 300 in `fsd`/`tmpd` for the rendezvous.
- **Precedent:** Fuchsia, where what passes between processes is a handle moved through a channel with its rights
  (browser-engine/survey.md#libraries-and-interfaces).

### Option C: Rewrite the engine's IPC over native channels (a `ConnectionZoraya` beside `ConnectionUnix`)

- **Description:** WebKit's `IPC::Connection` gets a Zoraya backend that calls the channel ABI directly, as Mojo has
  per-OS platform channels (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach) and WebKit has a Darwin
  backend over Mach ports.
- **Evidence for:** No POSIX emulation in the path; handles typed end to end.
- **Evidence against:** A second IPC backend to keep in step with upstream; GLib's event source and GIO's `GSocket`
  still need a descriptor to poll; every other port (Ladybird, CPython) gets nothing; Chromium dropped its Mach-port
  IPC because the native primitive did not join the event loop (browser-engine/survey.md#why-blink-and-gecko-are-out-of-reach).
- **Cost:** *(estimate)* 1,500 lines inside WebKit, carried at each upgrade.
- **Precedent:** Mojo on Fuchsia and Windows; WebKit's Darwin backend.

### Option D: No socket files at all; `socketpair` only

- **Description:** Option B without `bind`.
- **Evidence for:** The smallest surface; nothing for the engine needs `bind`.
- **Evidence against:** tmux and other servers of the plan need a name to attach to.
- **Cost:** *(estimate)* 900 lines.
- **Precedent:** ADR-0225 as accepted.

## Recommendation

**Recommendation (not a decision): Option B.** It changes ADR-0225 item 10 (`socketpair` becomes a channel, not two
crossed streams) and ADR-0342 item 11 (`sendmsg`/`recvmsg` carry `SCM_RIGHTS`), and narrows ADR-0225's refusal of
paths (refused outside the two places below).

1. **The descriptor.** `socket(AF_UNIX, type)` with `SOCK_STREAM`, `SOCK_SEQPACKET` or `SOCK_DGRAM` (and
   `SOCK_CLOEXEC`, `SOCK_NONBLOCK`) gives an unconnected descriptor; `socketpair` gives a connected pair, one channel.
   `AF_LOCAL` is the same.
2. **Messages.** `SOCK_SEQPACKET`/`SOCK_DGRAM`: one `sendmsg` is one channel message of at most 65,536 bytes
   (`EMSGSIZE` above); a `recvmsg` with a shorter buffer truncates and sets `MSG_TRUNC`, as Linux does. `SOCK_STREAM`:
   a `send` of more than 65,536 bytes is several messages; a `recv` returns what is queued up to its length, keeping a
   partly read message in the library; ancillary data is delivered with the first byte of its message.
3. **`SCM_RIGHTS`.** At most 64 descriptors a message (`ETOOMANYREFS` above, the Linux error). Each descriptor becomes
   its handle with the descriptor's rights: a file or directory a fresh connection to the same description (ADR-0103's
   `share`), a pipe or FIFO end its stream handle, a memory descriptor (ADR-0413) its memory object (read-only if the
   descriptor is), a Unix socket its channel endpoint. A Unix socket end or anything else that cannot be duplicated is
   **moved**: the sender's descriptor is closed by the send (a stated deviation). A TCP socket, a terminal, an `eventfd`
   or `/dev/null` is refused with `EINVAL` before anything is sent. A channel endpoint with handles queued in it is
   refused with `EBUSY` (ADR-0050's rule). Received descriptors take the lowest free numbers, close-on-exec if
   `MSG_CMSG_CLOEXEC`; if the table is full the message stays queued and `recvmsg` is `EMFILE`.
4. **Not offered.** `SCM_CREDENTIALS`, `SO_PASSCRED` and `SO_PEERCRED` (one identity, ADR-0104); the abstract
   namespace (`EAFNOSUPPORT`); `MSG_OOB`, `MSG_PEEK` on streams beyond the queued bytes (`EOPNOTSUPP`).
5. **Socket files.** `bind(AF_UNIX, PATH)` succeeds only where `PATH` is in the program's own directory (ADR-0341) or
   its line's `/tmp` (ADR-0107); the file service makes a socket object there (`S_IFSOCK`) holding a listener channel;
   `connect(PATH)` asks the service, which sends a new channel's end to the listener; `accept` takes it. Anywhere else
   `bind` is `EACCES` and the refusal is reported as other path refusals are. A socket object is removed by `unlink` or
   when its listener closes.
6. **Inheritance.** A Unix socket listed in a spawn's file actions moves to the child (driver: endpoints cannot be
   duplicated); `fork` with an open Unix socket is `ENOTSUP`, as with TCP sockets today (ADR-0287).
7. **Waiting and charges.** Readiness is the channel's readable, writable and peer-closed signals in `poll` (ADR-0414);
   blocking reads and writes are ADR-0315 long waits; a full queue is `EAGAIN` for a non-blocking send. The writer pays
   for queued messages (ADR-0261); a peer that does not read blocks its writer at 64 messages or 256 KiB.

### Effect of the vision

For target 2 a socket is a capability: an agent's program can talk to a server only if it was handed the socket or can
name a socket file inside its own places, never through a shared directory. A supervisor sees `S_IFSOCK` objects only
in a program's own directory. Target 3: tmux and Python's `asyncio` work. Networking: unchanged (`AF_INET` stays
`netd`'s). Threads: blocking socket calls are long waits.

### Examples

```
$ python3 -c "import asyncio; print(asyncio.run(asyncio.sleep(0, 'ok')))"
ok
$ tmux new -s work           # binds ~/.local/state/tmux/default inside tmux's own directory
$ tmux attach -t work        # from another line of the same room
```

A program that tries `bind("/home/me/sock")` gets `EACCES` and the shell shows
`refused: /home/me/sock (not this program's own directory)`.

## Consequences

- **Positive.** WebKit's and Ladybird's IPC run as written; CPython's `socketpair` users and tmux follow; descriptor
  passing is the kernel's handle transfer.
- **Negative.** A sent socket end leaves the sender; 64 descriptors and 64 KiB a message; a rendezvous object in two
  file services; two accepted ADRs change in a stated way.
- **Follow-up.** `libposix`'s Unix socket module, `fsd`/`tmpd`'s socket objects, a `posix-surface-added` block, an
  in-QEMU test with descriptors of every kind.

## Invariants

- **INV-0412-1.** A descriptor sent with `SCM_RIGHTS` arrives as a descriptor of the same kind whose rights are those of
  the sent descriptor or fewer, and a refused send changes nothing at either end. *Check:* `cargo test -p zoraya-posix`
  (a fake kernel; every kind of descriptor, full tables, endpoints with queued handles) and an in-QEMU C test that
  passes each kind between two processes and uses it.
- **INV-0412-2.** `SOCK_SEQPACKET` and `SOCK_DGRAM` keep message boundaries, and `SOCK_STREAM` delivers the bytes in
  order with nothing lost or repeated, for any split of sends and receives. *Check:* a host property test over random
  send and receive sizes.
- **INV-0412-3.** `bind` of an `AF_UNIX` path succeeds only inside the program's own directory or its line's `/tmp`.
  *Check:* `cargo test -p zoraya-posix` (the path rule) and an in-QEMU script binding inside and outside both.

## Open questions

- Whether WebKit ever sends more than 64 descriptors in one message (its limit is 254); the first engine part counts
  them in a debug build.
- Whether a moved socket end breaks a pattern in GLib or CPython (sending a socket and keeping it); found by their tests.

## Revisit triggers

- A port that needs `SCM_CREDENTIALS` or peer identity: identity is a decision of its own.
- Channel endpoints become duplicable: the move of item 3 becomes a copy.

## Architect's decision
