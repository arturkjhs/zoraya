# ADR-0101: Byte streams and pipes

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0002, ADR-0004, ADR-0006, ADR-0040, ADR-0041, ADR-0050, ADR-0051, ADR-0057
- **Evidence:** CP-08, CP-10, CP-14, DM-020, DM-022, DM-023, DM-077, DM-160, GR-002, alternatives-postmortem.md#redox, fs/requirements.md#sources-and-queries, stage-6/sources.md#libc-implementations, stage-6/sources.md#posix-semantics, task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes, task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1, task-02b/parallel-surface.md#new-syscalls-and-argument-patterns

## Context

ADR-0004 rule 7 and INV-0004-7 promise `PeerClosed` on a write to a byte stream whose peer has closed. ADR-0006 rule 1
gives byte streams the signals `READABLE`, `WRITABLE` and `PEER_CLOSED`, and rule 6 names `stream_read` and
`stream_write`. ADR-0050 built channels, deferred the byte stream to this stage, and asked whether it is a separate
object or a mode of the channel. `pipe()`, the make jobserver, the FIFO nodes of ADR-0107 and the terminal of ADR-0105
all need it. ADR-0060's console is not it: a shared ring with no peer.

**What is measured.**

- The shell's pipelines are `pipe` and `dup2` at every site, all `direct`; "the pipe is not the problem. What creates
  the other end is" (`task-02/callsite-analysis.md#feature-1-process-creation-exec-wait-pipes`).
- The jobserver is a pipe (`pipe2` with flags 0, fds 3 and 4) or a FIFO in `/tmp`; tokens are single `+` bytes, 2 to
  38 token reads per build; after a ready `pselect6` the non-blocking `read` lost the race 9 times in two W8 `-j4`
  runs and got `EAGAIN` (`task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`).
- Each job spawned while another runs gets a "bad stdin": a pipe whose write end is closed, `dup3`'d to fd 0
  (`task-02b/parallel-surface.md#new-syscalls-and-argument-patterns`).
- `yes | head -n 3`: `head` exits after three lines and `yes` is killed by `SIGPIPE` on its next write; dash treats
  that as normal (`task-02b/event-timelines.md#w10-background-jobs-wait-concurrent-pipelines-run-w10-parallel-r1`).
- musl's `posix_spawn` reports an `exec` failure through a pipe and relies on a write of `sizeof errno` bytes being
  atomic because it is below `PIPE_BUF` ([M6], `stage-6/sources.md#libc-implementations`).
- Linux: a pipe holds 65,536 bytes by default; `PIPE_BUF` is 4096; writes of at most `PIPE_BUF` bytes are atomic
  (non-blocking: all or `EAGAIN`), larger writes may be partial and interleaved; `read` returns 0 once every write end
  is closed; a write with no reader gives `SIGPIPE` or `EPIPE` ([P1], `stage-6/sources.md#posix-semantics`).
  musl's `limits.h` fixes `PIPE_BUF` at 4096 for programs compiled against it ([M5]).
- `read`, `write`, `readv` and `writev` counts in the data cover every kind of fd and do not separate pipe from file
  traffic (`fs/requirements.md#sources-and-queries`), so pipe volume is not known on its own.

**What is assumed.** Capacity and `PIPE_BUF` are Linux's values, not measured needs. Nothing about stream throughput
is measured on Zoraya.

**Forces from TASK-01.** Pipes are the composition primitive that held up (GR-002), and `SIGPIPE` by default is the
part that did not (DM-077). Untyped byte streams as the only medium is *contested* (DM-020); here they are one object
beside channels, not the only one. Readiness plus non-blocking transfer is the contract ADR-0006 chose, and a
registration must die with its handle (DM-023).

## Decision drivers

1. **POSIX pipe semantics must hold for the measured uses**: atomic token writes and reads, `EAGAIN` on a lost race,
   `EOF` when the writers are gone, `EPIPE`/`SIGPIPE` when the readers are gone (what is measured).
2. **A pipe end must be duplicable** (ADR-0003 rule 3, ADR-0040): `dup2` to fd 0, inheritance by `fork` and `spawn`,
   two makes holding one jobserver end.
3. **One wait** (ADR-0006, CP-08): readiness through the port, with the signals rule 1 already names.
4. **A wait point must be in the library**, so that ADR-0004's handlers run while a `read` blocks: the kernel must not
   block inside a transfer.
5. **Bounded kernel memory per object, committed at creation** (CP-14, ADR-0051's rule for memory objects).
6. **No cycles and no collector** (ADR-0050, DM-022): a stream carries bytes only, never handles.

## Considered options

### Option A: Linux pipes in the kernel, including `SIGPIPE` (baseline)

- **Description:** A kernel pipe object with blocking `read` and `write` system calls, 64 KiB capacity, `PIPE_BUF`
  atomicity, `O_NONBLOCK` as a per-description flag, and the kernel sending `SIGPIPE` to a writer with no readers.
- **Evidence for:** Exactly what the programs expect; `yes | head` ends as measured with no library code (what is
  measured). Fifty years of use (GR-002).
- **Evidence against:** `SIGPIPE` is DM-077 and asynchronous delivery is against CP-10 and ADR-0004. Blocking
  transfers in the kernel put the wait point where `libposix` cannot deliver handlers (driver 4) and would need a
  `Deadline` parameter (INV-0006-2). A per-description flag needs a kernel open file description, which ADR-0103 keeps
  in the library.
- **Cost:** *(estimate)* Smallest library; a second blocking path in the kernel and a signal path ADR-0004 rejected.
- **Precedent:** Unix.

### Option B: A byte stream as a mode of the channel

- **Description:** `channel_create` gains a `STREAM` flag; a stream channel ignores message boundaries on read, allows
  partial writes and refuses handles. One object type for both.
- **Evidence for:** No new object type, one set of signals, one teardown path, one `Status` vocabulary.
- **Evidence against:** ADR-0050 built channels around whole messages (all or nothing, `BufferTooSmall` leaves the
  message in place, 64 messages per direction, no `DUPLICATE`) and partial reads and writes "have nothing in common
  with whole messages" (ADR-0050's deferred section). An endpoint has one holder so that `PEER_CLOSED` is exact; a pipe
  end needs many holders (driver 2), so the flag would change the rights table and the `PEER_CLOSED` rule by mode,
  which is two objects under one name.
- **Cost:** *(estimate)* Less code at first; every channel rule gains a `STREAM` case.
- **Precedent:** Unix-domain sockets carry both stream and datagram semantics under one type (DM-022's object).

### Option C: A separate kernel byte-stream object, non-blocking only (Zircon socket shape)

- **Description:** `stream_create(out)` makes one buffer with a *read end* and a *write end*, two handles of distinct
  types. `stream_write(write_end, bytes)` and `stream_read(read_end, buffer)` never block: they transfer what fits and
  return `WouldBlock` when nothing does. Signals as ADR-0006 rule 1. Both ends are duplicable; `PEER_CLOSED` on an end
  means no handle to the other end exists any more. Zircon's socket is a kernel byte stream with `READABLE`,
  `WRITABLE` and `PEER_CLOSED` `[outside evidence: fuchsia.dev zx_socket, not fetched]`.
- **Evidence for:** Drivers 1 to 6. The library implements blocking as a port wait plus retry, which is where ADR-0004's
  handlers must run anyway. Duplicable ends give `dup`, `fork` and `spawn` for free and make `PEER_CLOSED` a count of
  live handles, which `Arc` already keeps (ADR-0041). Bytes only, so ADR-0050's cycle rule is untouched.
- **Evidence against:** A fourth transfer object beside channels, memory objects and the console. A blocking `read` is
  two system calls (wait, read) instead of one. Capacity committed at creation costs 64 KiB per pipe even for a
  one-byte jobserver token.
- **Cost:** *(estimate)* 300 lines of kernel code and a host-tested ring crate; thin library wrappers.
- **Precedent:** Zircon sockets (shipping); the readiness contract of ADR-0006.

### Option D: Pipes as a user-space service over shared memory (Redox ring, no kernel object)

- **Description:** A `piped` service, or each pair of processes, keeps a ring in a memory object and signals through
  an event object; the kernel carries no bytes.
- **Evidence for:** Nothing new in the kernel; bulk throughput of shared rings (`alternatives-postmortem.md#redox`).
- **Evidence against:** A ring in shared memory is hostile input to both sides (ADR-0051's rule, DM-160) and the
  reader cannot know the writer's liveness without a kernel object anyway; `PEER_CLOSED` becomes a protocol. A fork
  child must re-map every ring (ADR-0102 copies mappings). Every `read` of one byte is a cross-process dance for a
  token.
- **Cost:** *(estimate)* A service, a protocol and a double-fetch rule in every reader.
- **Precedent:** Redox rings (bulk I/O, not pipes).

## Recommendation

**Recommendation (not a decision): Option C**, specified as follows.

- **Object.** `stream_create(out)` writes two handles to an output buffer, as `channel_create` does (ADR-0050): a
  `StreamRead` end and a `StreamWrite` end to one buffer of `STREAM_CAPACITY`, 64 KiB `[assumption, Linux's default]`,
  committed from frames at creation or failing with `NoMemory` (driver 5). The buffer is a ring in a host-tested
  crate, `zoraya-stream`.
- **Calls.** `stream_write(end, bytes)` copies as many bytes as fit and returns the count; with less than
  `STREAM_ATOMIC` free (4096 `[Linux's PIPE_BUF, musl's limits.h]`) a write of at most `STREAM_ATOMIC` bytes copies
  nothing and returns `WouldBlock`, so writes of that size are all or nothing; a larger write with no room is
  `WouldBlock`, with some room a partial count. `stream_read(end, buffer)` returns what is there, at most the buffer,
  `WouldBlock` when empty and the writer end is held, and 0 when empty and no write-end handle exists. Neither call
  blocks, so neither takes a `Deadline`.
- **Peers.** `PEER_CLOSED` on the read end is asserted when the last write-end handle closes (the `Arc` of the
  write-end object drops, ADR-0041); `stream_write` after the last read-end handle closes returns `PeerClosed` and
  changes nothing (INV-0004-7). A handle in flight in a channel message counts as held.
- **Signals.** `READABLE` while the ring holds a byte or `PEER_CLOSED` is set (so that a reader wakes to see end of
  file, as `select` treats end of file as readable, [P11]); `WRITABLE` while at least `STREAM_ATOMIC` bytes are free
  and a read-end handle exists; `PEER_CLOSED` as above. All level-triggered.
- **Rights.** Read end: `READ`, `WAIT`, `DUPLICATE`, `TRANSFER`. Write end: `WRITE`, `WAIT`, `DUPLICATE`, `TRANSFER`.
  Both types are duplicable for `snapshot_process` (ADR-0102) and may be listed in `spawn` and in channel messages
  (so the temporary directory service of ADR-0107 can hand out FIFO ends).
- **Teardown.** Closing the last handle of both ends frees the ring. Nothing is queued inside a stream but bytes, so
  ADR-0050's no-cycle rule needs no extension.
- **Library.** `pipe()` and `pipe2()`: `stream_create`, read end at the lower fd. Blocking `read`: `stream_read`, on
  `WouldBlock` subscribe `READABLE | PEER_CLOSED`, `port_wait` (a wait point, ADR-0106), retry. Blocking `write`:
  `stream_write`, then `WRITABLE | PEER_CLOSED` and retry until all bytes are written, which gives the blocking
  semantics of [P1] for any size. `O_NONBLOCK` (ADR-0103's description flag) returns `EAGAIN` on `WouldBlock`.
  `PeerClosed` → `SIGPIPE` by the rule of ADR-0004 and ADR-0106: default disposition ends the process with
  `exit_with_reason(Terminated, 13)`, otherwise `EPIPE`. Bad stdin: `pipe()`, close the write end, `dup2` to 0, so
  the child's fd 0 is a read end with `PEER_CLOSED` set and `read` returns 0. The jobserver, both styles, is in
  ADR-0103 and ADR-0107 and needs only these calls.
- **`readv`/`writev`.** Library loops over the vector; `writev` of the "Deleting file" message in make's handler is a
  write of a few short pieces, each atomic by size.

**Tensions with accepted ADRs.**

- *ADR-0006 rule 6* says `stream_read`/`stream_write` "are synchronous and take a `nonblocking` flag". Here they are
  always non-blocking and have no flag; the blocking form, which would need a `Deadline` under INV-0006-2 and would
  hide a wait point from `libposix`, is not provided. Accepting this ADR replaces that sentence; rule 6's contract,
  readiness plus non-blocking transfer, is kept exactly.
- *ADR-0006 rule 1* lists terminals among byte streams. ADR-0105 makes the terminal's data path these objects, so the
  row becomes true; ADR-0060's console stays the kernel's own object.
- *ADR-0004 rule 7*: `PeerClosed` is returned by this object, as written.
- *INV-0051-5's allow-list*: `stream_create`'s output and the byte buffers use the `Out`, `Buffer` and `Size` kinds
  that exist; no new parameter kind.

### Effect of the vision

No change for any target. For target 2, a stream's 64 KiB is kernel memory a sandbox can allocate at will until
ADR-0056 charges it; that is ADR-0056's work, as for channels. Threads fit: a transfer is atomic per call. Networking:
a socket is a service reached by channel or a later object using these three signals (ADR-0050's words), and a
`StreamRead`/`StreamWrite` pair is what a user-space network stack would hand a POSIX process for a connected socket.

### Deferred to a later stage

A smaller capacity for pipes that only carry tokens, if 64 KiB per pipe shows in a measurement. Packet mode
(`O_DIRECT`, [P15]) is not in any workload.

## Consequences

- **Positive.** `pipe`, the jobserver, bad stdin and `yes | head` need nothing but this object and the port. One
  object type serves pipes, FIFOs and terminals. No `SIGPIPE` in the kernel.
- **Negative.** A blocking `read` costs two system calls. 64 KiB committed per pipe. A fourth object type.
- **Follow-up.** `zoraya-stream` crate; `abi` signal set for the two end types; the `libposix` wrappers in ADR-0103.

## Invariants

- **INV-0101-1.** A `stream_write` of at most `STREAM_ATOMIC` bytes either writes all of them or returns `WouldBlock`
  and writes none; bytes from two such writes never interleave. *Check:* a `zoraya-stream` host test with interleaved
  writers of 4096-byte records and a reader checking record boundaries, plus an in-QEMU test of the `WouldBlock`
  case on a nearly full ring.
- **INV-0101-2.** `stream_read` returns 0 exactly when the ring is empty and no write-end handle exists, and
  `stream_write` returns `PeerClosed` exactly when no read-end handle exists. *Check:* in-QEMU tests duplicating an
  end, closing the duplicates in each order, and asserting the return values and `PEER_CLOSED` before and after the
  last close; a handle moved through a channel message keeps the peer open.
- **INV-0101-3.** Neither `stream_read` nor `stream_write` blocks: with the ring full or empty each returns within a
  bounded time without any other thread running. *Check:* an in-QEMU test on one CPU with no other runnable thread,
  calling each on a full and on an empty stream and asserting `WouldBlock`.
- **INV-0101-4.** The ring and its metadata are allocated at `stream_create` and never afterwards; a stream's frames
  return when the last handle of both ends closes. *Check:* a counting allocator in the `zoraya-stream` host tests
  for the transfers, and the frame and heap counters of INV-0041-1 around create, transfer and close in QEMU.

## Open questions

- The per-pipe cost in kernel memory against the number of pipes W4 holds at once: count in the W4 measurement
  (ADR-0110).
- Whether the two-system-call blocking read shows in W8 at high `-j`: the wake-up measurement of ADR-0006's open
  question covers it.

## Revisit triggers

- A measured workload holding enough pipes for 64 KiB each to matter: a smaller or lazily committed capacity.
- A blocking transfer measured to dominate a pipeline: a combined wait-and-read on the port (ADR-0006's batching
  trigger), never a blocking kernel transfer.

## Architect's decision

