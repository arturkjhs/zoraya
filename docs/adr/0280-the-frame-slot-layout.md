# ADR-0280: The frame slot layout and the driver's side of the frame protocol

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0051, ADR-0052, ADR-0054, ADR-0091, ADR-0221, ADR-0230
- **Evidence:** CP-12, CP-15, DM-047, X-5, network/survey.md#the-device-virtio-net-under-qemu

## Context
ADR-0221 item 2 fixes the frame protocol's shape (one memory object of 64 slots of 1536 bytes per direction, slot
indices exchanged in `received`, `transmit` and `release`) and item 1 the driver's (1526-byte receive buffers, no
offloads; the survey's device section, `network/survey.md#the-device-virtio-net-under-qemu`). Building the driver in N1 (TASK-26) needed details the ADR leaves open: where virtio-net's 12-byte header
lives, what a message carries besides slot numbers, how the driver reports a finished or refused transmit, and what
the driver requires of the object. They are small, but a second implementation of `netd` or the driver depends on
them, so they are recorded here instead of silently in code (CLAUDE.md, "Decisions").

**What is measured.** N1 under HVF (`docs/design.md`, "Network driver"): an ARP round trip through the driver and
SLIRP, and the time from the driver's wake to the message read by the client. **What is assumed.** That one
`ethd` and one client is the only shape for the first five tasks.

## Decision drivers
1. The driver never reads or writes frame bytes, so a frame parser cannot be made to run in the process that holds
   DMA authority (ADR-0221 driver 1, CP-15).
2. Every number from the device or from the client is checked in host-tested `no_std` code (INV-0221-2, CP-12), because
   a driver fault must be diagnosable and never a hang (DM-047).
3. A slot is a bare frame, so `netd` hands it to smoltcp without an offset.
4. The client's accounting of slots never stalls on a refusal.

## Considered options

### Option A: the virtio header inside the slot, at offset 0
- **Description:** A slot is 12 bytes of header and then the frame (12 + 1514 = 1526 of the 1536 bytes).
- **Evidence for:** One descriptor per buffer; matches the number 1526 in ADR-0221 literally.
- **Evidence against:** The client must keep the first 12 bytes of every transmit slot zero and skip them on every
  receive; the driver must either write them (touching slot bytes) or trust the client to. A slot is not a frame.
- **Cost:** *(estimate)* Fewer descriptors; an offset in every user of a slot.
- **Precedent:** Linux's virtio-net with `mergeable` off places the header in front of the data.

### Option B: the header in the driver's own queue memory, two descriptors per buffer
- **Description:** A receive buffer is a chain of the driver's 12-byte header cell (one per slot) and the slot; a
  transmit is the driver's single all-zero header cell and the frame. The slot is exactly the Ethernet frame from byte
  0. QEMU accepts split headers (the device reads and writes through iovecs).
- **Evidence for:** Drivers 1 and 3: nothing in a slot but the frame, and the driver does not map the object at all;
  the transmit header is never written after start-up, so no offload bit can be set by a bug. Total buffer size is
  still 12 + 1514 = 1526.
- **Evidence against:** Twice the descriptors (128 per queue for 64 slots), so queue memory is seven pages.
- **Cost:** *(estimate)* Three pages per queue, one more for the cells.
- **Precedent:** Linux's virtio-net uses a separate header descriptor when the data is not contiguous with it.

### Option C: bounce buffers in the driver, copied to and from slots
- **Description:** The driver owns the DMA buffers and copies frames into the client's slots.
- **Evidence for:** The client's object need not be pinned or contiguous.
- **Evidence against:** The driver then touches every frame's bytes (against driver 1), and adds a copy to the two
  ADR-0221 already counts (X-5's bound).
- **Cost:** *(estimate)* A copy of up to 1514 bytes per frame.
- **Precedent:** Drivers without a shared-memory client protocol.

## Recommendation
**Recommendation (not a decision): Option B, with these details.**
1. Slots `0..64` receive, `64..128` transmit; a slot holds one bare Ethernet frame from byte 0; `SLOT_SIZE` 1536.
2. The object must be one contiguous run of device addresses of at least 196,608 bytes (`MemoryFlags::CONTIGUOUS`);
   otherwise `attach` answers `BAD_OBJECT`. One attach per driver life.
3. Frames of 14 to 1514 bytes are carried; a transmit of any other length, of a slot outside `64..128`, or of a slot
   the device still holds is refused and counted. A received frame shorter than 14 bytes is dropped and its slot goes
   back to the device; a used length beyond the buffer, or a used entry that names a chain the driver did not offer, is
   a broken device and ends the driver (reset, unpin, exit).
4. The driver pushes `received { at, frames[] }` (`at`: monotonic nanoseconds at the driver's wake for the interrupt,
   for the latency measurement), `sent { slots[] }` when the device is done with a transmit slot, `rejected { slots[] }`
   with a reason for each transmit it did not carry out, and `link_changed`. A refused transmit is never reported in
   `sent`: reason `STILL_QUEUED` (the device holds the slot, or its `sent` is not delivered yet) leaves the slot with
   the driver until that `sent`; `NOT_ATTACHED` and `BAD_LENGTH` leave it with the client, which was never taken. A
   slot outside `64..128` is only counted. A `received` that cannot be written is undone: the slots go back to the
   device and the frames are counted dropped.
5. Nothing pushed is lost on a full channel, and memory stays bounded. The driver keeps what it owes as state, not as a
   queue: one bit for each transmit slot finished and not yet told, at most one pending `rejected` reason for each
   transmit slot, one pending `info` reply and one pending `attach` reply (the newest request's transaction), and one
   "link changed" flag (the message carries the state when it is written). It writes them again whenever the channel
   takes messages (on every event, and on the channel's `WRITABLE` signal while something is pending). A transmit slot
   is not free for the driver until the `sent` that names it was written, so the client never sees a slot reused
   before it was told. At most 64 messages are read from the client in one pass.
6. Any message the driver cannot decode (an `attach` whose body is bad included), or whose ordinal it does not know,
   ends the driver, as in `blkd`'s hostile client rule; `init` is the supervisor. An `attach` without its handle is
   refused with `BAD_OBJECT`. The driver ends by itself, with exit code 0, after it reset the device and unpinned both
   memories; if the reset fails it ends with code 3 and the pins stay (quarantine, ADR-0054).
7. In N1 `init` starts `ethd` when a virtio-net device is alone on its page and keeps the channel open; nothing
   supervises it until `netd` exists (N2).

## Consequences
- **Positive.** The driver's attack surface is lengths and indices only; the frame parser lives in `netd`.
- **Negative.** 128 descriptors per queue; the contiguity requirement binds `netd`'s allocation (192 KiB, small).
- **Follow-up.** `netd` (N2) creates the object with `CONTIGUOUS`, treats `sent` as the release of a transmit slot (and `rejected` with `STILL_QUEUED` as no release), and
  releases every received slot.

## Invariants
- **INV-0280-1.** The driver reads no byte of any frame slot. *Check:* `zoraya-virtio-net` has `#![forbid(unsafe_code)]` and takes no `Shared` view of the frames object; `ethd` maps only its queue memory (`make check-ethd-maps`, a source check: one `memory_map` of the queue object, one `Shared` implementation, none in the crate; and clippy for `forbid(unsafe_code)`). It is not a run in QEMU: the kernel has no call that lists a process's mappings.

## Open questions
- Whether 64 slots per direction and a single `received` batch of up to 64 are right once `netd` runs (ADR-0221's own
  open question).
- Whether `info` should also give the client a way to ask which transmit slots the driver still owes (it cannot lose
  them now, so this is not needed).

## Revisit triggers
- A device or a client that needs a non-contiguous object: Option C, or runs in the protocol.

## Architect's decision

(Left empty for the architect.)
