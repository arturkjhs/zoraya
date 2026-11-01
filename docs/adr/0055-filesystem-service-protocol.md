# ADR-0055: Filesystem service protocol

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0005, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0016, ADR-0050, ADR-0051, ADR-0052, ADR-0054
- **Evidence:** CP-02, DM-160, alternatives-postmortem.md#qnx, alternatives-postmortem.md#redox

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
- **Evidence for:** Fewest copies and crossings.
- **Evidence against:** Against ADR-0012's isolation of crafted images, and ADR-0002 rule 1.
- **Cost:** A permanent kernel parser of hostile data.
- **Precedent:** Linux.

### Option B: One channel per client, with integer file ids inside
- **Description:** A client holds one connection and names open files by numbers the service gives it.
- **Evidence for:** One subscription per client, and cheap opens.
- **Evidence against:** A file cannot be passed as a handle (driver 2), so `spawn` cannot pass an fd; integers name objects, against the spirit of CP-02.
- **Cost:** A second naming layer, as in POSIX over a socket.
- **Precedent:** NFS, 9P.

### Option C: One channel per open object
- **Description:** Each open file or directory is a channel endpoint to the service. Opening creates a new channel; closing it is the close.
- **Evidence for:** Drivers 2 and 3. It is how Fuchsia's file protocol works, with each open object a channel `[outside evidence: fuchsia.io]`. QNX's resource managers likewise serve POSIX through messages (`alternatives-postmortem.md#qnx`).
- **Evidence against:** A subscription per open file on the service's port, and a channel per open in the kernel. ADR-0006 says idle subscriptions cost nothing, and INV-0006-3 checks it. An endpoint with pending handles cannot be moved (ADR-0050), which only matters to a client forwarding replies unread.
- **Cost:** *(estimate)* A channel create per open.
- **Precedent:** Fuchsia.

### Option D: Shared-memory queues per client
- **Description:** A ring of requests in shared memory.
- **Evidence for:** Fewest crossings; Redox's gain (`alternatives-postmortem.md#redox`).
- **Evidence against:** Handles still need a channel, and DM-160 applies to every field. Not needed until measured.
- **Cost:** *(estimate)* A second protocol.
- **Precedent:** Redox rings.

## Recommendation
Recommended: **Option C**, with a shared buffer for large transfers. It is a recommendation, not a decision.

- **Process.** One process per mounted volume, one thread, holding `zoraya_fs::Volume` over a `BlockDevice` that speaks to the driver over a channel. A thin `fs-service` crate owns all IPC; `zoraya-fs` does not change.
- **The loop.** One port; a subscription per client channel for `READABLE | PEER_CLOSED`. Take a request, decode it, run it on the `Volume`, reply. With one thread and synchronous engine calls, requests take effect one at a time, so the loop's order is ADR-0010's completion order (rule 1), and each request is atomic to other clients without further locking. The port's deadline drives `T_COMMIT`.
- **Replies.** A reply goes after the effect. `sync`, and a request with `durable`, get their reply only after the commit that covers them (ADR-0011 rule 3f). A blocking commit stalls the loop; that is acceptable at stage 4 and is measured.
- **Failure.** A request on a `Failed` volume gets `StorageFailed`, sticky, from the `Volume` itself. `take_failure_report` is polled after each request, and its result goes as a message on the supervisor channel of ADR-0053, which settles ADR-0010's assumption and ADR-0016 rule 8. Reads go on as ADR-0010 says. A service that dies gives every client `PEER_CLOSED`, which `libposix` maps as `StorageFailed`.
- **Handles and rights.** `open(directory, RelativePath, flags, rights)` returns a new channel. The service keeps the rights per connection: `read`, `write`, `create`, `remove`, `rename`, `publish`, `sync`, `clone`. A request lacking its right gets `AccessDenied`. `clone(rights)` gives a new channel with equal or fewer. A kernel handle cannot express these, so the connection does. Nothing resolves `..` here (ADR-0013).
- **Closing.** `PEER_CLOSED` on a connection is its close: open-file state goes, and a nameless file with no connection left is freed (ADR-0011 rule 7). A client that dies leaves nothing behind.
- **Data.** Reads and writes up to `MAX_INLINE` 60 KiB `[assumption: ADR-0050's limit less headers]` travel in the message. Above that, the client creates a memory object and sends it once per connection with `attach`; a request then says offset and length. `MAX_ATOMIC_WRITE` stays 1 MiB (ADR-0010 rule 2), reached through the buffer. The service copies out of the buffer before use (ADR-0051's rule), and its decoder takes those bytes as untrusted.
- **Cost of each.** Inline: two copies each way through ADR-0031's byte loop, three system calls per side. Buffer: one copy plus a request message, and setup. Neither is measured on Zoraya.
- **Paging.** `readdir` returns as many entries as fit, with a cookie. The cookie is opaque to the client.
- **`libposix`, for ADR-0005 T1.**

| POSIX | Native |
|---|---|
| `open` | namespace table prefix to a directory channel, then `open` with the rest as a `RelativePath` |
| `read`, `write`, `pread`, `pwrite`, `lseek` | requests with explicit offsets; the file position is a `libposix` value, shared by `dup` copies in the library |
| `close` | `handle_close` |
| `rename` | `rename` on the two directory channels, with the mode of ADR-0010 rule 2 |
| `stat`, `fstat` | `get_attributes` |
| `readdir` | `read_directory`, with its cookie |
| errors | `NotFound`, `Exists`, `NoSpace` and so on map to `errno`; `StorageFailed` and a closed peer to `EIO` |

A position shared across `spawn`, as a parent and child sharing stdout need, is a tension: see below.

**Tension with accepted ADRs.** ADR-0002 rule 4 says fds are handles passed at spawn. POSIX shares the file position between such copies. A client-held position does not cross processes. Stage 4 has no POSIX, so this ADR records it: either the service holds the position per connection, or `libposix` shares it through a memory object. Decide at stage 6.

### Effect of the vision
No change for targets 1 and 3. For target 2, one service per volume and per-connection rights give each sandbox a narrowed directory channel, which is what is wanted, and a hostile client is the assumed case. An agent could flood a service: per-client limits are a later addition at low cost. A multi-threaded service for concurrent reads would need engine changes, and threads are not precluded.

## Consequences
- **Positive.** ADR-0010's linear order holds with no locks. Files pass as handles. A dead client leaves nothing.
- **Negative.** Everything waits on one thread during a commit. A channel per open costs kernel memory. Rights are enforced by the service, so they are only as strong as it is, within the volume.
- **Follow-up.** The protocol crate. Measurements of inline and buffer paths. `readahead` is not specified.

## Invariants
- **INV-0055-1.** The service applies requests one at a time, and the reply follows the effect. *Check:* the crash harness of INV-0010-1 run through the service, with a model comparing completion order to reply order.
- **INV-0055-2.** `sync` replies success only after the commit covering every earlier reply. *Check:* in-QEMU test with a test device counting flushes.
- **INV-0055-3.** No request succeeds with a right its connection lacks, and `clone` never adds one. *Check:* unit test over every request and right.
- **INV-0055-4.** After `Failed`, every mutating request and `sync` gets `StorageFailed`, and the supervisor hears once. *Check:* fault-injecting device, in a host test of the service loop.
- **INV-0055-5.** Buffer bytes are copied before they are decoded or written. *Check:* test with a client changing the buffer during requests, under a checker of the written data's consistency, and review.

## Open questions
- Costs of the inline and buffer paths: measure.
- Where a shared file position lives (the tension above).
- Whether the service's blocking on the driver needs an asynchronous `BlockDevice`.

## Revisit triggers
- A measured commit stall that clients notice.
- `open` traffic that makes a channel per open too costly.

## Architect's decision
