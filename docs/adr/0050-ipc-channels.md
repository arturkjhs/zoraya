# ADR-0050: IPC channels

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0031, ADR-0032, ADR-0040, ADR-0041, ADR-0042, ADR-0051
- **Evidence:** DM-023, CP-02, CP-12, CP-14, DM-018, DM-022, GR-002, GR-018, X-5, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4, stage-4/sources.md#channels, alternatives-postmortem.md#redox, stage-4/sources.md#drivers

## Context
Stage 4 puts a driver and a filesystem service in user space, so something must carry requests and handles between processes. ADR-0006 lists a channel with `READABLE` and `PEER_CLOSED`, ADR-0004 rule 7 reserves `PeerClosed` for byte streams, and ADR-0002 rule 4 says the native primitive may carry handles and framing while `libposix` exposes only bytes. Nothing fixes the object. CP-02 and DM-022 leave open how handles in a queue avoid cycles. Nothing is measured on Zoraya; the sizes below are assumptions.

## Decision drivers
1. Handles must move with a message, atomically, without a garbage collector (DM-022).
2. A slow reader must not grow one queue without bound (CP-14). Total memory stays bounded only by `NoMemory` until ADR-0056's charging exists.
3. Failure is a value at the call (CP-12): a full or closed peer never interrupts anyone.
4. Fit ADR-0006's one wait: no second queue beside the port.
5. Service calls must be cheap enough, since IPC sets the ceiling of a user-space system (DM-018), but copying everything has a known bound (X-5).

## Considered options

### Option A: Unix-domain sockets (Linux baseline)
- **Description:** Byte or datagram sockets in a pair, with descriptors passed as ancillary data.
- **Evidence for:** Works everywhere; POSIX programs know it; GR-002's composition.
- **Evidence against:** `SCM_RIGHTS` has years of garbage-collection bugs and an awkward shape (DM-022). A byte stream has no message boundary, so handles must be matched to bytes by convention.
- **Cost:** *(estimate)* Moderate kernel code and a collector.
- **Precedent:** Linux; DM-022 records the outcome.

### Option B: Synchronous rendezvous with a reply (QNX, L4, Hubris)
- **Description:** `send` blocks until the receiver replies; the kernel copies directly between address spaces and queues nothing.
- **Evidence for:** No kernel queues, so no memory to bound (driver 2). Scheduling follows the call (GR-018, `alternatives-postmortem.md#qnx`). Hubris ships it (`alternatives-postmortem.md#hubris`, `stage-4/sources.md#drivers`). L4 kept fast synchronous IPC through 20 years (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** A sender blocks on a stuck server, so a client needs a thread per outstanding call, and Zoraya's processes have one thread and wait on a port (driver 4; DM-018 notes purely synchronous IPC forces multithreaded servers). Its strongest form adds asynchronous notifications, as L4 did after DM-018 and as QNX pulses do, and ADR-0006's port could carry them; that still needs a thread per outstanding call, which processes do not have. Direct copying needs both address spaces at once.
- **Cost:** *(estimate)* Small kernel path, larger cost in every client.
- **Precedent:** QNX, seL4, Hubris.

### Option C: Asynchronous message channel with bounded queues
- **Description:** A pair of endpoints. A write copies bytes and moves handles into the peer's queue, or fails at once. Readiness goes through ADR-0006's port.
- **Evidence for:** One wait for everything (driver 4). Zircon's shape, in production (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `stage-4/sources.md#channels`). Moves handles with no descriptor table in flight (driver 1).
- **Evidence against:** Two copies per message (X-5), and three system calls per side of a round trip. Zircon's documentation states no queue bound, only a warning that undrained channels use excessive kernel memory and may raise an exception (`stage-4/sources.md#channels`), so a hard bound is this ADR's own design.
- **Cost:** *(estimate)* 500 lines. A round trip is unmeasured here.
- **Precedent:** Zircon.

### Option D: A shared-memory ring between each client and server
- **Description:** No kernel data path: processes share a ring and use a signal to wake.
- **Evidence for:** Fastest in bulk; Redox's rings gained 14-fold over its synchronous calls on NVMe (`alternatives-postmortem.md#redox`, a filesystem-free benchmark).
- **Evidence against:** Handles cannot cross shared memory, so a kernel path is needed anyway. Every reader must treat the ring as hostile (X-5).
- **Cost:** *(estimate)* A protocol per ring, and a double-fetch rule everywhere.
- **Precedent:** Redox rings; ADR-0051 keeps this available for bulk data on top of channels.

## Recommendation
Recommended: **Option C**, and bulk data through ADR-0051's memory objects. It is a recommendation, not a decision.

- **Object.** `channel_create(out)`: two endpoints, written as two handles to an output buffer. `channel_read(endpoint, bytes, handles, sizes)` writes its two sizes to an output buffer, on `BufferTooSmall` as well. So ADR-0032's registers stay as they are: status in x0, one value in x1, x2 to x18 preserved. New `Status` values: `WouldBlock`, `PeerClosed`, `BufferTooSmall`. A message is bytes plus handles. Order is FIFO per direction. There is no connection-time negotiation.
- **Limits.** At most `MAX_MESSAGE` 64 KiB and 64 handles per message `[assumption, Zircon's values]`. Each direction queues at most 64 messages and 256 KiB `[assumption]`.
- **Write.** `channel_write(endpoint, bytes, handles)` is all or nothing. Queue full is `WouldBlock`, peer closed is `PeerClosed`. A `move` entry without `TRANSFER`, or a `duplicate` entry without `DUPLICATE`, is `AccessDenied`, the rule ADR-0040 gives for `spawn`. No memory for the message is `NoMemory`. Who pays for a channel is ADR-0056's. Unlike Zircon, on any failure the caller keeps every handle, as `spawn` does (INV-0042-3): a failed call has no effect. A handle entry is `spawn`'s `{handle, rights, mode}`, without the slot, and extends ADR-0040's "only as part of `spawn`".
- **Read.** A buffer too small is `BufferTooSmall`, reporting the sizes, and the message stays. A full handle table is `LimitReached`, and the message stays. A hostile message at the head can therefore block a connection, so a server keeps room for 64 handles and a maximum message, and closes a connection on either error. Empty is `WouldBlock`, or `PeerClosed` once the peer is gone and the queue is drained.
- **Signals.** `READABLE`, `PEER_CLOSED`, as ADR-0006 rule 1, plus `WRITABLE`: asserted while the direction has a free message slot and `MAX_MESSAGE` bytes free. A smaller write may succeed without it. Rule 1 does not list it for a channel: see the tensions below.
- **Closing.** The last handle to an endpoint ending destroys its unread messages and closes their handles (`stage-4/sources.md#channels`), and asserts `PEER_CLOSED` at the peer. Teardown is iterative, with a work list threaded through the messages themselves: nesting depth has no bound, so recursive `Drop` on a 64 KiB stack would be a kernel panic any program could cause. This bounds the stack, not the time: the kernel runs this with interrupts masked (ADR-0041), for as long as the chain lasts. At stage 4 every process is trusted, so a long chain is a bug and not an attack, and INV-0050-6's test measures the stall. ADR-0056 is what bounds a chain for untrusted code.
- **Rights.** `READ`, `WRITE`, `WAIT`, `TRANSFER`. No `DUPLICATE`: an endpoint has one holder, like a port, so `PEER_CLOSED` is exact. `snapshot_process` is stage 6: see the deferred section.
- **No cycles, by rule.** An endpoint cannot be sent through itself or its peer, and an endpoint that has handles queued for reading cannot be moved at all (`BadState`). A cycle of endpoints in each other's queues needs a last edge: moving an endpoint whose queue already holds a handle. So no cycle of endpoints can exist and no collector is needed for them. This does not bound depth or total memory, and it covers endpoints only. A process or job handle can sit in a queue only it can reach; exit and `job_kill` reclaim those. Exception channels (ADR-0004 rule 5) would let a job hold an endpoint, so binding one must extend this rule. CP-02's question stays open for those. The cost: a server drains an endpoint before handing it on, and a peer that keeps queuing handles can pin the endpoint to its holder, so a protocol in which a peer may send unsolicited handles cannot be forwarded.
- **Not built.** A combined `channel_call` until a measurement asks for it. Completion packets (ADR-0006 rule 7) stay reserved.

**Tensions with accepted ADRs.**

- *ADR-0006 rule 1* lists a channel's signals without `WRITABLE`. Its revisit triggers do not foresee this. Accepting this ADR is accepting that row's replacement (ADR-0001 rule 1).
- *ADR-0004 rule 5:* see the no-cycle rule.
- *ADR-0040:* transfer outside `spawn`, above.

### Effect of the vision
None on the choice of Option C, but the details change. For target 2, untrusted agents, memory charging (ADR-0056), which also covers handles in flight, becomes a requirement, and peer-controlled refusal of a move must be designed around. Networking fits: a socket is a service reached by channel, or a later object using the same signals. Threads fit: read and write are atomic per message.

### Deferred to stage 6
What a `snapshot_process` child does without channels (INV-0003-7), and byte streams for `pipe()`. Stage 4 has no `fork` and builds neither. Whether a byte stream is a separate object or a mode of this one is left open: partial reads and writes have nothing in common with whole messages, which suggests a separate object with ADR-0006's three signals. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** One wait for I/O, exit and messages. No collector. A failed write has no effect, so retry is simple.
- **Negative.** Three system calls per side per call until `channel_call` exists. An endpoint with handles queued cannot be forwarded. Two copies through the byte-at-a-time routine of ADR-0031 until it is improved.
- **Follow-up.** A round-trip measurement. Word-sized user copies. Handles in flight leave the table and so escape `MAX_HANDLES`; ADR-0056 covers that.

## Invariants
- **INV-0050-1.** A message is delivered whole, in order, or not at all, and a failed `channel_write` leaves the caller's handle table unchanged. *Check:* in-QEMU test filling a queue, with a handle in the refused message still usable.
- **INV-0050-2.** No channel endpoint can be held only by a queue reachable from itself. *Check:* a host model test that drives random sends and asserts the graph of endpoints in queues stays acyclic, plus in-QEMU tests of the two refusals.
- **INV-0050-3.** Queued bytes and messages per direction never exceed their bounds. *Check:* host test in the channel's crate, and an in-QEMU test seeing `WouldBlock`.
- **INV-0050-4.** Closing an endpoint closes every handle in its unread messages. *Check:* in-QEMU test sending a process handle, closing the reader, and seeing the object freed with the frame and heap counters of INV-0041-1.
- **INV-0050-5.** A channel handle has no `DUPLICATE` right. *Check:* a unit test over `OBJECT_TYPES`.
- **INV-0050-6.** Closing an endpoint uses bounded kernel stack whatever the nesting depth. *Check:* in-QEMU test building a chain far deeper than a kernel stack allows, then closing its head, reporting how long interrupts stayed masked.

## Open questions
- Round-trip cost on Zoraya: measure at stage 4, under HVF and TCG.
- Whether 64 messages and 256 KiB suit a busy service.
- Whether the moved-endpoint rule hurts a real protocol: none of stage 4's do.
- Whether the teardown stall is acceptable: if not, a kernel thread takes the work list and yields between objects.

## Revisit triggers
- A measured round trip that dominates a build: add `channel_call`, or a ring for that path.
- A protocol that must forward an endpoint with handles queued.

## Architect's decision
