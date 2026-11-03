# ADR-0055: Filesystem service protocol

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0005, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0016, ADR-0040, ADR-0050, ADR-0051, ADR-0052, ADR-0054, ADR-0057
- **Evidence:** CP-02, DM-023, DM-160, alternatives-postmortem.md#qnx, alternatives-postmortem.md#redox, stage-4/sources.md#services

## Context
`zoraya-fs` is a synchronous engine that knows nothing about IPC. ADR-0012 asks for one service per volume. ADR-0010 promises a single completion order, atomic requests, durability on `sync`, and sticky failure, and left two points open: `MAX_ATOMIC_WRITE`, "to be fixed with the IPC message size", and how a supervisor hears of failure. Native clients are the only clients at stage 4.

## Decision drivers
1. ADR-0010's rules must be true by construction of the request loop.
2. A file handle is a kernel handle: closing it, passing it and losing it on exit work without the service trusting a client's numbers (CP-02).
3. Rights only decrease, for files too.
4. Small reads and writes dominate the path (`fs/requirements.md`).
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
- **Evidence against:** The integer alone cannot be passed. A connection plus a file id can, as 9P's clone does `[outside evidence]`, at the cost of a second naming layer beside handles (CP-02).
- **Cost:** A second naming layer, as in POSIX over a socket.
- **Precedent:** NFS, 9P.

### Option C: One channel per open object
- **Description:** Each open file or directory is a channel endpoint to the service. Opening creates a new channel; closing it is the close.
- **Evidence for:** Drivers 2 and 3. It is how Fuchsia's file protocol works, with each open object a channel (`stage-4/sources.md#services`). QNX's resource managers likewise serve POSIX through messages (`alternatives-postmortem.md#qnx`).
- **Evidence against:** A subscription per open file on the service's port, and a channel per open in the kernel. Idle subscriptions cost no wait time (INV-0006-3), but they cost memory. An endpoint with pending handles cannot be moved (ADR-0050), which only matters to a client forwarding replies unread.
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

- **Process.** One process per mounted volume, one thread, holding `zoraya_fs::Volume` over a `BlockDevice` that speaks to the driver over a channel. A thin `fs-service` crate owns all IPC, behind a transport trait so that its loop runs on the host and in the crash harness. `zoraya-fs` gains one method, `Volume::device_lost` (ADR-0054). Open-file state lives in a service record that a connection refers to, not in the connection itself.
- **The loop.** One port; a subscription per client channel for `READABLE | PEER_CLOSED`, and `PEER_CLOSED` on the driver channel. Take a request, decode it, run it on the `Volume`, reply. With one thread and synchronous engine calls, requests take effect one at a time, so the loop's order is ADR-0010's completion order (rule 1), and each request is atomic to other clients without further locking. The port's deadline drives `T_COMMIT`.
- **Replies.** One request at a time per connection: the service does not read a connection whose reply is waiting. If `channel_write` of a reply gives `WouldBlock`, the service holds that one reply and changes the connection's subscription to `WRITABLE | PEER_CLOSED`, dropping `READABLE`, so a queued request cannot make a `Level` subscription spin. It then goes on with other clients. If it gives `NoMemory`, `WRITABLE` says nothing about it, so the service holds the reply, subscribes the connection to `PEER_CLOSED` alone, and sets a port deadline to try again. If the service cannot hold a reply, it closes that connection, which says nothing about whose fault it was. So a client that never reads stalls only itself while the service has memory. At stage 4 every client is trusted; bounds on what one client may cost are in ADR-0056. A reply goes after the effect. `sync`, and a request with `durable`, get their reply only after the commit that covers them (ADR-0011 rule 3f). A blocking commit stalls the loop; that is acceptable at stage 4 and is measured.
- **Failure.** A request on a `Failed` volume gets `StorageFailed`, sticky, from the `Volume` itself. `take_failure_report` is polled after every engine call, including the timed commit, and its result goes as a message on the supervisor channel of ADR-0053, which settles ADR-0010's assumption and ADR-0016 rule 8. Reads go on as ADR-0010 says, limited by ADR-0054 after device loss. The count of corrupt reads (ADR-0012 rule 3) is a request. A `status` request returns the failure to a client that asks. A service that dies gives every client `PEER_CLOSED`, which a client treats as `StorageFailed`, and restarts as ADR-0054 orders.
- **Handles and rights.** `open(directory, RelativePath, flags, rights, server_end)` attaches a new connection to the end the client passes. The client makes the channel: `fuchsia.io`'s `Open` has the same shape (`stage-4/sources.md#services`). The service keeps the rights per connection: `read`, `write`, `create`, `remove`, `rename`, `publish`, `sync`, `clone`. A request lacking its right gets `AccessDenied`. `clone(rights, server_end)` attaches a connection with equal or fewer rights. A kernel handle cannot express these, so the connection does. Nothing resolves `..` here (ADR-0013).
- **Reads and writes** name an explicit offset, so stage 4 has no file position. A write may instead ask to append, which is atomic (ADR-0010 rule 2).
- **When the service's table runs out.** ADR-0040 gives the service 1024 handles, and each open file holds one. So the service counts the handles it holds for clients: connections, attached memory objects and tokens. `MAX_OPEN` is 896 `[assumption]`: 1024, less 64 of room for reading a message of 64 handles (ADR-0050), less 64 for the service's own port, driver, supervisor channel and margin. A request that would pass it gets `TooManyOpen`, any handle it brought is closed at once, and the connection it came on goes on working. The room holds a new handle only for that moment, so ADR-0050's rule that a server closes a connection on `LimitReached` does not fire while the count holds. This is one limit for the whole service: one client can use all of it, which is acceptable because every process is trusted at stage 4. Sharing it between clients is in ADR-0056.
- **Two objects in one request.** `rename` across directories and `publish` name a second connection. An endpoint cannot be passed for that: the service would receive the client's end and could not match it. As in `fuchsia.io` (`stage-4/sources.md#services`), `get_token` returns a token, a handle to an event object the service keeps a reference to for that connection; the request carries the token, and the service compares it with a new call `object_same(a, b)`. It needs ADR-0006's event object, which stage 4 then builds. The token's connection must hold the right.
- **Closing.** `PEER_CLOSED` on a connection is its close: open-file state goes with the last connection that refers to it, and a nameless file with no connection left is freed (ADR-0011 rule 7). At stage 4 `clone` makes a new record, so that is always the one. A client that dies leaves nothing behind.
- **Data.** Reads and writes up to `MAX_INLINE` 60 KiB `[assumption: ADR-0050's limit less headers]` travel in the message. Above that, the client creates a memory object and sends it once per connection with `attach`; a request then says offset and length. `MAX_ATOMIC_WRITE` stays 1 MiB (ADR-0010 rule 2), reached through the buffer. A connection attaches at most one buffer. The service keeps at most 240 mapped `[assumption]`, which leaves room within ADR-0051's 256 mappings for the block data object of ADR-0054 and its own. The next `attach` gets `LimitReached`, and the client stays within `MAX_INLINE` or closes a connection that has a buffer. The service copies out of the buffer before use (ADR-0051's rule), and its decoder takes those bytes as untrusted.
- **Cost of each.** Inline: two copies each way through ADR-0031's byte loop, three system calls per side. Buffer: one copy plus a request message, and setup. Neither is measured on Zoraya.
- **Paging.** `readdir` returns as many entries as fit, with a cookie. The cookie is opaque to the client.

**Tensions with accepted ADRs.**

- *ADR-0040, Exhaustion.* A slot retires after 65,535 uses, and each `open` passes one handle through the service's table, so a long-lived service wears its table out: about 67 million handles in all, by arithmetic. ADR-0057 proposes to end retirement. Without it, a service must be restarted before that.
- *ADR-0012 rule 4.* "Holding only" the block handle and client channels: the service also holds a supervisor channel and client memory objects. The sentence needs widening.
- *ADR-0016 rule 8.* "After each request" becomes every engine call.

### Effect of the vision
No change for target 1. For target 3, every client waits on one thread during any commit, which a measured desktop may not bear. For target 2, per-connection rights give each sandbox a narrowed directory channel, but one sandbox calling `sync` in a loop stalls all others on the volume, and limits do not fix that, because fairness needs concurrency and the engine's `&mut self` forbids it. The cheap answer is a volume per sandbox. Making the engine concurrent is a rewrite of its request path `[estimate: thousands of lines]`. Threads are not precluded.

### Deferred to stage 6
**One requirement on stage 4.** Its design must not preclude an open file description with a shared position (DM-023), the POSIX object behind `dup`, `fork` and fds passed at `spawn`. Stage 4 meets this by keeping open-file state in a service record (Process, INV-0055-9) and by sending no file position on the wire: a later method, a new ordinal under ADR-0052, can attach a second connection to the same record and add a position to it.

Everything else is left to stage 6, undecided: `libposix` and its mapping of POSIX calls; how a `fork` child, which has no channels (ADR-0003 rule 3, ADR-0050), reconnects, for which a kernel object such as a handoff was sketched and is not introduced here; `pselect` over file handles (ADR-0006 rule 1); `close` returning `EIO` through `status` (ADR-0010); and how far 896 connections go for a POSIX toolchain.

*Note, 2026-11-03 (the architect's decision on ADR-0153, recorded by an agent):* the protocol gains `narrow(name, rights, server_end)` under ADR-0052: a new connection to a directory through which requests may name only that one component, with rights no wider than the caller's, enforced as any connection's rights are. "Create or replace" through it is
creating the name, or opening the file already there for writing with truncation; without `remove` it cannot delete. The shell gives it for operands that create, replace, remove or rename one name (ADR-0153 rule 2).

## Consequences
- **Positive.** ADR-0010's linear order holds with no locks. Files pass as handles. A dead client leaves nothing.
- **Negative.** Everything waits on one thread during a commit. A channel per open costs kernel memory, and one of the service's 1024 handles. Rights are enforced by the service, so they are only as strong as it is, within the volume.
- **Follow-up.** The protocol crate. Measurements of inline and buffer paths. `readahead` is not specified.

## Invariants
- **INV-0055-1.** The service applies requests one at a time, and the reply follows the effect. *Check:* the crash harness of INV-0010-1 run through the service, with a model comparing completion order to reply order.
- **INV-0055-2.** `sync` replies success only after the commit covering every earlier reply. *Check:* host test of the loop with a fake block peer counting flushes.
- **INV-0055-3.** No request succeeds with a right its connection lacks, and `clone` never adds one. *Check:* unit test over every request and right.
- **INV-0055-4.** After `Failed`, every mutating request and `sync` gets `StorageFailed`, and the supervisor hears once, on an idle volume too. *Check:* fault-injecting device, in a host test of the service loop, with a failing timed commit.
- **INV-0055-5.** Buffer bytes are copied before they are decoded or written. *Check:* test with a client changing the buffer during requests, under a checker of the written data's consistency, and review.
- **INV-0055-6.** A client that never reads its replies does not stop the loop serving others. *Check:* host test of the loop with such a client.
- **INV-0055-7.** Between requests, the handles the service holds for clients never exceed `MAX_OPEN`. *Check:* host model test of the loop with random `open`, `clone`, `attach` and closes.
- **INV-0055-8.** A request past `MAX_OPEN` gets `TooManyOpen`, its handle is closed, and the connection it came on keeps working. *Check:* host test opening without end, then an in-QEMU test against the kernel's real table.
- **INV-0055-9.** Open-file state is held in a record separate from the connection, and two connections can refer to one record. *Check:* a host test of the `fs-service` crate attaching two connections to one record through its internal interface.

## Open questions
- As of TASK-12: the protocol is declared and every invariant here waits for the service, in TASK-13.
- Costs of the inline and buffer paths: measure.
- Whether the service's blocking on the driver needs an asynchronous `BlockDevice`.

## Revisit triggers
- A measured commit stall that clients notice.
- `open` traffic that makes a channel per open too costly.
- A workload that reaches 896 connections on a volume: raise `MAX_HANDLES` for services (ADR-0040's revisit), carry several open files on one connection as Option B does, or split the volume.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect, on the condition of 0 errors in the changed lines of the sixth review, which `docs/research/stage-4/review-notes.md`, section "Sixth review, repeated", records.
