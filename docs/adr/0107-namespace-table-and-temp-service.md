# ADR-0107: The namespace table and the temporary directory service

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0005, ADR-0006, ADR-0010, ADR-0013, ADR-0014, ADR-0042, ADR-0050, ADR-0052, ADR-0053, ADR-0055, ADR-0080, ADR-0101, ADR-0103, ADR-0104, ADR-0105
- **Evidence:** CP-01, CP-04, CP-21, CP-22, X-15, X-8, DM-009, DM-041, DM-085, GR-004, GR-006, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, fs/requirements.md#13-open-flags-and-who-uses-them, fs/requirements.md#24-unlink-while-in-use-and-cleanup-on-interrupt, fs/requirements.md#4-metadata-read-and-written, stage-6/sources.md#gnu-make, stage-6/sources.md#not-reachable-from-this-session, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-3-global-paths-path-cwd, task-02/conflict-impact.md#x-1-capability-only-access-cp-01-cp-02-vs-ease-of-posix-porting-cp-27, task-02/method.md#tiers-runs-and-sets, task-02/posix-surface.md#linux-syscall-surface-m2, task-02b/parallel-surface.md#new-syscalls-and-argument-patterns

## Context

ADR-0002 rule 5 makes `/` a per-process table of (prefix → handle) built by the launcher, with cwd as a handle plus a
canonical path, `$PATH` an environment string, `/dev/tty` the terminal, `/dev/null` a null object and `/proc/self/exe`
the process's image handle; its follow-up, a table format in the `abi` crate, moved to this stage (ADR-0053). ADR-0002
rule 1 gives `/tmp`, with FIFO nodes, to a per-session temporary directory service, and INV-0006-9 names the three FIFO
open behaviours GNU make relies on. This ADR decides the table's format, who builds it and how it reaches a process,
how paths resolve, and how the temporary directory service is made. The two are one decision because the table's
`/tmp` entry is the service's root and the FIFO's job is to be opened by name from the table every process of the
session shares: neither works without the other's shape.

**What is measured.**

- Absolute paths opened: the workload's own tree 7,826, `/usr/include` 5,294, `/usr/lib` 559, `/dev/null` 156, `/tmp`
  9, `/etc/passwd` 2, `/etc/group` 1, `/dev/tty` 1; relative opens 19,007 against 13,921 absolute; the literal paths in
  code are `/bin/sh`, `/dev/null`, `/dev/tty`, `/dev/urandom`, `/tmp`, `/etc/passwd`, `/etc/group`, `/etc/profile`,
  `/proc/self/exe` (`task-02/conflict-impact.md#x-1-capability-only-access-cp-01-cp-02-vs-ease-of-posix-porting-cp-27`).
- `$PATH` is walked in user space: 2,922 of 3,002 `execve` calls used an absolute path
  (`task-02/posix-surface.md#linux-syscall-surface-m2`); `chdir` was absolute 32 times and relative once
  (`task-02/conflict-impact.md#x-1-capability-only-access-cp-01-cp-02-vs-ease-of-posix-porting-cp-27`); cwd is part
  of make's and dash's meaning (`task-02/callsite-analysis.md#feature-3-global-paths-path-cwd`).
- Every measured run worked in a namespace whose `/bin` was replaced per set
  (`task-02/method.md#tiers-runs-and-sets`).
- The FIFO jobserver: `mknodat("/tmp/GMfifo<pid>", S_IFIFO|0600)`, the top make opens `O_RDONLY|O_NONBLOCK` then
  `O_WRONLY`, the sub-makes open `O_RDONLY` by the path from `MAKEFLAGS` and set `O_NONBLOCK` with `fcntl`; the top
  make unlinks the FIFO at exit and from its fatal handler while sub-makes hold it open
  (`task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`, `fs/requirements.md#24-unlink-while-in-use-and-cleanup-on-interrupt`).
  In `posixos.c` the FIFO path is built from the temp directory and the pid ([K1], `stage-6/sources.md#gnu-make`).
- Temp files: make's `mkstemp` (`O_RDWR|O_CREAT|O_EXCL`, 2 in T2), toybox `mktemp -d` (`mkdtemp`, T2), dash's
  `memfd_create` for heredocs (10 in W4); `/tmp` prefixed opens 9 (`fs/requirements.md#13-open-flags-and-who-uses-them`,
  `fs/requirements.md#4-metadata-read-and-written`).
- POSIX FIFO opens: a non-blocking read-only open succeeds at once, a non-blocking write-only open fails with `ENXIO`
  without a reader, a blocking open waits for the other end ([P2], [P17], `stage-6/sources.md#posix-semantics`).
- The stage 4 service: `zoraya-fs` over any `BlockDevice`, one service per volume, `posix_mode` stored verbatim
  (ADR-0014), unnamed create and publish (ADR-0010), relative paths of several components, no `..` (ADR-0013), a
  `clone` with narrowed rights (ADR-0055).

**What is assumed.** The RAM volume's size is an assumption. The Fuchsia startup message is cited from memory of
TASK-10's reading, not fetched in this task (`stage-6/sources.md#not-reachable-from-this-session`).

**Forces from TASK-01.** A global mount namespace with root-only mounting (DM-041) and ambient authority by name
(DM-009) are the mistakes; Plan 9's per-process namespaces (GR-004) and the hierarchical namespace itself (GR-006) held
up; environment variables as implicit configuration (DM-085) and `MAKEFLAGS` as an inter-program protocol are X-15;
names resolve relative to a handle (CP-04) and are UTF-8 at the native boundary (CP-22, X-8). Fuchsia hands a new
process its namespace and its fds in a startup message with the handles inside
(`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, the launcher; the message format is
`[outside evidence: fuchsia.dev processargs, not fetched]`).

## Decision drivers

1. **No global root; the launcher decides what a process can name** (ADR-0002 rule 5, CP-01, GR-004).
2. **The table must reach a child without a second channel of ambient data**: not through `environ` (X-15, CP-21),
   and with its handles (ADR-0053's "parent-routed handles").
3. **Few connections per process** (ADR-0103 item 13, ADR-0055's 896): the measured working set is a few prefixes,
   most of them under one volume.
4. **FIFOs are files in a directory, not broker state** (ADR-0002 rule 1), opened by name from any process of the
   session, with INV-0006-9's three behaviours and unlink-while-open (`fs/requirements.md#24-unlink-while-in-use-and-cleanup-on-interrupt`).
5. **No second filesystem implementation**: the stage 4 engine and service exist and are tested (ADR-0010 to
   ADR-0016, ADR-0055).
6. **Per session**: `/tmp` names made of pids (`GMfifo<pid>`) are unique within a session only (ADR-0104's pids).

## Considered options

### Option A: One global namespace in the kernel, `/tmp` on the persistent volume (Linux)

- **Description:** The kernel resolves absolute paths against one tree; `/tmp` is a directory of the root volume;
  FIFOs are inodes the kernel implements.
- **Evidence for:** Nothing to build in the library; every program's expectations met.
- **Evidence against:** Against ADR-0002 rule 5 and CP-01 (DM-009, DM-041); the kernel has no filesystem (ADR-0055);
  `/tmp` on the volume survives sessions, so a crashed session's `GMfifo<pid>` collides with the next session's pids
  (driver 6) and leftovers accumulate.
- **Cost:** Rejected by accepted ADRs.
- **Precedent:** Unix.

### Option B: Prefix table of handles at fixed slots, described by an environment variable

- **Description:** The spawner puts each directory connection at a fixed slot range and sets `ZORAYA_NS="/=512:/tmp=513"`
  in the child's environment; `libposix` parses it at startup and removes it from `environ`.
- **Evidence for:** No new message; `spawn` alone carries everything; easy to read in a trace.
- **Evidence against:** The environment is the program's (X-15, DM-085): a program that clears or rewrites `environ`
  before `exec`ing (`env -i`, which W5 and the measurement harness use) would drop the table for its child; a string
  of slot numbers is a second encoding of the handle list; the fd kinds, `umask`, pids and cwd would need variables
  too.
- **Cost:** *(estimate)* Smallest; a fragile convention.
- **Precedent:** None in the files.

### Option C: A bootstrap message with the table, its handles and the process facts inside (Fuchsia's startup message)

- **Description:** The spawner writes one message into a channel it creates, moves the receiving end to the child at a
  fixed slot, and the child reads it first thing. The message is a flat record (ADR-0042's rules, in `zoraya-abi`):
  pid, parent pid, group pid, session pid, uid, gid, `umask`, the ignored-on-entry signals, the executable's path,
  cwd as a canonical path and an entry index, the fd table's kinds and flags by slot, and the namespace entries
  (prefix, kind, handle index); the handles travel in the message's handle list (ADR-0050, up to 64).
- **Evidence for:** Drivers 1 to 3: one typed record, decoded totally (ADR-0052's discipline), carrying the handles it
  describes, independent of `environ`; the same record serves the broker's launch of the shell and `libposix`'s
  `exec`; a `fork` child needs none (its memory is a copy). It is the shape Fuchsia's launcher uses.
- **Evidence against:** A channel per `spawn`, read once and closed; one more handle in INV-0002-5's list for the
  instant of startup; a record format to version.
- **Cost:** *(estimate)* 150 lines of encoder and total decoder in `zoraya-abi`, shared by the broker and `libposix`.
- **Precedent:** Fuchsia processargs (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option D: The table in a memory object at a fixed slot

- **Description:** As C, but the record sits in a memory object the child maps and the handles sit at the slots the
  record names.
- **Evidence for:** No channel; a page the child can keep mapped as its table.
- **Evidence against:** A memory object cannot carry handles, so the record and the handles are two things to keep
  consistent; a page per spawn committed and copied (ADR-0051) for a few hundred bytes; the child must unmap it to
  avoid a shared page with its parent, which ADR-0102 would otherwise copy.
- **Cost:** *(estimate)* As C plus the slot bookkeeping of B.
- **Precedent:** None in the files.

### For the service: Option E: `/tmp` on the persistent volume with FIFOs as a mode

- **Description:** No second service; `fsd` of the root volume gains FIFO behaviour for files whose `posix_mode` says
  `S_IFIFO`; `/tmp` is a directory of the root.
- **Evidence for:** One service, nothing to start per session; the engine stores the mode already (ADR-0014).
- **Evidence against:** Driver 6 (pids per session, leftovers across boots); temp files cost commits on the real
  volume (a `mkstemp` is a durable create under ADR-0010 rule 3's prefix); the session's `/tmp` is not private to it.
- **Cost:** *(estimate)* The FIFO logic alone (about 200 lines in `zoraya-fs-service`).
- **Precedent:** Unix.

### For the service: Option F: a per-session `fsd` over a RAM block device, with FIFOs as a mode

- **Description:** The broker starts `fsd --memory 16M` in the session job: the stage 4 service with an in-process
  `BlockDevice` over a memory object, a fresh `zoraya-fs` volume made at start, no driver, no durability. `mkfifo` is
  a create with `posix_mode` `S_IFIFO`; `open` of such an object is diverted to a stream pair the service keeps per
  live FIFO (ADR-0101); everything else (`stat`, `readdir`, `unlink`, `rename`) is the engine's.
- **Evidence for:** Drivers 4 to 6 with no new filesystem code: the engine, the loop and the checker are reused;
  `/tmp` dies with the session; FIFOs are entries the engine already names, so unlink-while-open is ADR-0010 rule 7's
  nameless object. The commit path over RAM costs copies, not I/O.
- **Evidence against:** A process and a volume per session; memory for the volume committed up front (ADR-0051); the
  engine's copy-on-write commits are work a RAM filesystem does not need. `memfd_create` as an unnamed file here puts
  a service hop on a heredoc.
- **Cost:** *(estimate)* A `MemoryDevice` of 100 lines, a `--memory` option in `fsd`, the FIFO logic of E.
- **Precedent:** QNX's and Fuchsia's memory filesystems as ordinary services
  (`alternatives-postmortem.md#qnx`; Fuchsia's `memfs` is `[outside evidence]`).

### For the service: Option G: FIFOs in the broker

- **Description:** `mkfifo` registers a name with the broker, which hands out stream ends.
- **Evidence for:** No filesystem involvement.
- **Evidence against:** ADR-0002 rule 1 forbids it; `stat`, `ls` and `unlink` of a FIFO would need a second name space.
- **Cost:** Rejected by an accepted ADR.
- **Precedent:** None.

## Recommendation

**Recommendation (not a decision): Option C for the table and Option F for the service.**

**The bootstrap record** (`zoraya_abi::posix::Bootstrap`, flat, little-endian, magic `ZPB1`, header size for
versioning, decoded totally):

1. Facts: `pid`, `ppid`, `pgid`, `sid`, `uid`, `gid`, `umask`, `ignored` (a 32-bit set of signals ignored on entry,
   ADR-0106 item 1), the executable path as the spawner resolved it (for `/proc/self/exe`).
2. cwd: an entry index and a canonical path string relative to that entry's prefix.
3. fds: for each slot below `OPEN_MAX` that holds a handle or a `Null`: kind (`File`, `Dir`, `StreamRead`,
   `StreamWrite`, `Tty`, `Null`), flags (`O_NONBLOCK`; `O_APPEND` lives in the description, ADR-0103), and for a
   `Tty` the index of its terminal connection among the message's handles (the stream end is at the slot).
4. Namespace entries, longest first: prefix string, kind (`Directory`, `Tmp`, `Null`, `Terminal`, `Image`), handle
   index into the message's handle list or none. A `Directory` or `Tmp` entry carries a directory connection
   (ADR-0055), `Terminal` the process's terminal connection (ADR-0105), `Image` the process's own image handle
   (ADR-0108), `Null` nothing.
5. Fixed slots in `zoraya-abi`: `BOOTSTRAP` (the channel end, read at start-up and closed), `BROKER` (ADR-0104),
   `PROCESS_JOB` (the process's own process job, `CREATE_PROCESS | TRANSFER`, ADR-0104 item 4; a different slot
   from ADR-0043's `JOB` of the first process), `SELF` (a handle to the process itself with `WAIT`, for
   `REQUEST_PENDING`, ADR-0106 item 3), all above `OPEN_MAX` (ADR-0103 item 4); fds at their numbers. The spawner
   cannot list the child's own handle in the child's `spawn`, since it exists only after the call: it writes a second
   message on the bootstrap channel right after `spawn` returns, carrying a `WAIT` duplicate of the process handle,
   and the child's start-up reads both messages before anything else. A `fork` child gets its `SELF` handle from
   `snapshot_process` (ADR-0102 item 3) and reads no bootstrap channel.

**Who builds it.** The broker for the session's shell (ADR-0104 item 12): `/` → a `clone` of the root connection with
every right the launcher chose (all of them for the toolchain: `read`, `write`, `create`, `remove`, `rename`,
`publish`, `clone`, and `sync`, which `O_SYNC`'s `durable` writes need, ADR-0010), `/tmp` → the temporary
service's root, `/dev/null` → `Null`, `/dev/tty` → `Terminal`, `/proc/self/exe` → `Image`; cwd `/`. `libposix` for
every child it spawns (ADR-0103 item 7): it copies its table, `clone`s each directory connection with the same rights,
`clone`s the terminal connection with the child's group (ADR-0105 item 4), and writes the record with the child's pid
facts from `spawn_prepare`. Nothing else is in the message: INV-0002-5's list is the table's handles, the broker
channel, the image, the job and the fds, plus `BOOTSTRAP` for the instant before the child reads it; the test for
INV-0002-5 enumerates the child's table after the read.

**Resolution** (`zoraya-posix`'s resolver, host-tested):

6. An absolute path is normalised lexically (`.` removed, `..` applied to the preceding component and dropped at the
   root, INV-0002-4), then matched against the entries by longest prefix on component boundaries; the remainder is a
   `RelativePath` of several components for the entry's connection (ADR-0013 allows several, no `..`). A relative path
   is resolved against cwd's entry and path the same way. No entry matched is `ENOENT`.
7. `IsLink` from the service (ADR-0013 rule 8): the link's target replaces the components consumed so far, a relative
   target from the link's directory, an absolute one from the table; at most 40 links per resolution, then `ELOOP`
   `[Linux's MAXSYMLINKS]`.
8. cwd: `chdir` resolves, opens a directory connection, and keeps (entry, canonical path); `getcwd` prints the entry's
   prefix joined with the path; `realpath` is resolution plus the canonical string. The `Dir` connection of cwd is one
   of the connections `fork` and `spawn` carry.
9. `$PATH`: `execvp` and the shells walk it against the table, as they walk it on Linux. `/bin/sh` in shebangs and
   make's `_PATH_BSHELL` resolve through `/`.
10. Names: UTF-8 (CP-22); a byte string that is not UTF-8 is `EILSEQ` (ADR-0013 resolves X-8 so); nothing here changes
    it.
11. `uname`: `sysname` "Zoraya", `release` the kernel's version string from the build, `machine` "aarch64",
    `nodename` "zoraya".

**The temporary directory service** (`tmpd` is `fsd` with `--memory SIZE`, `SIZE` 16 MiB `[assumption]`):

12. The broker starts it in a job of its own beside the session job (ADR-0104 item 1, so that a signal to the
    session never reaches it), with a fresh in-memory volume (`zoraya-mkfs`'s logic over a `MemoryDevice`), and
    gives the shell's table its root as `/tmp`. It ends with the session.
13. `mkfifo(path, mode)`: `open_with_mode` (ADR-0103 item 1) with `CREATE | EXCLUSIVE` and
    `posix_mode = S_IFIFO | (mode & ~umask)`, then close;
    `stat` reports `S_IFIFO`; `readdir`, `unlink` and `rename` are the engine's (driver 4). `mknodat` with `S_IFIFO`
    is the same; other node types are `EPERM`.
14. `open` of an `S_IFIFO` object in the service: the first open creates a stream pair (ADR-0101) for that object id
    and records reader and writer counts per connection; `O_RDONLY` gets a duplicate of the read end,
    `O_WRONLY` of the write end, `O_RDWR` is `EINVAL` (Linux allows it and POSIX leaves it undefined, [P2]; a stated
    deviation that no measured program meets). Open semantics (INV-0006-9): a read-side open with
    `O_NONBLOCK` returns at once; a write-side open with `O_NONBLOCK` returns `ENXIO` while no reader connection
    exists; a blocking open of either side is answered when the other side has an open connection (the service holds
    the request, at most one per connection). A FIFO fd in `libposix` is a `Stream` description with a connection
    (ADR-0103 item 4), so the service sees each close as that connection's `PEER_CLOSED`.
15. End of file and `EPIPE` on a FIFO: the service holds its own duplicates of both ends while the object has an open
    connection; it drops its write-end duplicate when the last writer connection closes, so readers see 0, and its
    read-end duplicate when the last reader closes, so writers see `PeerClosed`. A later opener after a drop gets a
    fresh pair for the same object; readers still holding the drained pair do not see the new writer. No measured
    program opens a FIFO after its writers have gone, and the deviation is stated.
16. `unlink` of a FIFO with open connections: the name goes, the object lives until the last connection (ADR-0010
    rule 7), which is make's unlink-while-open.
17. `mkstemp`, `tmpfile`: `O_EXCL` creates (ADR-0010 R-2); `mkdtemp` (T2) is an exclusive `mkdir`. `memfd_create`
    (ADR-0103 item 12) is an unnamed create on this service.

**Tensions with accepted ADRs.**

- *ADR-0002 rule 5*'s list of table entries: `/usr/include`, `/usr/lib`, `/bin`, `/etc/passwd` and `/etc/group` are
  reached through `/` here, not as entries of their own; a launcher that wants to narrow may still list them (item 6
  takes any prefixes).
- *ADR-0002 INV-0002-5*: its list gains `SELF` (the process's own handle), `PROCESS_JOB` is its "job handle", a tty
  fd is two handles (a stream end at the slot and a terminal connection among the message's handles), and the
  transient `BOOTSTRAP` channel exists until the record is read; the enumeration test runs after startup against
  this list. Accepting this ADR replaces the invariant's list by it.
- *ADR-0055*: a FIFO is a file object with a special `open`; no new node kind in the protocol, two new behaviours in
  the service (`ENXIO`, held opens). A held open is a request the loop leaves unanswered while it serves other
  connections, which ADR-0055's "one request at a time per connection" allows (the opener sends nothing else until
  its reply); `PEER_CLOSED` on a connection with a held open drops the request. `mkfifo` is `open_with_mode`
  (ADR-0103 item 1), the one protocol addition it needs.
- *ADR-0014*: `posix_mode`'s type bits now carry meaning for the service (`S_IFIFO`), where the ADR had them "stored
  verbatim"; the persistent volume's service treats them as verbatim still, so a FIFO created on the root volume
  would be a plain file. `mkfifo` outside `/tmp` therefore returns `EPERM` from `libposix` (no measured program does
  it).
- *ADR-0006 rule 1*: FIFO ends are byte streams, as it says.

### Effect of the vision

Target 2 is where this design earns its place: a sandbox's table is whatever its launcher lists, with narrowed
connections, and its `/tmp` is its own volume in its own job, so two sandboxes share no names. The record is data the
sandboxed process cannot forge upward (it only describes what the process already holds). Target 3: several sessions
are several `tmpd`s. Threads: the table is read-only after startup; cwd gains a lock. Networking: a `/dev` entry kind
for sockets would be one more kind.

### Deferred to a later stage

`/dev/urandom` (toybox, `direct`, no T1 applet uses it); `/etc/profile`; a persistent `/tmp`; bind-style overlays of
several entries on one prefix; a batch `clone` for spawn.

*Note, 2026-11-03 (the architect's decision on ADR-0153, recorded by an agent):* `zosh` is a third builder of the table. In its tables cwd is an absolute path and a relative path is resolved from it (item 6's second form, INV-0153-8). Item 5's fixed slots gain `REPORT` (the agent's means of carrying out the architect's refused-path hint), the write end of a channel on which `libposix` reports paths no entry covers, once each (ADR-0153 rule 11, INV-0153-7); the call still fails with `ENOENT`.

## Consequences

- **Positive.** One typed record carries a process's whole POSIX identity and namespace, independent of `environ`.
  The resolver is pure and host-tested. `/tmp` and FIFOs reuse the stage 4 engine and service entire.
- **Negative.** A channel and a message per `spawn`. A RAM volume per session, committed up front. FIFO end-of-file
  has a stated corner deviation.
- **Follow-up.** `zoraya_abi::posix::Bootstrap` with its tests; the resolver; `MemoryDevice` and `fsd --memory`; the
  FIFO logic and INV-0006-9's test; `docs/design.md`'s namespace description.

## Invariants

- **INV-0107-1.** `Bootstrap::decode` is total, and a record that encodes then decodes is equal. *Check:*
  `cargo test -p zoraya-abi` with truncations and random mutations, as `SpawnRequest`'s.
- **INV-0107-2.** Resolution never escapes a table entry: for every absolute path, the entry chosen is the longest
  matching prefix and the remainder contains no `..`; `..` at an entry's root resolves to that root. *Check:* a host
  test of the resolver with adversarial paths (INV-0002-4's test).
- **INV-0107-3.** A process's `environ` after startup is exactly the environment vector its spawner passed. *Check:*
  an in-QEMU `libposix` test spawning with a known environment and reading `environ` back.
- **INV-0107-4.** A FIFO node satisfies GNU make's open sequence: a non-blocking read-side open succeeds with no
  writer, a write-side open with a reader succeeds at once, and a sub-make's read-side open followed by
  `fcntl(F_SETFL, O_NONBLOCK)` reads the same tokens the top make wrote. *Check:* a host test of the service loop
  reproducing GNU make's open sequence; with INV-0107-5 this is INV-0006-9's check (its third behaviour is the
  unlink while open).
- **INV-0107-5.** Unlinking a FIFO or a file while connections are open keeps every open connection working and frees
  the object with the last one. *Check:* host test of the service (INV-0010-6's rule through the loop).

## Open questions

- 16 MiB for `/tmp`: W4's configure writes many small files there; ADR-0110's measurement counts its peak.
- Whether `clone` of several directory connections per `spawn` shows in W4's 3,002 `execve`s: ADR-0110.
- A FIFO reopened after its writers closed (item 15's deviation): only if a program in scope does it.

## Revisit triggers

- A measured need to narrow below `/` for the toolchain (target 2): the table already allows it; the launcher's
  policy would be a new ADR.
- `spawn` cost dominated by connection cloning: a batch `clone` request.
- A program in scope using FIFOs outside `/tmp`.

## Architect's decision

Accepted as recommended, 2026-11-02, with the architect's six changes already in the text of ADR-0100 to ADR-0110: the 10 % threshold in ADR-0102, `KILL` for `job_suspend` and `job_resume`, `make test-posix-long`, `make fetch-ports`, `aarch64-unknown-zoraya` with the T2 cost of `config.sub`, INV-0004-6 until exception channels, and INV-0106-4. Decided by the architect; recorded by an agent at the architect's request.
