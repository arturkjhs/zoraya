# ADR-0319: Advisory record locks (`fcntl` `F_SETLK`, `F_SETLKW`, `F_GETLK`, `F_OFD_*`) in `libposix`, in the process

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0001, ADR-0005, ADR-0100, ADR-0103, ADR-0109, ADR-0302, ADR-0309, ADR-0315
- **Evidence:** DM-036, X-15

## Context

Part 9b is done when SQLite's threaded tests pass under HVF and TCG (TASK-27). `threadtest3` builds and starts, and
every scenario fails at its first write: SQLite's unix VFS takes `fcntl(F_SETLK)` locks on the database file for
SHARED, RESERVED, PENDING and EXCLUSIVE, and `libposix` answers `EINVAL` to `F_SETLK`, `F_SETLKW` and `F_GETLK`
(`SQLITE_IOERR_LOCK`, 3850). ADR-0005 item 6 and its table keep "record locks" out of the T0 to T2 surface because the
measured programs never call them (DM-036 calls POSIX locks a mistake of the interface); 9b's tests are the first
caller. `flock` stays excluded (INV-0005-2).

What SQLite does with them `[outside evidence, SQLite's `os_unix.c`]`: inside one process it keeps its own table of
locks per inode (`unixInodeInfo`), so that two connections of one process never ask the system for conflicting locks;
`fcntl` locks serve only to exclude other processes (and are released, by POSIX's rule, when any fd of the file is
closed, which SQLite works around by deferring closes). `F_GETLK` is used to ask "is there a reserved lock" when none
is known in the process. OFD locks are not used by default.

What exists: the processes of Zoraya each have their own connection to the file service (`fsd`); the service has no
notion of a lock; the library's state of a process (`Posix`) is shared by its threads under one lock (ADR-0302 item 8).

## Decision drivers

1. SQLite's unix VFS, with several connections in one process (`threadtest3`), must work and must not corrupt a
   database.
2. The least change that is sound: no protocol change without a review (CLAUDE.md "Models and loops").
3. The semantics POSIX gives where they are observable: owner is the process, not the thread; a process's locks do
   not conflict with each other; `close` of any fd of the file drops the process's locks on it; a `fork` child has none.
4. Honesty about what is not provided (X-15: explicit over implied).

## Considered options

### Option A: a table in the process, owner = process (POSIX) or description (OFD); no other process sees it
- **Description:** `F_SETLK`, `F_SETLKW` (made by polling), `F_GETLK` and `F_OFD_SETLK`, `F_OFD_SETLKW`,
  `F_OFD_GETLK` in `libposix`, over a table of `(volume, id, range, kind, owner)` in `Posix`. A POSIX lock conflicts
  only with an OFD lock (any process-owned lock is never in its own way); OFD locks conflict between descriptions and
  with POSIX locks of the same process, as on Linux. Closing an fd of a file drops the process's POSIX locks on it;
  closing the last fd of a description drops its OFD locks. A `fork` child starts with none (it is a copy of the parent's `Posix`, and `after_fork_in_child` clears the table); `exec`
  starts a new image with none.
- **Evidence for:** drivers 1 to 4 within one process. Everything SQLite needs of `fcntl` inside one process: its own
  inode table already orders its connections.
- **Evidence against:** a second process does not see the lock. Two processes that open one SQLite database are not
  excluded: **this is the documented limit**, and the reason the claim "SQLite works" is "with one process per database
  file".
- **Cost:** *(estimate)* 350 lines and host tests; no protocol change; one extra `Stat` request per lock call (and per
  `close` while the process holds a lock).
- **Precedent:** none exact; Fuchsia's fdio has no record locks; many embedded libcs make `F_SETLK` a successful no-op.

### Option B: `fsd` keeps the locks; or the broker does
- **Description:** a new request on the file connection (`Lock { owner, start, len, kind, wait }`) and an owner
  identity the service can tell apart per process and per description, released on close and on the process's death.
- **Evidence for:** the full semantics, including a second process and `exec` keeping locks; it is where Linux keeps them
  (the kernel, shared by all).
- **Evidence against:** a protocol change in `zoraya-proto-file` and the service, a notion of process identity at `fsd`
  (connections carry none, ADR-0103), release when a process dies (the connection closes: possible), a blocking wait in
  the service's loop (or a notification), deadlock detection; each needs a review of its own. More than this slice
  needs.
- **Cost:** *(estimate)* 3 to 5 times Option A, plus a security review of what one process can learn about another's
  files from `F_GETLK` (a lock holder's pid).
- **Precedent:** Linux, the BSDs, Redox (`flock` in the scheme).

### Option C: leave record locks unsupported and run SQLite on `unix-dotfile` or `unix-none`
- **Description:** the test selects another VFS.
- **Evidence for:** no change to the library.
- **Evidence against:** `unix-none` has no locking at all: two connections of one process then write the file at once
  and corrupt it, so the test would be passing for the wrong reason or failing; `unix-dotfile` locks by creating
  `NAME.lock`, which works in one process but is not what SQLite programs on Zoraya (ports, W2 and W3) use, so it tests
  another thing. Neither is acceptable as the evidence that "SQLite's threaded tests pass". Also, any program that
  calls `fcntl(F_SETLK)` (for example a pid file) would still get `EINVAL`.
- **Cost:** 0.
- **Precedent:** none.

## Recommendation

Option A, with Option B recorded as the follow-up when a measured program needs to exclude another process
(the architect's call).

1. **Commands.** `F_GETLK` 5, `F_SETLK` 6, `F_SETLKW` 7, `F_OFD_GETLK` 36, `F_OFD_SETLK` 37, `F_OFD_SETLKW` 38 (Linux
   numbers, as musl has them), on `struct flock` of musl's aarch64 ABI (32 bytes). `l_whence` 0, 1 and 2 (`SEEK_CUR` asks
   the file service for the position, `SEEK_END` for the size); a negative `l_len` covers the bytes before `l_start`;
   `l_len` 0 means to the end of the file and beyond.
2. **Scope.** The table is the process's. A lock is not seen by another process, and is lost across `exec` and not
   inherited by `fork` (POSIX: the child has none). A `fork` child is a copy of the parent's memory, not a new `Posix`:
   `after_fork_in_child` empties the table, OFD records included. Decision: Linux lets the child share the parent's
   descriptions, so an OFD lock would stay held for both, but the child's connections are clones with other handles, so
   a record keyed by the parent's handle would name nothing of the child's; the child has none, an honest limit. A file is identified by `(st_dev, st_ino)` from `Stat` on the
   connection.
3. **Conflicts.** Read locks share; a write lock excludes every other owner. One owner never conflicts with itself and
   setting a range replaces the owner's part there (split, merge, change of kind). A conflict of `F_SETLK` is `EAGAIN`;
   `F_GETLK` reports the first lock in the way as `SEEK_SET` bytes with the holder's pid (`l_pid` -1 for an OFD
   holder), or `F_UNLCK`. `F_GETLK` with `l_type` `F_UNLCK` is `EINVAL`. A range whose last byte passes `off_t` is
   `EOVERFLOW`, a start before the file `EINVAL`; a record that reaches the last offset is reported with `l_len` 0.
4. **Waiting.** `F_SETLKW` and `F_OFD_SETLKW` sleep a millisecond at a time with the library lock released and ask again;
   a handler that ran ends the wait with `EINTR`; cancellation applies (the sleep is the library's). `SA_RESTART` is ignored: a wait ends with `EINTR` whenever a
   handler ran, where Linux restarts `F_SETLKW`. Between polls the wait checks that its fd still names the same
   description (`EBADF` if another thread closed it); a close and a reopen that give the same connection handle inside
   one poll are not told apart. No deadlock detection: `EDEADLK` is never returned.
5. **Release.** `close`, `dup2` and `dup3` over an fd drop the process's POSIX locks on the file (one `Stat` while the
   process holds any lock); the last fd of a description drops its OFD locks.
6. **Descriptors.** The lock needs a file or directory description: a pipe, a terminal, a socket and `/dev/null` give
   `EINVAL`. `F_RDLCK` needs a readable description and `F_WRLCK` a writable one (`EBADF`).
7. **Bound.** 4096 records; past it `ENOLCK`, for a lock and for an unlock that would split a record.
8. **Tension with ADR-0005.** Item 6 and the table row for record locks say they are absent; this ADR admits
   `fcntl`'s record locks only, in the process. `flock`, `lockf` are unchanged (`lockf` is musl's, over `fcntl`, and
   works now).

### Effect of the vision
For target 1 (a person at the shell) nothing changes. Targets 2 and 3: a program that locks a file (SQLite, a pid
file, a package manager) works as a single process; it does not exclude a second program, and the documentation says
so. Threads: the table is under the library's one lock, so all threads of a process are one owner, as POSIX has it.

## Consequences
- **Positive.** SQLite's unix VFS runs; `threadtest3` can be a test of the threads and not of the locks.
- **Negative.** Two processes on one SQLite file can corrupt it. A program that relies on record locks to exclude
  another process is silently not protected: `F_SETLK` says yes. This is the same unsoundness as the `fcntl` stubs of
  other small libcs, and is named here.
- **Follow-up.** Option B (the file service holds the locks) when a program needs it; `exec` keeping locks and
  `F_GETLK` across processes need it too.

## Invariants
- **INV-0319-1.** A write lock of one owner on a byte range is refused (`EAGAIN`) to another owner of the same file;
  read locks share; one owner never conflicts with itself. *Check:* `cargo test -p zoraya-posix` (`records` and
  `record_locks`).
- **INV-0319-2.** `close` of any fd of a file drops the process's POSIX locks on it, and the last close of a description
  drops its OFD locks; a `fork` child (`after_fork_in_child`) has none. *Check:* `cargo test -p zoraya-posix` (`close_releases`,
  `a_fork_child_starts_with_no_locks`).
- **INV-0319-3.** SQLite's `threadtest3` runs its listed scenarios on SQLite's default unix VFS in Zoraya, with
  integrity checks of its own, under HVF and TCG. Not evidence for the lock logic (one process, one owner): INV-0319-1 and -2 are. *Check:*
  `make test-posix-threadtest3`.

## Open questions
- Whether `exec` should keep the locks (POSIX does; only another process could tell, and only Option B has one).
- Whether a deadlock between two description owners in one process should return `EDEADLK`.
- Whether the one `Stat` per lock call matters for a program that locks a lot; the identity could be kept in the
  description.

## Revisit triggers
- A program of the measured set, or a port, that excludes another process with record locks.
- The file service gaining an owner identity per process (ADR-0103's connections).

## Architect's decision
