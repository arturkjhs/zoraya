# ADR-0103: File descriptors and open file descriptions

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0003, ADR-0010, ADR-0013, ADR-0014, ADR-0040, ADR-0050, ADR-0051, ADR-0052, ADR-0055, ADR-0057, ADR-0080, ADR-0101, ADR-0102
- **Evidence:** CP-02, CP-04, DM-002, DM-023, GR-001, alternatives-postmortem.md#qnx, fs/requirements.md#12-linux-syscalls, fs/requirements.md#13-open-flags-and-who-uses-them, fs/requirements.md#26-appends, fs/requirements.md#4-metadata-read-and-written, stage-4/sources.md#services, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes, task-02/posix-surface.md#linux-syscall-surface-m2, task-02b/parallel-surface.md#new-syscalls-and-argument-patterns

## Context

ADR-0002 rule 4 makes a POSIX fd an index into `libposix`'s table of native handles, and ADR-0057 keeps it so. ADR-0055
built the filesystem protocol with one channel per open object, explicit offsets and no file position, and left one
requirement on itself: not to preclude an open file description with a shared position, the object behind `dup`,
`fork` and fds passed at `spawn` (DM-023). It kept open-file state in a service record separate from the connection
(INV-0055-9) for that purpose, and sketched but did not introduce a handoff object for `fork`. This ADR decides the fd
table, the open file description, how a description is shared across `dup`, `dup2`, `fork`, `posix_spawn` and `exec`,
what the filesystem protocol gains, and how a `fork` child reconnects.

**What is measured.**

- Close-on-exec bookkeeping is the bulk of `fcntl` traffic: `F_SETFD` 5,360 and `F_DUPFD_CLOEXEC` 3,894 calls, against
  `F_GETFL`/`F_SETFL` 39 and 25 (`task-02/posix-surface.md#linux-syscall-surface-m2`).
- Redirections are `pipe` plus `dup2` at every site (`task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes`);
  the parallel jobserver inherits fds 3 and 4 by clearing close-on-exec around recursive spawns only, and gives every
  other recipe `--jobserver-auth=-2,-2`; each concurrent job gets a bad stdin by `dup3(5, 0, 0)`
  (`task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`).
- `O_APPEND` opens are dash's `>>` redirections, 393 in T1 and T2; `fcntl(F_SETFL, O_WRONLY|O_APPEND)` appears 12 times
  per W2 `-j` run; the data does not show two processes appending to one file at once (`fs/requirements.md#26-appends`).
- Opens are overwhelmingly `O_RDONLY`, then `O_WRONLY|O_CREAT|O_TRUNC` and `O_APPEND`; `O_EXCL` 3 times
  (`fs/requirements.md#13-open-flags-and-who-uses-them`). `lseek` 1,214 calls in T1 and 7,381 in T2, on every kind of fd
  (`fs/requirements.md#12-linux-syscalls`).
- tcc opens headers by trying paths and stats nothing; make stats targets and prerequisites for their times
  (`fs/requirements.md#4-metadata-read-and-written`).
- The stage 4 service: a channel per open object, 896 open handles in all, `MAX_INLINE` 60 KiB, a `clone` with equal
  or fewer rights, and a `clone` that today makes a new record (ADR-0055, ADR-0080).
- POSIX: `dup` shares the open file description, so offset and status flags; the close-on-exec flag is per descriptor;
  `dup2` closes its target atomically ([P4], [P16], `stage-6/sources.md#posix-semantics`); a `fork` child's
  descriptors refer to the same descriptions ([P3]); `exec` keeps descriptors without close-on-exec ([P7]).

**What is assumed.** Channel round-trip cost on Zoraya is unmeasured (ADR-0050). The connection count per POSIX process
below is arithmetic.

**Forces from TASK-01.** The fd as a uniform handle is what held up (GR-001); its inheritance by default is the mistake
(DM-002), and confusing the descriptor with the description is DM-023. Capabilities say a passed handle is the
authority (CP-02), and names resolve relative to handles (CP-04).

## Decision drivers

1. **One position per description, shared across processes** (DM-023, ADR-0055's requirement): `dup`, `fork` and
   `spawn` copies must see one offset and one `O_APPEND`.
2. **The data path stays in the library and the service**: no broker on `open`, `read`, `write`, `close`, `lseek`
   (INV-0002-3).
3. **Explicit inheritance**: a child holds what its spawner listed (INV-0003-1), at the slot asked (INV-0003-6), so
   `MAKEFLAGS` fd numbers work.
4. **Channel ends are not duplicable** (ADR-0050): every copy of an fd that is a connection needs a connection of its
   own, made by the service, with equal or fewer rights (INV-0055-3).
5. **The service's table is finite** (896, ADR-0055): the design must say how many connections a POSIX process costs.
6. **No new gated kernel facility beyond `snapshot_process`** (ADR-0002 rule 3(b), INV-0002-2).

## Considered options

### Option A: Kernel open file descriptions and a kernel fd table (Linux)

- **Description:** The kernel keeps `struct file` with position and flags, and a per-process fd array; `dup` and
  `fork` share by reference count; `exec` honours close-on-exec in the kernel.
- **Evidence for:** Exactly POSIX, no library state, no round trips for `lseek` ([P4]).
- **Evidence against:** The kernel would hold a file position for a filesystem it does not implement (ADR-0055: the
  engine is a service), reintroducing a VFS layer ADR-0002 rule 1 keeps out; and a per-process fd table is a second
  handle table (ADR-0040). DM-002's default inheritance is the kernel shape this option brings.
- **Cost:** *(estimate)* A kernel file layer and its locking; permanent.
- **Precedent:** Unix.

### Option B: Position in the library, copied at `dup`, shared by convention

- **Description:** Each fd in `libposix` holds the handle, position and flags; `dup` copies the entry and the two
  advance independently; `fork` copies the table; `spawn` passes handles.
- **Evidence for:** Smallest change: no service work, `lseek` is free, `fuchsia.io` keeps the seek offset per
  connection and its clones do not share it (`stage-4/sources.md#services`), so a shipping system lives with it.
- **Evidence against:** Not POSIX: `cmd >> log` from two children of one shell, or a parent and child writing stdout
  in turn, would overwrite each other's bytes; `dup2(fd, 1)` then `write` through both fds would not advance one
  offset. The measured appends (`>>`) are `O_APPEND`, which the service makes atomic anyway, but a shared position for
  plain `write` on an inherited stdout file is what every shell script redirecting a whole script to a file relies
  on. It is DM-023's confusion built in.
- **Cost:** *(estimate)* Lowest; a known deviation that scripts would hit.
- **Precedent:** Fuchsia's `fuchsia.io` (deliberately not POSIX here).

### Option C: Position in the service's description; connections share a description (QNX resource-manager shape)

- **Description:** The service gains an *open description*: position, append flag, and the open file, held in the
  record INV-0055-9 already separates from the connection. `open` and `clone` make a description; a new request
  `share(rights, server_end)` attaches another connection to the *same* description. `read` and `write` carry an
  explicit offset or `current`; `seek(whence, offset)` returns the new position; append is a description flag. The
  library's fd table maps fds to descriptions it holds a connection to, with per-fd `FD_CLOEXEC`; `dup` is a second
  fd on the same connection; `spawn` and `fork` get a `share`d connection per inherited description. QNX's resource
  managers serve POSIX through messages this way (`alternatives-postmortem.md#qnx`).
- **Evidence for:** Drivers 1 to 5. One position per description, advanced inside one request on one thread, so it is
  atomic across every connection (ADR-0055's loop). The library holds no position to get wrong. The protocol grows by
  new ordinals only (ADR-0052). Within one process, `dup` costs nothing.
- **Evidence against:** `lseek` is a round trip (1,214 in T1, 7,381 in T2 across all fds, part of them on files). A
  `fork` costs one `share` per connection. A fd's connection cannot be duplicated, so `spawn` of an fd costs a `share`
  too. Connections per POSIX process rise (two per shared file during the handover).
- **Cost:** *(estimate)* Service: a description table and three requests (`share`, `seek`, `current` offsets),
  200 lines. Library: the fd table with descriptions and the `share` dance at `spawn` and `fork`. Round trips as above.
- **Precedent:** QNX (production); ADR-0055's own deferred section asked for exactly this.

### Option D: A handoff kernel object for `fork` (the TASK-10 sketch) on top of Option C

- **Description:** As C, but the new connections for a `fork` child travel in a duplicable `Handoff` object that
  `snapshot_process` copies and the child empties with `handoff_take`.
- **Evidence for:** `snapshot_process` stays a plain copy with no parameters; the handoff is reusable for any
  duplicable container need.
- **Evidence against:** A second personality-only kernel facility (ADR-0002 rule 3(b); INV-0002-2's "currently one
  entry"); its `DUPLICATE` right lets a `posix` process hand it to a native child through `spawn` (the review's S2);
  two calls and an object type where a parameter of the one existing call does the same (ADR-0102's move list); the
  review of TASK-10 found seven open problems in it (S2, S5 to S8, S12) and the architect moved it out.
- **Cost:** *(estimate)* Option C plus an object type, two calls, and the gating.
- **Precedent:** None in the files.

## Recommendation

**Recommendation (not a decision): Option C, with ADR-0102's move list for `fork`.**

**Descriptions and connections.**

1. **Service side.** An *open description* is the service record of one `open` or `clone`: the open file, the
   position, the append flag. `share(rights, server_end)` attaches a new connection to the caller's description, with
   rights equal to or narrower than the caller's; it needs the `clone` right (ADR-0055), which every connection a
   POSIX process holds carries. The description ends with its last connection. `clone` keeps making a fresh
   description (ADR-0080), which is `open` of the same object again. `open` gains a form that carries the POSIX
   `mode` for a create (`open_with_mode`, a new ordinal under ADR-0052; the stage 4 `Open` message has `flags`,
   `rights` and `path` only, and ADR-0014 rule 6's "a create request carries the mode" was never built): `libposix`
   uses it for every `O_CREAT` open, so tcc's `a.out` is created `0o777 & ~umask` and executable, and ADR-0107's
   `mkfifo` can set `S_IFIFO`. The alternative, `set_attributes` after each create, would cost a round trip on each
   of the 1,317 `O_CREAT|O_TRUNC` opens.
2. **Offsets.** `read` and `write` take an offset or the marker `current`; with `current` the service reads at the
   position and advances it inside the request, atomically across the description's connections because requests
   run one at a time (INV-0055-1). `seek(whence, offset)` returns the new position; `SEEK_END` is a size read in the
   same request. Append: a write to a description with the flag goes to the end and moves the position there (ADR-0010
   rule 2 keeps it atomic). `set_flags` changes append. `pread`/`pwrite` are explicit offsets and move nothing.
3. **Directories.** A directory description has no position in the service; the `readdir` cookie is held once per
   description entry in the library, so `dup`ed directory fds share one stream, as POSIX asks. ADR-0080 keeps one
   cookie per connection in the service, valid for that connection's last reply, so after `fork` or `spawn` the
   child's connection has no valid cookie and its directory stream starts again from the beginning: a stated
   deviation (POSIX leaves a directory stream's state after `fork` to the implementation
   `[outside evidence: POSIX readdir, not fetched]`; no measured program reads a directory across a fork).

**The library's table.**

4. **Shape.** `fds[0..OPEN_MAX)` of `{description index, FD_CLOEXEC}`, `OPEN_MAX` 512 `[assumption: half the handle
   table, ADR-0040; a POSIX process also needs slots for its port, namespace, broker and internal handles]`.
   A *description entry* holds its kind and handles: `File { connection }`, `Dir { connection, cookie }`,
   `Stream { read end and/or write end, connection }` (pipes have no connection; FIFOs and the terminal have one,
   ADR-0105, ADR-0107), `Null`, plus the per-description flags the service does not hold: `O_NONBLOCK` (ADR-0006), and
   for streams `O_APPEND` is meaningless. `open` of a file is one `open` request; of `/dev/null`, no handle.
   `O_NONBLOCK` is therefore per process after `fork` or `spawn`, where POSIX shares it across every copy of the
   description ([P16]): a stated deviation, harmless for make, which sets the flag once on its own read end.
5. **`dup`, `dup2`, `dup3`, `F_DUPFD*`.** A second fd on the same description entry, in the process; no IPC. `dup2`
   closes its target first and is a no-op when the fds are equal ([P4]). `FD_CLOEXEC` is per fd, off on the copy.
6. **`close`.** The last fd of a description entry closes its handles. For a `File` opened for writing, `close` first
   sends `status` (ADR-0055) and returns `EIO` on `StorageFailed` (ADR-0010's personality mapping); a reply of
   `PEER_CLOSED` is `EIO` too.
7. **`spawn` (posix_spawn, exec).** For each fd without `FD_CLOEXEC`, in fd order: `Stream` ends are listed as
   duplicates at slot = fd; a `File`, `Dir` or FIFO connection is `share`d into a fresh channel whose client end is
   moved at slot = fd; a terminal connection is `clone`d by the terminal protocol with the child's group as
   `spawn_prepare` returned it (ADR-0105 item 4), since the terminal's `NotForeground` compares that tag; `Null` is
   listed as no handle, with its kind in the bootstrap record. A `spawn` carries at most `MAX_SPAWN_HANDLES` (64,
   ADR-0042) handles, fixed slots included: a process with more inherited fds and namespace handles than that gets
   `E2BIG` from `posix_spawn` and `exec`, a limit Linux does not have (tension below). The child's `libposix`
   rebuilds its table from the slots and the bootstrap record (ADR-0107), so `MAKEFLAGS` `3,4` names the child's
   slots 3 and 4 (INV-0003-6), and `-2,-2` needs nothing. Pipe style's close-on-exec toggling is `FD_CLOEXEC` in the
   table and costs no call.
8. **`fork`.** The parent `share`s every `File`, `Dir` and FIFO connection and the namespace table's directory
   connections (ADR-0107), `clone`s the terminal connection with the child's group (item 7), awaits the replies
   together, and lists the new client ends in the move list at the slots the parent's own table uses for those
   descriptions, so the child's copied table is valid as it is (ADR-0102 item 4); stream ends duplicate by
   themselves (ADR-0102). A failed snapshot closes the new
   ends and the service drops those connections; the parent's own were never touched.
9. **`exec`.** `spawn` as item 7 plus exit (ADR-0003 rule 5): the old process's connections close with it.
10. **`stat` family.** `stat`, `fstat`, `lstat`, `fstatat`: the service's attributes (ADR-0014) with `st_ino = id`,
    `st_dev` per volume, `st_nlink` 1, `st_atime = st_mtime`, the session's uid/gid, mode from `posix_mode`;
    `lstat` and `fstatat(AT_SYMLINK_NOFOLLOW)` stop at a link object (ADR-0013). `fstat` of a pipe or terminal
    synthesises `S_IFIFO` or `S_IFCHR` with no IPC. `isatty` reads the kind.
11. **`access`, `faccessat`, `eaccess`.** A `stat` and the owner bits (ADR-0014 rule 6); `X_OK` is the execute bit,
    which the measured `$PATH` probes need.
12. **`mmap`.** `MAP_PRIVATE|MAP_ANONYMOUS` only: a memory object created, mapped and recorded for `munmap`;
    `MAP_SHARED` and file mappings return `ENODEV` (ADR-0005). `memfd_create` is an unnamed file on the temporary
    directory service (ADR-0107): one `File` description, so `write`, `lseek` and `read` work unchanged.
13. **Connections per process.** A shell holds: `/` and `/tmp` (namespace, ADR-0107), cwd, the terminal connection
    (ADR-0105), and one per open file. Three stdio fds on the terminal are one description whose output side is a
    stream end and whose connection is the terminal connection just counted. So a quiet shell holds about 4
    connections; a `make -j4` tree of 7 makes and 4 recipes about 60 `[arithmetic]`, against ADR-0055's `MAX_OPEN` of
    896, which also counts attached buffers (one per connection that transfers more than 60 KiB) and tokens.
14. **What is not exported and what maps elsewhere.** `fsync`, `fdatasync`, `sync` and `syncfs` are not exported
    (ADR-0010's personality mapping, INV-0005-1); `O_SYNC` and `O_DSYNC` are the `durable` flag on every write;
    `O_TMPFILE` is an unnamed create and `linkat(fd, "", dir, name, AT_EMPTY_PATH)` on it is `publish` (ADR-0010,
    ADR-0100 item 6); tcc's unlink-then-create pattern is left as written (ADR-0010's open question 1 stays open).
    `pselect` on a `File` or `Dir` fd reports it ready at once (regular files are `NotWaitable`, ADR-0006 rule 1; a
    file connection's `READABLE` means a reply, not data), as ADR-0106 item 3 says.

**Tensions with accepted ADRs.**

- *ADR-0055*: new requests `share`, `seek`, `set_flags`, `open_with_mode`, the `current` offset and `SEEK_END`; new
  ordinals under ADR-0052, no change to a stage 4 method. *ADR-0014 rule 6 and ADR-0080*: the mode of a create
  reaches the service only now (item 1); the stage 4 protocol never carried it.
- *ADR-0042*'s `MAX_SPAWN_HANDLES` bounds the fds a `spawn` can pass (item 7).
- *ADR-0080*'s one cookie per connection makes a directory stream restart in a `fork` or `spawn` child (item 3).
- *[P16]*'s shared `O_NONBLOCK` is per process here (item 4). `clone`'s record stays fresh, so "attach a second connection to the same
  record" is `share`, as ADR-0055's deferred section foresaw.
- *ADR-0050's* rule that an endpoint with queued handles cannot be moved: the moved ends are fresh (nothing queued),
  so the rule is met; `docs/backlog.md` R-27 asks the rule's wording to say "through a channel", which `spawn` and
  the move list rely on.
- *ADR-0002 INV-0002-5*: the child's fd slots hold shared connections, not the parent's handles; the invariant's list
  is met, with the bootstrap channel of ADR-0107 added for the moment of startup.
- *ADR-0003 rule 3*'s "fd table already copied": the table is copied, and the move list refreshes its connections.
- *ADR-0010 personality mapping*: `close` sends `status` on written files, as it says.

### Effect of the vision

For target 2 nothing changes in shape: a sandboxed process's files are connections with narrowed rights, and a
`share` can only narrow (INV-0055-3). The service keeps the position, so a sandbox cannot lie about it. Threads: the
fd table gains a lock; `current` offsets stay atomic because the service advances them. Networking: a socket fd is
one more description kind over a stream pair or a connection. Changing later to Option A would mean a kernel file
layer; nothing here precludes it, nothing asks for it.

### Deferred to a later stage

Record locks (ADR-0005 excludes them); `fuchsia.io`-style per-connection offsets as an option for programs that want
them; a batch `share` if the `fork` cost shows (ADR-0102).

## Consequences

- **Positive.** POSIX sharing semantics by construction, in one place with one lock. The library keeps no position.
  `dup` is free. Close-on-exec is a table bit.
- **Negative.** `lseek` is a round trip. `spawn` costs a `share` per inherited file fd, `fork` one per connection.
  More connections per process.
- **Follow-up.** The protocol additions in `zoraya-proto-file`; the description table in `zoraya-fs-service` with the
  host test of INV-0055-9 extended; the fd table in `zoraya-posix`; a measurement of `lseek` and `share` round trips.

## Invariants

- **INV-0103-1.** Connections of one description see one position and one append flag; a `read` or `write` with
  `current` is atomic across them; the description ends with its last connection. *Check:* a host test of the service
  loop over two connections with interleaved `current` requests against a model of the position, and closes in each
  order.
- **INV-0103-2.** After `fork`, a `read` through an inherited fd in one process moves the position the other sees,
  and after a failed `fork` the service's connection count is what it was before. *Check:* an in-QEMU `libposix` test
  against the service, with snapshot failure injected by exhausting a limit; the connection count is read through the
  `fs-service` crate's host test interface in a host re-run of the same sequence, since the protocol has no count
  request.
- **INV-0103-3.** `libposix` sends no message to the broker on `open`, `read`, `write`, `close`, `stat`, `lseek`,
  `dup`, `mmap` or `pipe`. *Check:* INV-0002-3's test, a file-only workload under a counting broker.
- **INV-0103-4.** A fd with `FD_CLOEXEC` is absent in a `spawn`ed child; one without it is present at the slot equal
  to its number and shares its description with the parent. *Check:* an in-QEMU test spawning with fds 3 and 4 open,
  3 marked close-on-exec, and reading back in the child.
- **INV-0103-5.** `share` never yields rights its connection lacked. *Check:* INV-0055-3's unit test extended to
  `share`.

## Open questions

- Round-trip cost of `share` and `seek` on Zoraya, and the `fork` cost it implies: ADR-0110 measures.
- Whether `OPEN_MAX` 512 suits make with many open files; the W1 to W3 peak is counted by ADR-0110's call log.
- Whether `lseek(SEEK_CUR, 0)` should be answered from a cached position; POSIX allows no divergence, so only with
  a stated rule.

## Revisit triggers

- `lseek` or `share` traffic measured to matter in W1 to W3: a cached position with invalidation, or a batch form.
- A program needing `MAP_SHARED` or file mappings: ADR-0005's exclusion row, by ADR.
- A measured need for more than 896 connections on one volume (ADR-0055's trigger).

## Architect's decision

