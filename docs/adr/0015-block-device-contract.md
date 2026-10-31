# ADR-0015: Block device contract for the filesystem

- **Status:** accepted
- **Date:** 2026-10-25
- **Depends on:** ADR-0001 (rules 6 and 8 refer to ADR-0010 and ADR-0012 only to say what the consumer does; they are informative)
- **Evidence:** CP-12, GR-021, fs/survey.md#block-devices, fs/survey.md#ext4, fs/survey.md#littlefs, fs/survey.md#redoxfs, fs/survey.md#sources, fs/survey.md#testing-crash-consistency

## Context

The filesystem (ADR-0010, ADR-0011) runs on a block device. On QEMU `virt` that device is virtio-blk (roadmap stage
4; GR-021). This ADR states what the filesystem assumes about the device: block size, atomicity of writes, reordering,
flush and FUA, and error reporting. The assumptions must match the virtio specification, and where they rely on it
this ADR says so. The same contract is the interface of the in-memory test device used by every crash test.

**What the virtio specification says** (OASIS VIRTIO, section "Block Device", subsections "Device Operation" and its
device requirements; quoted in `fs/survey.md#block-devices`, source F-20 in `fs/survey.md#sources`):

- Requests are "serviced (probably out of order) by the device"; the device uses them "not necessarily in order".
- Sector numbers and lengths are always in 512-byte units; `blk_size` (feature `VIRTIO_BLK_F_BLK_SIZE`) is the
  "optimal sector size" and "does not affect the units used in the protocol".
- **Durability.** "A write is considered volatile when it is submitted; the contents of sectors covered by a volatile
  write are undefined in persistent device backend storage until the write becomes stable." A write becomes stable
  once it is completed and one of: (1) neither `VIRTIO_BLK_F_CONFIG_WCE` nor `VIRTIO_BLK_F_FLUSH` was negotiated but
  `VIRTIO_BLK_F_FLUSH` was offered; (2) the cache is in writethrough mode for the whole time between submission and
  completion; (3) the write carried the FUA flag (feature `VIRTIO_BLK_F_REQ_FLAGS_OUT_FUA`); (4) a
  `VIRTIO_BLK_T_FLUSH` request is sent *after the write is completed* and is completed itself. A device backed by
  persistent storage "MUST ensure that stable writes are committed".
- A device that does not offer `VIRTIO_BLK_F_FLUSH` "MAY" commit writes before completing them, but need not; such
  an implementation "will not be resilient to data loss in case of crashes".
- Status per request: `OK`, `IOERR` ("device or driver error"), `UNSUPP`; for a multi-segment discard or write-zeroes
  that fails, the state of each segment is indeterminate.
- The specification says nothing about write atomicity beyond the "undefined until stable" rule above.

**What QEMU does** (`fs/survey.md#block-devices`, source F-21): with its default device properties (`config-wce` on,
`x-enable-wce-if-config-wce` true) it offers `VIRTIO_BLK_F_FLUSH` (under its legacy name `WCE`) and
`VIRTIO_BLK_F_CONFIG_WCE` in **every** cache mode, so a guest can read and switch the cache mode; only with
`config-wce=off` (or an old machine type) does the offer depend on the drive's write cache being enabled. Its
virtio-blk model does not offer the FUA request flag. Its default cache mode is `writeback`; `cache=unsafe`
("cache.no-flush") ignores flushes.

**Linux's model** for comparison: filesystems mark a request `REQ_PREFLUSH` (flush the volatile cache before this
write) and/or `REQ_FUA` (complete only when this write is on non-volatile storage); the block layer emulates both on
devices that lack them (`fs/survey.md#block-devices`, F-22). `dm-log-writes` records writes in the order that a power failure could expose: a write
counts only once a flush issued after its completion completes (`fs/survey.md#testing-crash-consistency`).

**What is measured.** Nothing about devices: TASK-02 ran on a Linux VM's own disk. This ADR rests on the
specification, not on data.

## Decision drivers

1. **Assume no more than the virtio specification guarantees** (GR-021: drivers that trusted the device had to be
   retrofitted).
2. **The contract must be simulable exactly by an in-memory test device** so that crash testing covers every state
   the real device could produce (`fs/survey.md#testing-crash-consistency`).
3. **ADR-0010's durability claim must be true on QEMU's default configuration.**
4. **Errors must reach the filesystem as typed results** so ADR-0010 rule 6 can act on them (CP-12).
5. **Do not depend on features QEMU does not implement** (FUA).

## Considered options

### Option A: Linux block-layer model with sector atomicity (baseline)

- **Description:** Assume a write of one 512-byte (or physical-block) sector is atomic across power loss; use
  `PREFLUSH` and `FUA` as ordering tools; let the block layer emulate FUA with a flush where missing.
- **Evidence for:** What Linux filesystems are written against (`fs/survey.md#block-devices`); journal
  commit records are often sized to one sector for this reason [inference]. FUA saves one flush per commit.
- **Evidence against:** Virtio says the contents of every sector of a volatile write are *undefined* until stable,
  not "old or new" (driver 1). QEMU does not offer FUA (driver 5). A sector-atomicity assumption cannot be simulated
  faithfully by a test device without deciding what the real device does, which the specification does not say.
- **Cost:** *(estimate)* Slightly fewer flushes per commit; a hidden assumption.
- **Precedent:** ext4, XFS; their journals discard a transaction whose commit record is missing or fails its checksum
  (`fs/survey.md#ext4`), which suggests the single-sector assumption is not relied on alone `[inference]`.

### Option B: The virtio contract as written: no atomicity, ordering only by flush

- **Description:** The device stores what it was given only for writes that have become stable; any sector of a
  write that is not yet stable may, after a crash, hold the old contents, the new contents, or anything else.
  Completed writes become stable only through a flush issued after their completion. Requests in flight have no
  order among themselves. FUA is used if negotiated, never required.
- **Evidence for:** Exactly the specification (driver 1); exactly simulable: a test device keeps, per sector, the
  last stable content and the set of unstable writes, and a crash picks any combination (driver 2). ADR-0011's
  copy-on-write commit needs nothing more: checksums detect torn root records and a ring of root slots keeps an older
  valid one, as RedoxFS does (`fs/survey.md#redoxfs`). littlefs is designed for power loss "at any time" on hardware with even fewer
  guarantees (`fs/survey.md#littlefs`).
- **Evidence against:** Two flushes per commit (before and after the root record) where FUA could save one. The test
  device's crash state space is larger than under Option A (any content for unstable sectors).
- **Cost:** *(estimate)* One extra flush per commit on QEMU (FUA absent anyway).
- **Precedent:** dm-log-writes' worst-case model, CrashMonkey's bounded enumeration
  (`fs/survey.md#testing-crash-consistency`).

### Option C: Assume larger atomic writes (NVMe-style atomic write units)

- **Description:** Assume writes up to some size (for example 4 KiB or 16 KiB) are atomic, and design commit records
  to fit.
- **Evidence for:** Simplifies commit records and allows in-place updates of single blocks.
- **Evidence against:** Nothing in the virtio specification provides it (driver 1).
- **Cost:** *(estimate)* Unverifiable assumption.
- **Precedent:** Database engines that rely on device-reported atomic write units `[inference; not in the record]`.

### Option D: Flash-level contract (littlefs: program, erase, power loss at any time; non-Unix precedent)

- **Description:** Model the device as erase blocks and program operations, with power loss at any instant.
- **Evidence for:** The most conservative model; littlefs survives it in 7,700 lines (`fs/survey.md#littlefs`).
- **Evidence against:** virtio-blk is not flash: any sector can be overwritten directly, with no erase-before-write constraint (F-20). Modelling
  erase would add work with no device to justify it.
- **Cost:** *(estimate)* Wear levelling and erase management for nothing.
- **Precedent:** littlefs, SPIFFS, JFFS (`fs/survey.md#littlefs`).

## Recommendation

**Recommendation (not a decision): Option B.** The contract, as the trait the filesystem core crate is written
against and that both the virtio driver and the test device implement:

1. **Geometry.** The device reports its size in 512-byte sectors (virtio units). The filesystem uses 4,096-byte
   blocks aligned to 4,096 bytes and issues only whole-block reads and writes `[assumption: 4 KiB; blk_size is
   advisory]`. Space past the last whole block is unused.
2. **Writes.** A write is *volatile* from submission. After a crash, every 512-byte sector of every write that was not
   stable may contain its old content, its new content or arbitrary bytes, independently of every other sector. No
   size of write is atomic.
3. **Stability.** A write is stable when it has completed and either it carried FUA (only if negotiated) or a flush
   that was issued after its completion has completed. Nothing else makes a write stable. The filesystem never
   assumes order between requests that are in flight at the same time.
4. **Flush.** `flush()` completes after every write that had completed before the flush was issued is stable.
5. **Read-write mount requires a flush.** The driver negotiates `VIRTIO_BLK_F_FLUSH`. If the device does not offer
   it, the filesystem mounts the volume read-only, because the specification does not require such a device to
   commit anything (case 1 of the stable-write rule needs the feature to be *offered*). An explicit "unsafe" mount
   flag may override this for tests that do not crash; crash tests never use it. On QEMU with default properties the
   flush feature is always offered (F-21), so this rule matters for other devices or `config-wce=off`. The driver
   does not negotiate `VIRTIO_BLK_F_CONFIG_WCE` in the first release: it leaves the cache mode as the device set it
   and relies only on flush. QEMU's `cache=unsafe` ignores flushes and is allowed only for tests that do not crash.
6. **Errors.** Every request completes with `Ok`, `IoError` or `Unsupported`, as a typed result at the request site
   (CP-12). A failed write leaves its sectors undefined. A failed flush means that nothing since the last successful
   flush is known to be stable. The filesystem treats any write or flush error as a write-back failure (ADR-0010
   rule 6) and any read error as `Corrupt`-equivalent for that block (ADR-0012 rule 3). It does not retry.
7. **Discard and write-zeroes** are not used in the first release (their failure semantics are per-segment
   indeterminate, F-20).
8. **The device is untrusted** for memory safety: the virtio driver validates every length and index the device
   returns (GR-021), and the filesystem validates every byte it reads (ADR-0012 rule 4).

**Why B.** It is the specification, not an interpretation of it (driver 1), it is the only option a test device can
reproduce exactly (driver 2), and ADR-0011's design does not need anything stronger. A and C assume what virtio does
not promise; D models hardware we do not have.

**Principles this recommendation strains.** None.

## Consequences

- **Positive.** Crash tests cover every state the device may legally produce. The filesystem is correct on any device
  that implements the virtio contract, including future real devices that implement virtio (GR-021).
- **Negative.** Two flushes per commit; on QEMU with `cache=writeback`, each flush is a host `fsync` of the image file
  `[inference from F-21's cache table]`, so commits are not free. Devices without flush are read-only.
- **Follow-up work.** The `BlockDevice` trait in a `no_std` crate; the recording, crash-simulating test device; the
  virtio-blk driver (stage 4) with flush negotiation and untrusted-device validation; the `tools/run-qemu.sh` drive
  options for tests (default `cache=writeback`; `cache=unsafe` only for non-crash tests).

## Invariants

- **INV-0015-1.** The test device implements exactly rules 2–4: after a simulated crash, a sector holds its last stable content unless a write covering it was not stable, in which case it holds the old content, the new content or "arbitrary", chosen independently per sector by the test; "arbitrary" is represented finitely by a fixed set of patterns (all zeros and a seeded pseudo-random pattern) in both the device and the model. *Check:* unit tests of the test device itself, enumerating small write/flush histories and comparing the finite set of possible crash images with an independent brute-force model that uses the same representation.
- **INV-0015-2.** The filesystem core never depends on order between concurrently in-flight requests: its tests pass when the test device completes in-flight requests in any order. *Check:* the crash harness of INV-0010-1 run with a device that permutes completion order randomly (fixed seeds).
- **INV-0015-3.** A volume on a device that did not negotiate flush is never mounted read-write without the explicit unsafe flag. *Check:* a test with a device that reports no flush support, asserting that a read-write mount fails and a read-only mount succeeds.
- **INV-0015-4.** Every device error is surfaced to the filesystem core as a typed result, and every write or flush error moves the volume to `Failed` (ADR-0010 rule 6). *Check:* the fault-injecting test device of INV-0010-3, plus a driver unit test that maps each virtio status to the trait's error type.

## Open questions

- Should the driver negotiate `VIRTIO_BLK_F_CONFIG_WCE` and switch QEMU's cache to writethrough (each write stable
  on completion, case 2 of the specification) to save flushes? Rule 3 does not rely on it; a stage-4 measurement can
  decide.
- Block size: 4 KiB is assumed; measure whether larger blocks reduce ADR-0011's write amplification.
- Multi-queue virtio-blk (`VIRTIO_BLK_F_MQ`) and request merging are performance questions for the driver.
- *Check status (TASK-07, ADR-0001 rule 6).* INV-0015-4's fault-device half is checked in
  `crates/zoraya-fs/tests/faults.rs`; its driver half (each virtio status mapped to the trait's error type) waits
  for the virtio-blk driver of stage 4.

## Revisit triggers

- A supported device offers FUA: use it for root records (one flush fewer per commit); no contract change.
- A real-hardware target appears (out of scope today, `docs/design.md` Platform): its storage contract (NVMe, SD/eMMC)
  needs its own ADR.
- A device is observed to violate rule 3 (claims stable but loses data): document it and refuse read-write mounts on
  it.

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. Reasons and open conditions: [filesystem-batch-selection.md](filesystem-batch-selection.md).
