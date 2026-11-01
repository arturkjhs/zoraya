# ADR-0050: IPC channels

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0031, ADR-0032
- **Evidence:** CP-02, CP-12, CP-14, DM-018, DM-022, GR-002, GR-018, X-5, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#qnx, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4, stage-4/sources.md#channels, alternatives-postmortem.md#redox

## Context
Stage 4 puts a driver and a filesystem service in user space, so something must carry requests and handles between processes. ADR-0006 lists a channel with `READABLE` and `PEER_CLOSED`, ADR-0004 rule 7 reserves `PeerClosed` for byte streams, and ADR-0002 rule 4 says the native primitive may carry handles and framing while `libposix` exposes only bytes. Nothing fixes the object. CP-02 and DM-022 leave open how handles in a queue avoid cycles. Nothing is measured on Zoraya; the sizes below are assumptions.

## Decision drivers
1. Handles must move with a message, atomically, without a garbage collector (DM-022).
2. A slow reader must not grow kernel memory without bound (CP-14).
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
- **Evidence for:** No kernel queues, so no memory to bound (driver 2). Scheduling follows the call (GR-018, `alternatives-postmortem.md#qnx`). Hubris ships it (`alternatives-postmortem.md#hubris`, `stage-4/sources.md#channels`). L4 kept fast synchronous IPC through 20 years (`alternatives-postmortem.md#l4-family-and-sel4`).
- **Evidence against:** A sender blocks on a stuck server, so a client needs a thread per outstanding call, and Zoraya's processes have one thread and wait on a port (driver 4; DM-018 notes purely synchronous IPC forces multithreaded servers). A server cannot wait on a port and receive by the same primitive without adding notifications. Direct copying needs both address spaces at once.
- **Cost:** *(estimate)* Small kernel path, larger cost in every client.
- **Precedent:** QNX, seL4, Hubris.

### Option C: Asynchronous message channel with bounded queues
- **Description:** A pair of endpoints. A write copies bytes and moves handles into the peer's queue, or fails at once. Readiness goes through ADR-0006's port.
- **Evidence for:** One wait for everything (driver 4). Zircon's shape, in production (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `stage-4/sources.md#channels`). Moves handles with no descriptor table in flight (driver 1).
- **Evidence against:** Two copies per message (X-5), and three system calls per side of a round trip. Zircon's documentation states no queue bound (`stage-4/sources.md#channels`), so bounding it is this ADR's own design.
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

- **Object.** `channel_create() -> (a, b)`: two endpoints. A message is bytes plus handles. Order is FIFO per direction. There is no connection-time negotiation.
- **Limits.** At most `MAX_MESSAGE` 64 KiB and 64 handles per message `[assumption, Zircon's values]`. Each direction queues at most 64 messages and 256 KiB `[assumption]`.
- **Write.** `channel_write(endpoint, bytes, handles)` is all or nothing. Queue full is `WouldBlock`, peer closed is `PeerClosed`, a handle without `TRANSFER` is `AccessDenied`. Unlike Zircon, on any failure the caller keeps every handle, as `spawn` does (INV-0042-3): a failed call has no effect.
- **Read.** `channel_read(endpoint, bytes, handles)` returns sizes. A buffer too small is `BufferTooSmall`, reporting the sizes, and the message stays. A full handle table is `LimitReached`, and the message stays. Empty is `WouldBlock`, or `PeerClosed` once the peer is gone and the queue is drained.
- **Signals.** `READABLE`, `PEER_CLOSED`, as ADR-0006 rule 1, plus `WRITABLE`, needed by the backpressure above. Rule 1 does not list it for a channel: see the tension below.
- **Closing.** The last handle to an endpoint ending destroys its unread messages and closes their handles (`stage-4/sources.md#channels`), and asserts `PEER_CLOSED` at the peer.
- **Rights.** `READ`, `WRITE`, `WAIT`, `TRANSFER`. No `DUPLICATE`: an endpoint has one holder, like a port, so `PEER_CLOSED` is exact. `snapshot_process` leaves such slots empty (INV-0003-7), which answers ADR-0003's open question on a duplicated endpoint for this type.
- **No cycles, by rule.** An endpoint cannot be sent through itself or its peer, and an endpoint that has handles queued for reading cannot be moved at all (`BadState`). A cycle of endpoints in each other's queues needs a last edge: moving an endpoint whose queue already holds a handle. So no such cycle can exist, no collector is needed, and CP-02's open question closes. The cost: a server drains an endpoint before handing it on.
- **Byte streams.** A separate object with ADR-0006's three signals, at stage 6, not a mode of this one. Partial reads and writes have nothing in common with whole messages, and no stage 4 work needs them.
- **Not built.** A combined `channel_call` until a measurement asks for it. Completion packets (ADR-0006 rule 7) stay reserved.

**Tension with accepted ADRs.** ADR-0006 rule 1 lists a channel's signals without `WRITABLE`. Adding one is an extension that its revisit trigger foresees, but the accepted text must be superseded if this is accepted.

### Effect of the vision
None on the choice. For target 2, untrusted agents, the bounds and the no-cycle rule matter more, and memory charging (ADR-0041's open question) becomes a requirement. Networking fits: a socket is a service reached by channel, or a later object using the same signals. Threads fit: read and write are atomic per message.

## Consequences
- **Positive.** One wait for I/O, exit and messages. No collector. A failed write has no effect, so retry is simple.
- **Negative.** Three system calls per side per call until `channel_call` exists. An endpoint with handles queued cannot be forwarded. Two copies through the byte-at-a-time routine of ADR-0031 until it is improved.
- **Follow-up.** A superseding ADR-0006 row for `WRITABLE`. A round-trip measurement. Word-sized user copies.

## Invariants
- **INV-0050-1.** A message is delivered whole, in order, or not at all, and a failed `channel_write` leaves the caller's handle table unchanged. *Check:* in-QEMU test filling a queue, with a handle in the refused message still usable.
- **INV-0050-2.** No handle can reach a state in which only a channel queue reachable from itself holds it. *Check:* a host model test that drives random sends and asserts the graph of endpoints in queues stays acyclic, plus in-QEMU tests of the two refusals.
- **INV-0050-3.** Queued bytes and messages per direction never exceed their bounds. *Check:* host test in the channel's crate, and an in-QEMU test seeing `WouldBlock`.
- **INV-0050-4.** Closing an endpoint closes every handle in its unread messages. *Check:* in-QEMU test sending a process handle, closing the reader, and seeing the object freed with the frame and heap counters of INV-0041-1.
- **INV-0050-5.** A channel handle has no `DUPLICATE` right. *Check:* a unit test over `OBJECT_TYPES`.

## Open questions
- Round-trip cost on Zoraya: measure at stage 4, under HVF and TCG.
- Whether 64 messages and 256 KiB suit a busy service.
- Whether the moved-endpoint rule hurts a real protocol: none of stage 4's do.

## Revisit triggers
- A measured round trip that dominates a build: add `channel_call`, or a ring for that path.
- A protocol that must forward an endpoint with handles queued.

## Architect's decision
