# ADR-0055: Filesystem service protocol

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0005, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0016, ADR-0050, ADR-0051, ADR-0052, ADR-0054
- **Evidence:** CP-02, DM-160, alternatives-postmortem.md#qnx, alternatives-postmortem.md#redox, fs/requirements.md#26-appends, stage-4/sources.md#services

## Context
`zoraya-fs` is a synchronous engine that knows nothing about IPC. ADR-0012 asks for one service per volume. ADR-0010 promises a single completion order, atomic requests, durability on `sync`, and sticky failure, and left two points open: `MAX_ATOMIC_WRITE`, "to be fixed with the IPC message size", and how a supervisor hears of failure. ADR-0002 rule 4 has POSIX files be native handles.

## Decision drivers
1. ADR-0010's rules must be true by construction of the request loop.
2. A file handle is a kernel handle: closing it, passing it and losing it on exit work without the service trusting a client's numbers (CP-02).
3. Rights only decrease, for files too.
4. Small reads and writes dominate the path: `libposix` must stay thin (ADR-0002 rule 1, `fs/requirements.md`).
5. Large writes stay atomic up to ADR-0010's limit.

## Considered options

### Option A: A kernel VFS with file descriptors (Linux baseline)
- **Description:** The filesystem runs in the kernel, and `read` is a system call.
- **Evidence for:** Fewest copies and crossings., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Evidence against:** Against ADR-0012's isolation of crafted images, and ADR-0002 rule 1., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Cost:** A permanent kernel parser of hostile data.
- **Precedent:** Linux.

### Option B: One channel per client, with integer file ids inside
- **Description:** A client holds one connection and names open files by numbers the service gives it.
- **Evidence for:** One subscription per client, and cheap opens., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Evidence against:** The integer alone cannot be passed. A connection plus a file id can, as 9P's clone does `[outside evidence]`, at the cost of a second naming layer beside handles (CP-02)., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Cost:** A second naming layer, as in POSIX over a socket.
- **Precedent:** NFS, 9P.

### Option C: One channel per open object
- **Description:** Each open file or directory is a channel endpoint to the service. Opening creates a new channel; closing it is the close.
- **Evidence for:** Drivers 2 and 3. It is how Fuchsia's file protocol works, with each open object a channel (`stage-4/sources.md#services`). QNX's resource managers likewise serve POSIX through messages (`alternatives-postmortem.md#qnx`)., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Evidence against:** A subscription per open file on the service's port, and a channel per open in the kernel. Idle subscriptions cost no wait time (INV-0006-3), but they cost memory. An endpoint with pending handles cannot be moved (ADR-0050), which only matters to a client forwarding replies unread., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Cost:** *(estimate)* A channel create per open.
- **Precedent:** Fuchsia.

### Option D: Shared-memory queues per client
- **Description:** A ring of requests in shared memory.
- **Evidence for:** Fewest crossings; Redox's gain (`alternatives-postmortem.md#redox`)., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Evidence against:** Handles still need a channel, and DM-160 applies to every field. Not needed until measured., fs/requirements.md#26-appends, stage-4/sources.md#services
- **Cost:** *(estimate)* A second protocol.
- **Precedent:** Redox rings.

## Recommendation
Recommended: **Option C**, with a shared buffer for large transfers. It is a recommendation, not a decision.

- **Process.** One process per mounted volume, one thread, holding `zoraya_fs::Volume` over a `BlockDevice` that speaks to the driver over a channel. A thin `fs-service` crate owns all IPC, behind a transport trait so that its loop runs on the host and in the crash harness. `zoraya-fs` gains one method, `Volume::device_lost` (ADR-0054).
- **The loop.** One port; a subscription per client channel for `READABLE | PEER_CLOSED`, and `PEER_CLOSED` on the driver channel. Take a request, decode it, run it on the `Volume`, reply. With one thread and synchronous engine calls, requests take effect one at a time, so the loop's order is ADR-0010's completion order (rule 1), and each request is atomic to other clients without further locking. The port's deadline drives `T_COMMIT`.
- **Replies.** One request at a time per connection: the service does not read a connection whose reply is waiting. If `channel_write` of a reply gives `WouldBlock`, it holds that one reply, subscribes `WRITABLE`, and goes on with other clients. So a client that never reads costs one reply's memory and stalls only itself. A reply goes after the effect. `sync`, and a request with `durable`, get their reply only after the commit that covers them (ADR-0011 rule 3f). A blocking commit stalls the loop; that is acceptable at stage 4 and is measured.
- **Failure.** A request on a `Failed` volume gets `StorageFailed`, sticky, from the `Volume` itself. `take_failure_report` is polled after every engine call, including the timed commit, and its result goes as a message on the supervisor channel of ADR-0053, which settles ADR-0010's assumption and ADR-0016 rule 8. Reads go on as ADR-0010 says, limited by ADR-0054 after device loss. The count of corrupt reads (ADR-0012 rule 3) is a request. A service that dies gives every client `PEER_CLOSED`, which `libposix` maps as `StorageFailed`, and restarts as ADR-0054 orders.
- **Handles and rights.** `open(directory, RelativePath, flags, rights)` returns a new channel. The service keeps the rights per connection: `read`, `write`, `create`, `remove`, `rename`, `publish`, `sync`, `clone`. A request lacking its right gets `AccessDenied`. `clone(rights)` gives a new channel with equal or fewer. A kernel handle cannot express these, so the connection does. Nothing resolves `..` here (ADR-0013).
- **Two objects in one request.** `rename` across directories and `publish` name a second connection. An endpoint cannot be passed for that: the service would receive the client's end and could not match it. As in `fuchsia.io` (`stage-4/sources.md#services`), `get_token` returns a token, a handle to an event object the service keeps a reference to for that connection; the request carries the token, and the service compares it with a new call `object_same(a, b)`. It needs ADR-0006's event object, which stage 4 then builds. The token's connection must hold the right.
- **Closing.** `PEER_CLOSED` on a connection is its close: open-file state goes, and a nameless file with no connection left is freed (ADR-0011 rule 7). A client that dies leaves nothing behind.
- **Data.** Reads and writes up to `MAX_INLINE` 60 KiB `[assumption: ADR-0050's limit less headers]` travel in the message. Above that, the client creates a memory object and sends it once per connection with `attach`; a request then says offset and length. `MAX_ATOMIC_WRITE` stays 1 MiB (ADR-0010 rule 2), reached through the buffer. One buffer per connection counts against ADR-0051's 256 mappings, so a client has at most 4 buffers, and a service unmaps idle ones; `LimitReached` is that client's error. The service copies out of the buffer before use (ADR-0051's rule), and its decoder takes those bytes as untrusted.
- **Cost of each.** Inline: two copies each way through ADR-0031's byte loop, three system calls per side. Buffer: one copy plus a request message, and setup. Neither is measured on Zoraya.
- **Paging.** `readdir` returns as many entries as fit, with a cookie. The cookie is opaque to the client.
- **`libposix`, for ADR-0005 T1.**

| POSIX | Native |
|---|---|
| `open` | namespace table prefix to a directory channel, then `open` with the rest as a `RelativePath` |
| `read`, `write`, `pread`, `pwrite`, `lseek` | requests with explicit offsets; the file position is a `libposix` value, shared by `dup` copies in the library |
| `close` | a `status` request on a written fd, for ADR-0010's `EIO`, one round trip, then `handle_close` |
| `rename` | `rename` on one directory channel, with a token for the other and the mode of ADR-0010 rule 2 |
| `O_APPEND` writes | `write` with an append flag, which is atomic (ADR-0010 rule 2); dash opens 393 such files (`fs/requirements.md#26-appends`) |
| `unlink`, `mkdir` | `remove`, `make_directory` on the directory channel |
| `stat`, `fstat` | `get_attributes` |
| `readdir` | `read_directory`, with its cookie |
| errors | `NotFound`, `Exists`, `NoSpace` and so on map to `errno`; `StorageFailed` and a closed peer to `EIO` |

A position shared across `spawn`, as a parent and child sharing stdout need, is a tension: see below.

**Tensions with accepted ADRs.**

- *ADR-0002 rule 4.* Fds are handles passed at spawn, and POSIX shares the file position between such copies. A client-held position does not cross processes. In `fuchsia.io` the service holds the position per connection (`stage-4/sources.md#services`), but clones there do not share it. Stage 4 has no POSIX: either the service holds it per connection, or `libposix` shares it through a memory object. Decide at stage 6.
- *ADR-0003 rule 3.* See ADR-0050: a `fork` child has no files.
- *ADR-0006 rule 1.* Regular files are `NotWaitable`, but a file handle here is a channel whose `READABLE` means a reply, not data. `libposix`'s `pselect` must treat it as always ready.
- *ADR-0010 personality mapping.* `close` costs a round trip on a written fd, above.
- *ADR-0012 rule 4.* "Holding only" the block handle and client channels: the service also holds a supervisor channel and client memory objects. The sentence needs widening.
- *ADR-0016 rule 8.* "After each request" becomes every engine call.

### Effect of the vision
No change for target 1. For target 3, every client waits on one thread during any commit, which a measured desktop may not bear. For target 2, per-connection rights give each sandbox a narrowed directory channel, but one sandbox calling `sync` in a loop stalls all others on the volume, and limits do not fix that, because fairness needs concurrency and the engine's `&mut self` forbids it. The cheap answer is a volume per sandbox. Making the engine concurrent is a rewrite of its request path `[estimate: thousands of lines]`. Threads are not precluded.

## Consequences
- **Positive.** ADR-0010's linear order holds with no locks. Files pass as handles. A dead client leaves nothing.
- **Negative.** Everything waits on one thread during a commit. A channel per open costs kernel memory. Rights are enforced by the service, so they are only as strong as it is, within the volume.
- **Follow-up.** The protocol crate. Measurements of inline and buffer paths. `readahead` is not specified.

## Invariants
- **INV-0055-1.** The service applies requests one at a time, and the reply follows the effect. *Check:* the crash harness of INV-0010-1 run through the service, with a model comparing completion order to reply order.
- **INV-0055-2.** `sync` replies success only after the commit covering every earlier reply. *Check:* host test of the loop with a fake block peer counting flushes.
- **INV-0055-3.** No request succeeds with a right its connection lacks, and `clone` never adds one. *Check:* unit test over every request and right.
- **INV-0055-4.** After `Failed`, every mutating request and `sync` gets `StorageFailed`, and the supervisor hears once, on an idle volume too. *Check:* fault-injecting device, in a host test of the service loop, with a failing timed commit.
- **INV-0055-6.** A client that never reads its replies stalls only itself. *Check:* host test of the loop with such a client.
- **INV-0055-5.** Buffer bytes are copied before they are decoded or written. *Check:* test with a client changing the buffer during requests, under a checker of the written data's consistency, and review.

## Open questions
- Costs of the inline and buffer paths: measure.
- Where a shared file position lives (the tension above).
- Whether the service's blocking on the driver needs an asynchronous `BlockDevice`.

## Revisit triggers
- A measured commit stall that clients notice.
- `open` traffic that makes a channel per open too costly.

## Architect's decision
