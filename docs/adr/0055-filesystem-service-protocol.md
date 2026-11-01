# ADR-0055: Filesystem service protocol

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0005, ADR-0040, ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0016, ADR-0050, ADR-0051, ADR-0052, ADR-0054, ADR-0056
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

- **Process.** One process per mounted volume, one thread, holding `zoraya_fs::Volume` over a `BlockDevice` that speaks to the driver over a channel. A thin `fs-service` crate owns all IPC, behind a transport trait so that its loop runs on the host and in the crash harness. `zoraya-fs` gains one method, `Volume::device_lost` (ADR-0054).
- **The loop.** One port; a subscription per client channel for `READABLE | PEER_CLOSED`, and `PEER_CLOSED` on the driver channel. Take a request, decode it, run it on the `Volume`, reply. With one thread and synchronous engine calls, requests take effect one at a time, so the loop's order is ADR-0010's completion order (rule 1), and each request is atomic to other clients without further locking. The port's deadline drives `T_COMMIT`.
- **Replies.** One request at a time per connection: the service does not read a connection whose reply is waiting. If `channel_write` of a reply gives `WouldBlock`, the service holds that one reply and changes the connection's subscription to `WRITABLE | PEER_CLOSED`, dropping `READABLE`, so a queued request cannot make a `Level` subscription spin. It then goes on with other clients. If a reply gets `QuotaExceeded` or `NoMemory`, `WRITABLE` says nothing about it, so the service closes that connection: the client's job pays for the queue (ADR-0056), so that client is the one at its limit. Held replies total at most 4 MiB `[assumption]`, and past that, a reply that cannot be queued closes its connection in the same way. So a client that never reads stalls or loses only itself. A reply goes after the effect. `sync`, and a request with `durable`, get their reply only after the commit that covers them (ADR-0011 rule 3f). A blocking commit stalls the loop; that is acceptable at stage 4 and is measured.
- **Failure.** A request on a `Failed` volume gets `StorageFailed`, sticky, from the `Volume` itself. `take_failure_report` is polled after every engine call, including the timed commit, and its result goes as a message on the supervisor channel of ADR-0053, which settles ADR-0010's assumption and ADR-0016 rule 8. Reads go on as ADR-0010 says, limited by ADR-0054 after device loss. The count of corrupt reads (ADR-0012 rule 3) is a request. A `status` request returns the failure to a client that asks. A service that dies gives every client `PEER_CLOSED`, which a client treats as `StorageFailed`, and restarts as ADR-0054 orders.
- **Handles and rights.** `open(directory, RelativePath, flags, rights, server_end)` attaches a new connection to the end the client passes. The client makes the channel, so its job pays for it (ADR-0056), and `fuchsia.io`'s `Open` has the same shape (`stage-4/sources.md#services`). The service keeps the rights per connection: `read`, `write`, `create`, `remove`, `rename`, `publish`, `sync`, `clone`. A request lacking its right gets `AccessDenied`. `clone(rights, server_end, budget)` attaches a connection with equal or fewer rights. A kernel handle cannot express these, so the connection does. Nothing resolves `..` here (ADR-0013).
- **Reads and writes** name an explicit offset, so stage 4 has no file position. A write may instead ask to append, which is atomic (ADR-0010 rule 2).
- **Connection budgets.** ADR-0040 gives the service 1024 handles, and each open file holds one, so one client could fill the table and shut out the rest. The kernel cannot stop this: the table is the service's, and ADR-0056's limits count the client's memory, not the service's slots. So the service counts, in *accounts*. An account is a budget of service handles. Every connection belongs to one, and a connection made by `open` or `clone` joins its parent's account, unless `clone` makes a new one. So "one client" means one account to the service, here and wherever this ADR limits a client: it has no other identity to go on. Each connection counts one against its account. So does each attached memory object and each token, since each holds a service handle.
  - **Root.** The first account has 896 handles `[assumption]`: 1024, less 64 of room for reading a message (ADR-0050), less 64 for the service's own port, driver, supervisor channel and margin. Its connection goes to `init`.
  - **Carving.** `clone(rights, server_end, budget)` with a budget of `n` makes a new account, taking `n` from the caller's account until the new one's last connection closes. It is a reservation: budgets never sum past the root, so no account can touch another's. It works as ADR-0056's limits do. `init` gives each client or session a budget this way, and a client can only subdivide its own.
  - **At the budget.** The request gets `TooManyOpen`, its `server_end` is closed, and the connection it came on goes on working. The service holds the new end only for that moment, inside the 64 of room, since each request of this protocol carries at most one handle. So the rule in ADR-0050 that a server closes a connection on `LimitReached` never fires for a client within this protocol, and the room is always there at the next read.
  - **What it costs.** A volume serves at most 896 connections, whatever the budgets. The answers are in the revisit triggers.
- **Two objects in one request.** `rename` across directories and `publish` name a second connection. An endpoint cannot be passed for that: the service would receive the client's end and could not match it. As in `fuchsia.io` (`stage-4/sources.md#services`), `get_token` returns a token, a handle to an event object the service keeps a reference to for that connection; the request carries the token, and the service compares it with a new call `object_same(a, b)`. It needs ADR-0006's event object, which stage 4 then builds. The token's connection must hold the right.
- **Closing.** `PEER_CLOSED` on a connection is its close: open-file state goes, and a nameless file with no connection left is freed (ADR-0011 rule 7). A client that dies leaves nothing behind.
- **Data.** Reads and writes up to `MAX_INLINE` 60 KiB `[assumption: ADR-0050's limit less headers]` travel in the message. Above that, the client creates a memory object and sends it once per connection with `attach`; a request then says offset and length. `MAX_ATOMIC_WRITE` stays 1 MiB (ADR-0010 rule 2), reached through the buffer. A connection attaches at most one buffer, and an account has at most 4 at a time; a fifth `attach` gets `LimitReached`, and the client detaches one or stays within `MAX_INLINE`. With at most 64 accounts `[assumption]`, ADR-0051's 256 mappings are enough, and `clone` refuses a 65th with `TooManyOpen`. An account is usually one client program, so a program that needs more concurrent large transfers asks for more accounts. The service copies out of the buffer before use (ADR-0051's rule), and its decoder takes those bytes as untrusted.
- **Cost of each.** Inline: two copies each way through ADR-0031's byte loop, three system calls per side. Buffer: one copy plus a request message, and setup. Neither is measured on Zoraya.
- **Paging.** `readdir` returns as many entries as fit, with a cookie. The cookie is opaque to the client.
**Tensions with accepted ADRs.**

- *ADR-0012 rule 4.* "Holding only" the block handle and client channels: the service also holds a supervisor channel and client memory objects. The sentence needs widening.
- *ADR-0016 rule 8.* "After each request" becomes every engine call.

### Effect of the vision
No change for target 1. For target 3, every client waits on one thread during any commit, which a measured desktop may not bear. For target 2, per-connection rights give each sandbox a narrowed directory channel, but one sandbox calling `sync` in a loop stalls all others on the volume, and limits do not fix that, because fairness needs concurrency and the engine's `&mut self` forbids it. The cheap answer is a volume per sandbox. Making the engine concurrent is a rewrite of its request path `[estimate: thousands of lines]`. Threads are not precluded.

### Deferred to stage 6
**One requirement on stage 4.** Its design must not preclude an open file description with a shared position (DM-023), the POSIX object behind `dup`, `fork` and fds passed at `spawn`. Stage 4 meets this by keeping open-file state in a service record that a connection refers to, not in the connection itself, and by sending no file position on the wire: a later method, a new ordinal under ADR-0052, can attach a second connection to the same record and add a position to it.

Everything else is left to stage 6, undecided: `libposix` and its mapping of POSIX calls; how a `fork` child, which has no channels (ADR-0003 rule 3, ADR-0050), reconnects, for which a kernel object such as a handoff was sketched and is not introduced here; `pselect` over file handles (ADR-0006 rule 1); `close` returning `EIO` through `status` (ADR-0010); and how far 896 connections go for a POSIX toolchain.

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
- **INV-0055-6.** A client that never reads its replies stalls only itself. *Check:* host test of the loop with such a client.
- **INV-0055-7.** The budgets of all accounts sum to at most the root's, and the service's handles in use never exceed that sum plus its own. *Check:* host model test of the loop with random `open`, `clone`, `attach` and closes.
- **INV-0055-8.** A client at its budget gets `TooManyOpen`, keeps the connection it asked on, and no other account's request is refused. *Check:* host test with one client opening without end, and a second one opening to its full budget, then an in-QEMU test against the kernel's real table.

## Open questions
- Costs of the inline and buffer paths: measure.
- How `init` should divide 896 handles among clients.
- Whether the service's blocking on the driver needs an asynchronous `BlockDevice`.

## Revisit triggers
- A measured commit stall that clients notice.
- `open` traffic that makes a channel per open too costly.
- A workload that reaches 896 connections on a volume: raise `MAX_HANDLES` for services (ADR-0040's revisit), carry several open files on one connection as Option B does, or split the volume.

## Architect's decision
