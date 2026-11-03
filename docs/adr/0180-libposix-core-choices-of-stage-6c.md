# ADR-0180: `libposix` core choices the stage 6 ADRs leave open

- **Status:** accepted
- **Date:** 2026-11-03
- **Depends on:** ADR-0010, ADR-0013, ADR-0014, ADR-0052, ADR-0055, ADR-0100, ADR-0101, ADR-0103, ADR-0107, ADR-0109, ADR-0110, ADR-0140, ADR-0141, ADR-0153
- **Evidence:** CP-11, CP-27, DM-023, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

TASK-20 built row 6c of the stage 6 plan: the Rust half of `libposix` for files, descriptors, pipes and memory
(`crates/zoraya-posix`), the open description in the file protocol and service (ADR-0103), the bootstrap record
(ADR-0107), and a native launcher that runs C programs built by `zcc` against `fsd`. Writing the code met points
where the accepted ADRs are silent, or say less than the code must decide, or say something the code had to do
differently. The task's rule: write a proposed ADR, implement the least committing option, never fill a gap
silently. Each item below is implemented as the Recommendation says; the tests that cover it are named under
Invariants.

What is measured: nothing here is chosen on a measurement. The costs are in `docs/design.md`, "`libposix` core (as
built)": a `seek` round trip from a native client about 11 µs under HVF, `share` about 18 µs, an `lseek` from C about
7 µs, the resolver about 0.3 to 0.5 µs a path.

The items:

1. **Requests at the position.** ADR-0103 item 2: "`read` and `write` take an offset or the marker `current`", as
   new ordinals with no change to a stage 4 method. An offset field cannot carry a marker without giving a meaning to
   a value the stage 4 requests take today.
2. **What `share`, `seek`, `set_flags` need.** ADR-0103 item 1 gives `share` the `clone` right; nothing names a right
   for `seek` or the flags, nor how `fcntl(F_GETFL)` learns `O_APPEND`, which lives in the service.
3. **`stat`'s fields.** ADR-0103 item 10 wants `st_ino = id` and `st_dev` per volume; the stage 4 `Attributes` reply
   carries neither, and its times are clamped to unsigned nanoseconds.
4. **The owner bits and `O_TRUNC`.** ADR-0014 rule 6 has `open` fail with `EACCES` on a clear owner bit; the library
   learns the mode only from the service, which truncates inside `open` when asked. An object this open made is its
   creator's whatever its mode, so the library must also know whether the open created it.
5. **The bootstrap record's layout.** ADR-0107 items 1 to 5 list the fields, not their offsets, the slot numbers,
   the second message's form, or how a record says it is one of `zosh`'s absolute tables (ADR-0153 rule 6).
6. **The refused-path report.** ADR-0153 rule 11 gives `{ path, access }`, once per path, at most 16, never waiting,
   "dropped and counted"; not the encoding, nor where the count goes.
7. **Start-up.** ADR-0109 item 4 puts `AT_PHDR` on the startup block's copy of the program headers and has
   `zoraya_posix_start` tail-call musl's `_start_c`. `zcc`'s links have a `PT_PHDR` header, from which musl's
   `__init_tls` computes the program's base as `AT_PHDR - p_vaddr`: with the copy's address that base is wrong for a
   program with a TLS segment. And a reference to `_start_c`, which `crt1.o` defines, from a member of `libc.a` is an
   unresolved name to the export check (ADR-0141 item 7).
8. **The `errno` table's open rows.** ADR-0100 item 9 lists most statuses; `LimitReached` is "`EMFILE` or `EAGAIN` by
   call", and `BufferTooSmall`, `DeadlineExceeded`, `Cancelled`, `BadSyscall` and the file protocol's own statuses
   are not listed.
9. **What the stub's open questions leave.** ADR-0141 asks whether `pthread_setcancelstate` and the `nscd` client's
   socket calls become no-ops or the `passwd` parsers are patched; ADR-0140 whether the rows ADR-0100 item 6 stubs on
   purpose keep a stub kind.
10. **A bad pointer.** The task asks for `EFAULT` where POSIX allows it. The library copies file data between the
    program's memory and messages itself; only stream transfers hand the program's pointer to the kernel.
11. **Directory listings.** The protocol's listing gives no object ids, nor `.` and `..`.
12. **Clocks.** `fsd` stamps files with the monotonic clock (no wall clock exists, ADR-0014's open question); `make`
    compares file times with `CLOCK_REALTIME`.
13. **Observation before 6d.** ADR-0110 item 7's statistics travel to the broker, which is 6d's; the task's
    measurements need the resolver's cost per `open` now.
14. **Smaller points the code had to settle.** `fchmod` on an fd opened only to read: ADR-0014 rule 6 lets the owner
    change the mode, but the fd's connection has no `WRITE`. `st_blocks`: ADR-0014 rule 9 says "allocated blocks",
    which the service does not report. `writev`: one request per piece would make a line of musl's stdio (two pieces)
    two appends. Times before 1970: `set_attributes` takes unsigned seconds. Start-up: ADR-0109 item 4 says the vector
    goes on the stack and `AT_RANDOM` comes from the clock and handle generations.
15. **A launcher before the broker.** ADR-0107 has the broker and `libposix` build tables; neither exists for a native
    program to start a C program in 6c.

## Decision drivers

1. **Least committing** (TASK-20's rule): a choice 6d and 6e can change without breaking a program.
2. **Refuse what is not understood** (CP-11): unknown flags and values fail, a request that cannot be answered as
   POSIX asks fails rather than half-succeeds.
3. **The measured surface only** (CP-27): no export beyond the tier list and the ADRs that add names.
4. **One description, one position** (DM-023, ADR-0103): whatever the library does, the service holds the position.

## Considered options

### Option A: Do what Linux does (baseline)

- **Description:** `read` and `write` at a position the kernel holds in `struct file`; `stat` from the inode; the
  auxiliary vector from the loader, which maps the program headers; `EFAULT` from the kernel's copy of every buffer;
  `readdir` with `.`, `..` and inode numbers from `getdents64`.
- **Evidence for:** What every ported program expects; no deviation to state.
- **Evidence against:** ADR-0103 Option A was rejected for the kernel file layer it needs; the position here is the
  service's, and the library sees files only through messages. None of the points is a choice Linux makes for this
  design.
- **Cost:** Not applicable to this design.
- **Precedent:** Linux.

### Option B: The least committing choice at each point (this implementation)

- **Description:** As the Recommendation lists.
- **Evidence for:** Drivers 1 to 4; every point has one stated behaviour and a test.
- **Evidence against:** Several are stated deviations (a fault instead of `EFAULT` on a file read into bad memory,
  no `.` and `..`, `O_TRUNC` as a second request) that a later program might meet.
- **Cost:** As built.
- **Precedent:** ADR-0130 did the same for stage 6a's kernel objects.

### Option C: Fuchsia's shapes (non-Unix precedent)

- **Description:** `fdio`'s per-connection offsets, with a `seek` that returns the position and `read`/`write` that
  advance it, and attributes queried by a separate call (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`;
  the method names are `[outside evidence: fuchsia.io, not fetched]`).
- **Evidence for:** No marker values; a seek is its own request, as here.
- **Evidence against:** Per-connection offsets are ADR-0103's rejected Option B: `dup` across processes would not
  share a position. A separate attribute call after every `open` would double `stat`'s and `open`'s round trips.
- **Cost:** *(estimate)* As B, with one more round trip per `open` for the mode.
- **Precedent:** Fuchsia.

## Recommendation

**Recommendation (not a decision): Option B**, as follows.

1. **Nine new requests at ordinals 23 to 31, `stat` at 32 and `read_directory_ids` at 33** (ADR-0052): `share`, `seek`, `set_flags`,
   `get_flags`, `open_with_mode`, and `read_current`, `write_current`, `read_buffer_current`,
   `write_buffer_current`, which read and write at the description's position and move it inside the request. An
   explicit offset moves nothing, appending or not. No stage 4 request changes. A position past `i64::MAX` is
   refused, as is a seek before 0.
2. **`seek`, `set_flags` and `get_flags` need no right**: they change and read the description, not the object, and
   every connection of it shares them. `share` has `clone`'s rules: the `clone` right, rights only narrow, a place
   in `MAX_OPEN`. `fcntl(F_GETFL)` asks `get_flags` (one round trip); `F_SETFL` sends `set_flags`.
3. **`stat`** gives the volume's id, the object's id and the times exactly (seconds signed). `open_with_mode`'s reply
   gives the same, with `OpenReply`'s fields and whether this request created the object, so a POSIX `open`, `stat`
   or `access` is one round trip.
4. **Created** means the object's id is at or past the volume's next id before the request: ids come in order and are
   never reused (ADR-0014 rule 1). **`O_TRUNC`** is sent as a second request (`set_size` 0) after the owner check, and
   only for an existing non-empty file, so a protected file is never cut and a new or empty one costs nothing more;
   the open and the truncation are then two steps of the completion order, not one. If the truncation fails, the
   open is undone: the description is closed, no fd is made, and the truncation's `errno` is returned.
5. **The record** is `zoraya_abi::posix`'s `ZPB1` (header 64 bytes, then the executable's path, cwd, 8-byte fd
   entries and the entries with their prefixes); the fixed slots are 1000 to 1004 (`BOOTSTRAP`, `BROKER`,
   `PROCESS_JOB`, `SELF`, `REPORT`), of which a spawned child finds `SELF` not at its slot but in the second message,
   the 4 bytes `ZPBS` with one handle (a spawner cannot list the child's own handle), and keeps it for 6d; `SELF`'s
   slot is a `fork` child's (`self_slot`, ADR-0102); flag `ABSOLUTE` marks a
   table whose relative paths join cwd's absolute path (ADR-0153 rule 6), and its absence ADR-0107 item 6's form.
   Unknown flags, reserved bytes that are not zero, a handle index used twice, prefixes out of order or not normal,
   and a cwd entry that is not a directory are refused.
6. **A report** is `ZPR1`, the access, the count of reports dropped before it, and the path. A process sends at most
   16; one that does not fit the channel is dropped and counted; a path is never reported twice, sent or dropped.
7. **Start-up.** `AT_PHDR` is `PT_PHDR`'s `p_vaddr` when the table has one (the loaded copy, so musl's base is 0, as
   for every static executable), else the startup block's copy. `crt1.S` passes `_start_c`'s address to
   `zoraya_posix_start`, which calls it, so `libc.a` refers to nothing outside itself.
8. **`errno`:** `LimitReached` is `EMFILE` for a call that makes a descriptor, `ENOMEM` for one that maps memory,
   `EAGAIN` otherwise; `BadSyscall` `ENOSYS`, `DeadlineExceeded` `ETIMEDOUT`, `Cancelled` `ECANCELED`,
   `BufferTooSmall` `EMSGSIZE`. File statuses: `INVALID_NAME` `EINVAL`, a link left over `ELOOP`, `LIMIT_REACHED`
   (the service's buffers) `ENOMEM`, `UNSUPPORTED` `ENOSYS`, a connection gone `EIO` (ADR-0055: `StorageFailed`).
9. **The stub's questions.** `pthread_setcancelstate` succeeds and changes nothing (one thread, no cancellation);
   the `nscd` client's `socket`, `connect`, `send` and `sendmsg` fail with `ENOSYS` (musl's `getpw*` read the files
   first, so nothing changes for them). `tee`, `syscall` and `mprotect` fail with `ENOSYS` as ADR-0100 item 6 says;
   no stub kind is kept, the functions are written out. The process functions of 6d (`fork`, `exec*`,
   `posix_spawn*`, `wait*`, `kill`, signals, `pselect`) fail with `ENOSYS` until 6d; a FIFO from `mkfifo` or
   `mknodat` is `ENOSYS` until the temporary directory service exists, any other node `EPERM`.
10. **Bad pointers. The rule: a copy `libposix` makes in the process faults on a bad pointer; a copy the kernel makes
    returns `EFAULT`.** So a file read or write through a bad pointer, whose data the library copies between the
    program and a message or its buffer, ends the process with a fault (POSIX leaves it undefined), and the service
    and the kernel go on; a stream transfer hands the program's pointer to the kernel (`writev` to a stream piece by
    piece), so a bad one is `EFAULT`. `NULL` where a buffer, path or result is needed is `EFAULT`, checked by the
    library before any copy. A count past `SSIZE_MAX` is `EINVAL`.
11. **Listings** give each entry's object id as `d_ino`, the id `stat` gives as `st_ino` (ADR-0014 rule 7; the
    architect's decision), through `read_directory_ids` (ordinal 33), which carries it; `d_type` from the kind; `d_off` 0
    (no `telldir` or `seekdir` is in the tier list); no `.` or `..`.
12. **`CLOCK_REALTIME` is the monotonic clock** while `fsd` stamps files with it, so `make`'s comparisons agree;
    `UTIME_NOW` uses it too. Every clock id 0 to 7 is that clock.
13. **`ZORAYA_POSIX_STATS`** in the environment makes the library write one `POSIX-STATS` line of its counters to fd 2
    at exit (resolutions and their mean time, requests, seeks, shares, reports), until ADR-0110 item 7's statistics.
14. **The smaller points.** `fchmod` through a connection without `WRITE` reopens the fd's place with `WRITE` and
    changes the mode only if that place still holds the fd's object (same volume and id), else `EACCES`; it widens
    nothing a narrowed table did not already give, since the reopening goes through the table's own entry.
    `st_blocks` is the size rounded up to 4 KiB, in 512-byte units (sparse files overstate). `writev` to a file
    gathers pieces of at most 60 KiB in all into one write, so a stdio line is one append; to a stream it writes
    piece by piece, so the kernel still checks each pointer. At most 256 refused paths are remembered,
    whatever became of their reports. `utimensat` with negative seconds is
    `EINVAL`. Start-up builds the vector in memory of the library's heap, not on the stack, and seeds `AT_RANDOM`
    from the clock and an address (weak, as ADR-0109 item 4 labels it).
15. **`posixrun`**, a native program in the ramdisk, is 6c's launcher: a table from its options (`/` by default),
    the program from the volume through `image_create`, fds 0 to 2 (`/dev/null` and a stream it copies to the
    console), a `BROKER` channel nobody serves and whose messages it counts, the `REPORT` channel, and a `Tmp` entry
    that is a volume directory standing in for 6d's service. It is not a broker: no pid, no `posix_spawn`.

## Consequences

- **Positive.** Every point has one stated behaviour and a test. `open`, `stat` and `access` cost one round trip.
  The library never holds a position.
- **Negative.** `O_TRUNC` on an existing non-empty file is two requests. A bad pointer to a file read faults instead
  of `EFAULT`. Listings have no `.` or `..`. `posixrun` is a second table builder until the broker.
- **Follow-up.** 6d replaces `posixrun`'s role with the broker and `libposix`'s `spawn`; `share` is then used at every
  `spawn` and `fork`; a wall clock would change item 12 for both `fsd` and the library together.

## Invariants

- **INV-0180-1.** A request at the position moves it by exactly what it read or wrote, for every connection of the
  description, and an explicit offset moves it not at all. *Check:* `zoraya-fs-service`'s host tests
  (`interleaved_requests_at_the_position_lose_no_update`, `shared_connections_see_one_position`,
  `buffer_requests_at_the_position_move_it`).
- **INV-0180-2.** `open` never truncates a file whose owner write bit is clear, and never refuses a file it created
  for its mode. *Check:* `zoraya-posix`'s host test `owner_bits_are_applied_as_posix_would`, against the real
  service.
- **INV-0180-3.** `libc.a` contains no reference to a name it does not define, and `svc` only in `zoraya-rt`'s member.
  *Check:* `posix-surface-check check ... --allow-svc zoraya_rt-` in `make check-ports`, and the five broken
  archives.

## Open questions

- Whether a program in scope reads `.` or `..` from a listing (W1 to W3 will show).
- Whether an `EFAULT` for file data is worth a copy through the kernel on every read and write.

## Revisit triggers

- A wall clock in the kernel: item 12.
- A measured `O_TRUNC` workload where the second request shows, or a reader that sees the moment between: an
  owner-bit-aware truncation in the service.
- 6d's broker: item 15.
- A program that sets times before 1970 (`tar`, `cpio` of old archives): a signed `set_attributes`.

## Architect's decision

Accepted 2026-11-03. Decided by the architect; recorded by an agent at the architect's request.
