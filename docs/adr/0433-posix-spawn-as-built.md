# ADR-0433: `posix_spawn` as built (plan part F4): the shape of a child's table and fds, the grants, what crosses to it, and the names

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0042, ADR-0102, ADR-0103, ADR-0104, ADR-0107, ADR-0142, ADR-0287, ADR-0310, ADR-0341, ADR-0411, ADR-0412, ADR-0413, ADR-0430, ADR-0432, ADR-0434, ADR-0435
- **Evidence:** CP-05, DM-002, GR-005

## Context

ADR-0411 (accepted 2026-11-14) decides that `posix_spawn` takes a namespace list, a grant mask and an empty-table flag, so that a
child gets what its parent passes and nothing else. Plan part F4 builds them, with `posix_spawnp` and `addopen`. ADR-0412 item 6
and ADR-0413 item 3 decide that a Unix socket and a memory descriptor go to a spawned child; ADR-0430 item 7 and ADR-0432 item 5
deferred that to this part, because the bootstrap record has one handle for an fd. The code needed choices that those ADRs leave
open: how an entry is named and opened, what a child with no entry has as its working directory, how a connection that cannot
be duplicated is given, where the extra descriptors travel, what a refused spawn leaves. None is against ADR-0411. Two of
them need a change in a service, which is its own ADR (ADR-0434 for `netd`, ADR-0435 for `rngd`). This ADR records the rest and
carries the `posix-surface-added` block (ADR-0342 item 1). **Measured:** the host tests of `crates/zoraya-posix/tests/spawn.rs` and
`t_spawn` (`make test-libposix`), recorded in `docs/design.md`. **Assumed:** WebKit's launcher can pass a Unix socket, a 4 MiB
memory descriptor and a list of entries this way (the first engine part checks it).

## Decision drivers

1. **A child gets exactly what its parent passes** (CP-05, GR-005): every entry, fd and connection of a chosen child is named by
   the spawn, each no wider than the parent's, and the service, not the library, is what refuses a wider one.
2. **No descriptor leaks** (DM-002): a descriptor reaches a child only through a file action or by being inherited as before; an
   empty child inherits none.
3. **A refused spawn changes nothing**: every handle made is closed again, every parent handle that was to move is still its
   own (INV-0042-3), and no child is left.
4. **The old spawn is untouched** (ADR-0411 INV-0411-4): without an attribute of this part a spawn gives what it gave before,
   except for the descriptors that could not cross.

## Considered options

### Option A: the choices below (the recommendation)

### Option B: the table of the child chosen by the broker or the shell

The parent names a role and a launcher builds the table (ADR-0411 Option C). Rejected as the first step: a new request path from
a running program to the shell; ADR-0411 keeps it as a later hardening.

### Option C: Linux's `posix_spawn` as it is, plus Landlock

Children inherit everything and are narrowed after the start. Rejected by ADR-0411 Option A; it also leaves a Unix socket or a
memory descriptor of the parent to every child that does not set close-on-exec.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The attributes.** `POSIX_SPAWN_EMPTY_NAMESPACE` is `0x2000` in `posix_spawnattr_setflags` (below the sign bit, as
   `POSIX_SPAWN_TCSETPGRP`'s `0x4000`). `posix_spawnattr_setnamespace_np(attr, list)` stores the entries `PATH` or `PATH:ro` of a
   NULL-terminated list (a path is absolute and in normal form, a path with a colon is the path with it unless it ends in `:ro`;
   anything else is `EINVAL`); `posix_spawnattr_setgrants_np(attr, mask)` stores the mask of `POSIX_SPAWN_GRANT_NET` (1),
   `_RANDOM` (2), `_WINDOW` (4) and `POSIX_SPAWN_MOVE_NET` (8). Both live in the 56 bytes that musl's `posix_spawnattr_t` keeps
   after `__fn` (at 280 and 288; the list is a leaked box freed by `posix_spawnattr_destroy`). A list without the flag, a path
   listed twice, a mask with another bit, and `GRANT_NET` with `MOVE_NET` make the spawn `EINVAL` (the second also the setter).
   The constants and the three declarations are in musl's `<spawn.h>` by `ports/musl/zoraya/0005-spawn-zoraya.patch`, under
   `_BSD_SOURCE` as `addchdir_np` is.
2. **The table.** Without the flag the child has the parent's table as before. With it the child has the listed entries and no
   other. Each is opened *through the parent's own table* as a directory: `resolve` then the file protocol's open with
   `DIRECTORY` and the rights wanted, all of them for `PATH`, reading alone for `PATH:ro`, so that the file service refuses
   (`EACCES`) what the parent's connection could not give. A path no entry of the parent covers is `EACCES` at the spawn (and a
   refused-path report as for any open); a path that is a file is `ENOTDIR`, one that is not there `ENOENT`. `/dev/null`,
   `/dev/tty` and `/proc/self/exe` are listed by those names (a terminal is cloned for the child's group; `:ro` of a device is
   `EINVAL`). The record lists them longest first. A `fork` child, an `exec`ed image and a spawn without the flag are unchanged.
3. **The working directory.** A chosen table keeps the parent's working directory only if a listed directory covers it (the
   child's cwd is then the same place, relative to that entry); otherwise the child has none. The bootstrap record names none with
   the cwd entry `0xFFFF_FFFF` (`NO_ENTRY`) and an empty path, which `Bootstrap::decode` accepts for any table; the library
   takes a relative path of a child with none as one from `/` (it is refused unless the table has a `/`), and `getcwd` says `/`.
4. **The fds.** With the flag the child's fds are the targets of the file actions and nothing else: a `dup2` takes its source
   from the parent's table, a `close` of a number the child lacks does nothing. `posix_spawn_file_actions_addopen` opens the path
   *in the parent*, with the parent's table and authority, when the spawn is made (not in the child, whose table may not hold it),
   and the child's fd is that file; the temporary fd is closed once the spawn is over, on every path. The child gets a file
   outside its table in this way, as it would by `adddup2` of an fd the parent holds: the parent's act, named in the spawn.
   An `O_CLOEXEC` open gives the child no fd (it is opened, so its errors are the spawn's, and closed at the
   exec), and the entries are checked before `addopen` runs, so a spawn refused for a bad entry has made no file.
   `posix_spawn_file_actions_addclosefrom_np(fa, n)` (glibc 2.34's name, which CPython's `subprocess` asks for) removes every
   child fd from `n` up, at its place in the order of the actions.
5. **Connections.** The mask is applied in the parent: `GRANT_NET` asks `netd` to make a connection with the parent's own rights
   and scope (ADR-0434) and gives it; `MOVE_NET` gives the parent's own connection, which the parent no longer has after the
   spawn; `GRANT_RANDOM` asks the random service for a connection (ADR-0435); `GRANT_WINDOW` gives the window channel the
   parent still holds (a channel end cannot be shared), which the parent no longer has. A connection the parent lacks is
   `EACCES`; a service with no room is `EAGAIN`; a failure after the first connection was made closes it. The connections travel
   as ADR-0310 item 3 has them, in the second message `ZPBX`. A spawn without the mask, and every `exec`, send `ZPBS` and no
   connection.
6. **Unix sockets and memory descriptors.** A descriptor of either kind that the child's fd list names goes to the child in an extra
   bootstrap message, `ZPBF`, sent *before* the second message (`SELF`, ADR-0310's `ZPBX`): the records of ADR-0430 and ADR-0432
   (a record per descriptor, the same decoder), then each fd number. It is sent all or nothing (`channel_send`), and before
   the connections of item 5, so that a refusal of it (the kernel refuses a moved channel end that holds handles, or a full
   queue) leaves the parent its window and its network connection (INV-0433-2). **A Unix socket moves only if the spawn's file
   actions list it** (the target of a `dup2` or `addopen`, as ADR-0412 item 6 has it): one that is only inherited stays with
   the parent and the spawn is `EBADF`, as a TCP socket always was (so `system()` of a program that holds an open socket
   fails as it did, and moves nothing). **A listed socket moves whole, a stream socket too:** its byte streams and its
   ancillary channel go to the child, and the parent's descriptor is dead (`EPIPE`) until the parent closes it, as a
   packet socket's. **This is the point for the architect.** ADR-0412 item 6 says a stream socket's byte streams are
   *duplicated* to a spawned child and its ancillary channel moves, with `SCM_RIGHTS` `EOPNOTSUPP` in both. Both holders
   reading and writing one stream (a count of bytes for each, descriptors tagged by those counts) loses a descriptor
   or attaches it to the wrong byte, so this part moves the socket whole and no process shares a stream's counts. The cost: a
   parent that wants to go on talking over a stream socket it handed to a child must keep its own end of the pair (as
   it does with a socketpair). The alternative, ADR-0412's text, keeps the parent's streams and refuses `SCM_RIGHTS` in
   both; it is kept if the architect prefers. A stream socket a `fork` already shares goes as shared (no channel; the
   channel it left with the parent is closed). **A memory descriptor** goes as a copy of its object handle (read-only if the
   descriptor is), with its size and seals; the size is fixed in both from then (INV-0413-1), and it goes whether listed or only
   inherited (nothing of the parent's is taken). The same description named twice is `EINVAL`; a socket not connected, a TCP
   socket and one holding bytes or a message back are `EBADF` and `EBUSY` as before; a memory descriptor with no object is
   `EINVAL`. A close-on-exec descriptor stays behind as before. `exec` takes the same path.
7. **`posix_spawnp`** looks for a name without `/` in `$PATH` of the caller's own environment (default
   `/usr/local/bin:/bin:/usr/bin`, as musl's), going on past `ENOENT`, `ENOTDIR` and `EACCES`, and runs a file that is not an
   executable by `/bin/sh`, as `execvp` does. **The scheduling attributes** `posix_spawnattr_setschedparam` and
   `posix_spawnattr_setschedpolicy` are `ENOSYS`, as musl's, because CPython links them.
8. **The names.**

```posix-surface-added
posix_spawnp t_spawn
posix_spawn_file_actions_addopen t_spawn
posix_spawn_file_actions_addclosefrom_np t_spawn
posix_spawnattr_setnamespace_np t_spawn
posix_spawnattr_setgrants_np t_spawn
posix_spawnattr_setschedparam t_spawn
posix_spawnattr_setschedpolicy t_spawn
```

### Effect of the vision

Target 2: a child holds what its creator named, the service checks each entry against the creator's own, and no handle a
creator did not list reaches it. The browser's content process is a spawn with an empty table, `/usr/share/wpe:ro` and a socket.

## Consequences

- **Positive.** ADR-0411's launcher can be written; a Unix socket and a memory descriptor reach a child; CPython's
  `os.posix_spawn` and `subprocess` can use `posix_spawn` (`ports/Makefile` no longer lists it absent, and
  `ports/patches/python/0007-subprocess-posix-spawn.patch` turns it on for `sys.platform == "zoraya"`).
- **Negative.** A socket handed to a child is dead in the parent; `addopen` is the parent's open, not the
  child's; a child with no working directory reads a relative path as one from `/`; `GRANT_WINDOW` and `MOVE_NET` leave the
  parent without the connection; a refusal of the descriptors message after the child exists ends the child with status 127 (the parent keeps everything).
- **Follow-up.** The engine's launcher (ADR-0411 item 3); the positive network grant in an in-QEMU test, which needs a program
  the shell gives a network connection to.

## Invariants

- **INV-0433-1.** A child started with `POSIX_SPAWN_EMPTY_NAMESPACE` has exactly the entries listed (opened through the parent's
  table, each at the rights asked and no wider than the parent's), exactly the fds the actions name, and exactly the
  connections the mask names. *Check:* `cargo test -p zoraya-posix` (`tests/spawn.rs`, the entries, rights at the service, fds
  and grants) and `make test-libposix` (`t_spawn table`, `table2`, `refuse`, `grants`, `addopen`).
- **INV-0433-2.** A spawn that is refused leaves no handle made and changes nothing of the parent's table, connections,
  sockets or descriptors. *Check:* `cargo test -p zoraya-posix` (`tests/spawn.rs`, the handle counts after every refusal) and
  `make test-libposix` (`t_spawn refuse`).
- **INV-0433-3.** A Unix socket or a memory descriptor reaches a spawned child as the same kind with the rights of the parent's
  descriptor or fewer (a read-only memory handle stays read-only at the kernel), and a message of the third kind that a peer's
  library did not build is dropped with its handles closed. *Check:* `cargo test -p zoraya-posix` (`tests/spawn.rs`, two
  processes over the fake kernel, `unix::tests`) and `make test-libposix` (`t_spawn unix`, `memfd`).

## Open questions

- Whether `POSIX_SPAWN_GRANT_WINDOW` should share a window instead of moving it, once the compositor's channel can be
  duplicated.
- **Known limitations of the review (F4 notes).** `posix_spawnp` repeats the whole spawn, `addopen` and the grants included,
  for each `$PATH` directory, and goes on past an `EACCES` that came from an entry or a grant and not from the file;
  `me`, the process handle duplicate of the second message, leaked when its write failed (closed now); the addopen/entries
  order is fixed (entries first). The python lanes run nothing that starts a second interpreter through `subprocess`, so CI
  shows only that the port still builds and passes; `t_spawn` is the evidence for the spawn itself.
- The positive network grant is not in an in-QEMU test (the test guest's programs have no network connection); its library half is
  tested over a fake `netd` and `netd`'s half in `zoraya-net`.
- CPython: `subprocess` of a bare name (`['true']`) is not spawned by `posix_spawn` (CPython requires a directory in the
  name); a path (`['/bin/true']`) is. The port's tests were not run by this part.

## Revisit triggers

- A launcher that needs more than the table, fds and grants (Ladybird's process types): the table of ADR-0411 item 2 per type.
- Channel endpoints become duplicable: a packet socket is copied instead of moved, and `GRANT_WINDOW` shares.

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
