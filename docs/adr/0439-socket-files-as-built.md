# ADR-0439: Socket files as built (plan part F7): a node in the line's `/tmp`, a listener channel, a connect that forwards a pair's far end

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0050, ADR-0055, ADR-0107, ADR-0153, ADR-0225, ADR-0262, ADR-0341, ADR-0412, ADR-0430, ADR-0433
- **Evidence:** CP-01, CP-02, GR-008

## Context

ADR-0412 item 5 (accepted) says `bind(AF_UNIX, PATH)` succeeds only in a program's own directory (ADR-0341) or its line's `/tmp`
(ADR-0107), where the file service keeps a transient socket object holding a listener channel; `connect(PATH)` asks the service,
which makes the new pair and sends one end to the listener; `accept` takes it; the object lives in the service's memory only and
disappears with `unlink`, with its listener, or with the service. ADR-0430 item 8 left `bind`, `listen`, `accept` and a path for
`connect` to plan part F7, and INV-0412-3 (`bind` only inside those places) as `not yet applicable`. Building it needed choices that
ADR-0412 does not make: which service holds the node and how, where the pair is made, what the service reads, what the listener
trusts, what a full queue is, and what stays of the node when the listener goes. The code is `crates/zoraya-posix/src/unix_file.rs`,
the socket nodes of `crates/zoraya-fs-service`, the messages 41 to 43 of `zoraya-proto-file`. **Measured:** the host tests
(`crates/zoraya-fs-service/tests/socket.rs`, `crates/zoraya-posix/tests/socketfile.rs`) and `t_unix file` in QEMU. **Assumed:** a
program that needs a socket file outside `/tmp` (tmux's `~/.local/state/tmux`) is ported with a path inside `/tmp` or waits for the
own-directory part below.

## Decision drivers

1. **No ambient authority** (CP-01): a socket is reached only through a directory the connecting program's own table names, and
   connecting is a write on it.
2. **Descriptors stay handles** (CP-02, GR-008): a connection is the far end of a new pair, moved to the listener; the library makes
   both ends, so the service needs no knowledge of socket types.
3. **A service that cannot be made to hold what a hostile program chooses**: bounds on nodes and listeners, no state that grows
   with a loop of `bind` and `unlink`, every handle it takes closed on every refusal.
4. **The least that ADR-0412 accepted**, and each difference from it stated.

## Considered options

### Option A: the node is a file of the temporary directory service with the type bits kept in the service, the listener is a channel the service holds

`bind` is an exclusive create with `S_IFSOCK` (as `mkfifo` is with `S_IFIFO`, ADR-0107 item 13); the service keeps the node's id in
a table. `listen` sends the service one end of a channel, held for the connection that sent it. `connect` opens the node with the
`WRITE` right and sends the far end of a new pair, which the service writes, unread, to the listener's channel.

### Option B: the service makes the pair and `connect` asks for one end

The service would know the socket types and make streams and channels for each connection, charged to its own account.

### Option C: a rendezvous object in every file service, including the volume's (ADR-0412's Option B as written)

`fsd` would show virtual directory entries that are not on the volume, in `readdir`, `stat`, `rename` and the sealed tree.

### Option D: the baseline, as Linux has it

A socket file anywhere a program may write, persisting until unlinked, any number of senders for datagrams.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **Where.** `bind` works in a table entry of kind `Tmp` (the line's `/tmp`, served by `fsd --memory`) and nowhere else; in another
   entry of the table it is `EACCES`, for the entry's root itself or a name that ends in a slash `EINVAL`, outside every entry the
   resolver's refusal (`ENOENT` and the report of ADR-0153 rule 11). **Different from ADR-0412 item 5:** a program's own directory
   (ADR-0341) is not built. The volume's service would have to show a node that is not on the volume, and the bootstrap record has no
   kind for "own directory" (a `Directory` entry is any granted directory, and a rule that picked the own one out would need the
   shell's planner, which stage 10 owns). INV-0412-3 holds with the narrower place.
2. **The node.** A file of the `/tmp` service whose mode the service reports with `S_IFSOCK` while its id is in the service's table
   (`stat` and `fstat` say socket; `open(2)` of it is `ENXIO`; `readdir` shows it as a plain name, a limit). The table holds at most
   256 nodes; a `bind` past it is `ENOMEM`. `unlink` and a `rename` over the name free the entry (a node with a live listener stays,
   unnamed, until its listener goes); a node nobody listens on costs no handle. **Different from ADR-0412 item 5:** a node is not
   removed when its listener goes. It stays, inert, until `unlink`, as a socket file does on Linux: `connect` is `ECONNREFUSED`,
   `bind` of the name `EADDRINUSE`. Nothing is written to a volume (the service is memory only).
3. **Listening.** `listen` on a bound `SOCK_STREAM` or `SOCK_SEQPACKET` socket makes a channel and sends the service one end
   (`socket_listen`, message 41, needs `WRITE`); the service holds it, charged as one handle to the connection's account, for as long
   as the connection that sent it lives. Only the connection that made the node may send it (`EACCES` for any other with
   `WRITE` on the file), and when that connection closes the node can never listen again: another program cannot win the race
   between `bind` and `listen` or take an inert node over. The connection is the one `bind` made and the socket keeps, so closing the socket (or the
   process ending) ends the listener, and no event wiring is needed. One listener per node, one node per connection (`EADDRINUSE`
   by the status `EXISTS`), 64 listeners in a service (`ENFILE`); a datagram socket cannot be bound (`EOPNOTSUPP`). `backlog` is a
   hint: the queue is the channel's 64 messages (ADR-0050), and the 65th connect is `EAGAIN` (Linux blocks; a blocking `connect` here
   is `EAGAIN` too).
4. **Connecting.** `connect(path)` resolves the path in the connecting program's own table, opens the node with `WRITE`
   (`EACCES` without it, `ENOENT` for a path the table does not cover or a name that is not there), makes the pair, and sends the
   far end's handles with a 24-byte record of what it is (message 42 for one channel end, 43 for a stream's three) on the node's
   connection. The service checks the right, finds the listener and the type, and writes the record and handles to the listener's
   channel in one message; it reads nothing of the record. A node with no listener, or a file that is no socket, is
   `ECONNREFUSED`; a type that is not the listener's `EPROTOTYPE`; a full queue `EAGAIN`; a listener whose channel is gone is
   dropped and the connect `ECONNREFUSED`. On every refusal the far end's handles are closed, by the library's write or by the
   service's loop.
5. **Accepting.** `accept` and `accept4` read the listener channel. A message is taken only if `unix::decode_connect` accepts it: 24
   bytes, a fresh pair's record of the listener's own type (a stream with its three handles, offsets 0, not shared; a packet socket
   with one), the handle count the record says. Anything else, from any program that holds the file with `WRITE`, is dropped and its
   handles closed (INV-0430-1's rule). The new socket is connected, has the listener's name and no peer name; its flags are
   `accept4`'s (`SOCK_NONBLOCK` and `SOCK_CLOEXEC`), and whether the call waits is the listener's own `O_NONBLOCK`. A full table is
   `EMFILE` with the connection still queued. A listener whose channel was closed from the service's side is `ECONNABORTED`.
6. **Names.** `getsockname` of a bound or accepted socket is the path as the program gave it (`struct sockaddr_un` with a NUL);
   `getpeername` of a connected socket is the path it connected to, and empty for an accepted one; `SO_ACCEPTCONN` is 1 on a
   listener. Poll: a listener is readable when a connection waits and never writable.
7. **What is not carried.** A listening socket is not sent with `SCM_RIGHTS` (`EINVAL`), not inherited by `spawn` or `execve` (`EBADF`: a Unix socket the file actions do not list fails the spawn, as ADR-0433 item 6 says, and a listed listening socket is `EINVAL`), and `fork` with one is `ENOTSUP`: its listener is tied to the connection the library
   holds. A bound socket that does not listen is the same.
8. **TCP.** `bind`, `listen` and `accept` on an `AF_INET` socket are `EOPNOTSUPP` (the symbols exist so that programs link and say so
   at run time); ADR-0440 records what listening over `netd` needs.
9. **The names** (ADR-0342 item 1):

```posix-surface-added
bind t_unix
listen t_unix
accept t_unix
accept4 t_unix
```

### Effect of the vision

For target 2 a socket file is a capability inside one line's `/tmp`: a program with no `/tmp` entry, or with `/tmp` read-only, cannot
connect; the listener checks what arrives and the service reads nothing, so a program with `WRITE` on the file can fill the
64-message queue (`EAGAIN` for the next); and, because the service's tables are the line's, take all 64 listener slots (`ENFILE` for
every other program), fill the 256 nodes (`ENOMEM` until something is unlinked) and make the service's job (`tmp_job`, which has no
limit of its own) pay for up to 64 x 64 queued connections, where the connector pays only for its pair. A socket name authenticates
nothing: any program of the line that can write `/tmp` can `unlink` or `rename` a live server's name and `bind` its own (a
listener whose name was unlinked between `bind` and `listen` gets `EPROTOTYPE`, where Linux succeeds). Another line's `/tmp` is
another service.

## Consequences

- **Positive.** `AF_UNIX` servers (a Python `socketserver` on a Unix socket, a `multiprocessing` listener, later tmux) can run between two
  programs of a line; the service has no new account, no event, and one table of 256.
- **Negative.** No own-directory sockets (tmux's default place); a node stays until `unlink` after its server dies; no datagram
  `bind`; the accept queue is 64; `readdir` shows no socket type.
- **Follow-up.** The own-directory place once the record can say which entry is a program's own (stage 10's planner) and the volume's
  service can hold a transient node; datagram sockets with a name; ADR-0440.

## Invariants

- **INV-0439-1.** A socket node of the `/tmp` service is reached for `listen` and `connect` only through a connection with `WRITE`
  on it, a connect to a node with no listener, a file that is not a socket or a listener of another type is refused with the
  connector's handles closed, and the service closes every handle a refused request carried. *Check:* `cargo test -p zoraya-fs-service
  --test socket` (rights, types, a full queue, a dead listener) and `t_unix file` in QEMU.
- **INV-0439-2.** The service's tables of socket nodes and listeners are bounded (256 and 64), a loop of `bind` and `unlink` leaves
  them as they were, and a listener's handle is charged to its connection's account and released when the connection closes. *Check:*
  `cargo test -p zoraya-fs-service --test socket` (the bounds, `unlink` and `rename` over a node, the handle count after closing).
- **INV-0439-3.** A listener takes from its channel only a record of its own socket type, a fresh pair's, with the handle count it
  says; any other message is dropped and its handles closed, and no byte of it is trusted. *Check:* `cargo test -p zoraya-posix`
  (`unix::tests`, `a_connection_record_decodes_only_in_the_form_its_listener_takes`, and a totality sweep over lengths and byte
  patterns in `tests/socketfile.rs`: no panic, and whatever decodes has the handle count it says).

## Open questions

- **For the architect.** Whether the own-directory place is wanted soon enough to change the bootstrap record (a kind for "own") and
  give `fsd` a transient node, or whether `/tmp` alone is enough until tmux is ported.
- A socket file is shown to `readdir` as a plain file (`DT_REG`): the service's listing would need the kind.
- A listener whose process stopped but whose descriptors another process holds (a `dup`ed fd shares the description, so the listener
  lives): the same as Linux.

## Revisit triggers

- Channel endpoints become duplicable (ADR-0412's trigger): a listener could be inherited by `fork` and `spawn`.
- A port that needs a socket file in its own directory or a datagram socket with a name.

## Architect's decision
