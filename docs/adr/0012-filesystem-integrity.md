# ADR-0012: Filesystem integrity: checksums, mismatch handling and crafted images

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** ADR-0002, ADR-0010, ADR-0011, ADR-0015
- **Evidence:** CP-15, CP-17, CP-26, DM-037, GR-014, GR-017, GR-021, fs/requirements.md#13-open-flags-and-who-uses-them, fs/requirements.md#5-questions-the-data-cannot-answer, fs/requirements.md#sources-and-queries, fs/survey.md#btrfs, fs/survey.md#comparison, fs/survey.md#ext4, fs/survey.md#failure-modes-from-the-task-01-catalogue, fs/survey.md#littlefs, fs/survey.md#plan-9-cwfs-fossil-and-venti, fs/survey.md#redoxfs, fs/survey.md#testing-crash-consistency, fs/survey.md#xfs, fs/survey.md#zfs

## Context

This ADR decides whether the filesystem checksums metadata and data, with which algorithm, what happens when a
checksum does not match, and what the filesystem must do to survive a maliciously crafted image. The task brief says
to assume that removable or downloaded images can be hostile. The filesystem is a user-space service (ADR-0002); the
device is virtio-blk (ADR-0015), which hands the filesystem whatever bytes the host's image file contains.

**What is measured.** Nothing about corruption: no workload read a corrupted block, and no crash was injected
(`fs/requirements.md#5-questions-the-data-cannot-answer`). On cost, the data shows that file reads are on the hot
path: tcc alone opened files read-only 3,998 times in T1 and 24,796 times in W4 (headers and sources,
`fs/requirements.md#13-open-flags-and-who-uses-them`). The `read`/`write` call totals cannot be used to compare file
reads with file writes, because they include pipes and terminals (`fs/requirements.md#sources-and-queries`).

**What the record says.**

- Devices corrupt data silently: a study of 1.53 million drives found more than 400,000 checksum mismatches that only
  the layer above the drives detected (GR-014). Journaling does not detect silent data corruption (GR-014). ext4 and XFS added metadata checksums late and have none on data (`fs/survey.md#ext4`,
  `fs/survey.md#xfs`). ZFS and btrfs checksum data and metadata and repair from redundant copies (GR-014,
  `fs/survey.md#zfs`, `fs/survey.md#btrfs`); ZFS stores each block's checksum in the block pointer that references it
  (GR-014), as RedoxFS does (`fs/survey.md#redoxfs`); where btrfs stores its checksums is not in the record.
- XFS's "self-describing metadata" adds to each metadata block its filesystem UUID, its own block number, its owner
  and the log sequence number, because a block number alone does not catch a write misdirected to another device
  (`fs/survey.md#xfs`).
- Memory corruption is not covered by on-disk checksums (GR-014).
- Crafted images: fuzzers that mutate images and system calls together find memory-safety bugs in Linux filesystems
  (Janus, `fs/survey.md#testing-crash-consistency`); XFS maintainers describe crafted-image reports as an ongoing
  burden (`fs/survey.md#xfs`). From a search summary only (U-04, not fetched): syzbot generates deliberately corrupted
  images, one Linux maintainer's position is that nothing can be done about a deliberately corrupted filesystem, and
  distributions auto-mount images from USB devices (`fs/survey.md#testing-crash-consistency`). The device-side analogue is
  GR-021: Linux virtio drivers were written trusting the device and had to be hardened.
- Checksums detect accidents, not attackers: whoever crafts an image recomputes them [inference].

## Decision drivers

1. **Silent corruption is real and only an end-to-end check detects it** (GR-014).
2. **The checksum must also catch lost and misdirected writes**: a stale block that is internally consistent passes
   its own checksum (`fs/survey.md#xfs`; GR-014's parent-pointer design).
3. **A detected mismatch must never become wrong data returned to a client or a silent repair** (ADR-0010 rule 6;
   DM-037's lesson that errors must be reported where they matter).
4. **A crafted image must not compromise anything beyond the volume itself** (CP-15: address spaces are the isolation
   boundary; CP-17: components are isolated and restartable; GR-021).
5. **Small and in-house**: no external crate unless justified (repository rules, `docs/dependencies.md`); the
   algorithm must be cheap on the read path.
6. **Room to change the algorithm later** without a format break (contrast XFS's deliberate "no provision for
   changing the integrity checking mechanism", `fs/survey.md#xfs`).

## Considered options

### Option A: Metadata checksums only, in each block (ext4, XFS; Linux baseline)

- **Description:** Each metadata block carries a CRC32c of itself (and, as in XFS, identity fields). File data is not
  checksummed.
- **Evidence for:** Mature and cheap: CRC32c is hardware-accelerated on common CPUs, and XFS argues that for blocks of
  at most 64 KiB a 32-bit CRC is "more than sufficient" (`fs/survey.md#xfs`). No read-path cost for data.
- **Evidence against:** Data corruption is returned to applications (GR-014; driver 1). A lost write of a metadata
  block leaves an older, internally valid block that the block's own checksum accepts (driver 2), unless identity
  fields like XFS's LSN are cross-checked.
- **Cost:** *(estimate)* Small: a CRC function and a verify call per metadata read.
- **Precedent:** ext4 (2012 retrofit), XFS (self-describing metadata).

### Option B: Checksums of every block stored in its parent pointer, plus self-describing headers (ZFS, RedoxFS; btrfs checksums all blocks too)

- **Description:** Every block, metadata and data, is checksummed; the checksum is stored in the pointer that
  references the block (so verification checks *that the parent expected this content*), and every metadata block
  also carries a header with its type, volume identifier, own address and commit generation. Root records carry their
  own checksum (ADR-0011).
- **Evidence for:** Detects corruption, lost writes and misdirected writes for data and metadata (drivers 1, 2); ZFS's
  design "held up" and was robust under fault injection (GR-014). In a copy-on-write tree (ADR-0011) the parent is
  rewritten anyway, so storing the child's checksum there costs no extra write (`fs/survey.md#comparison`). RedoxFS
  does this in Rust with a 64-bit SeaHash per block pointer (`fs/survey.md#redoxfs`).
- **Evidence against:** Every data read pays a checksum (driver 5). No redundancy means detection without repair:
  with one copy, a mismatch is an error, not a fix (GR-014 repairs only "if redundancy exists"). Overwrite-heavy
  workloads fragment under the copy-on-write that this relies on (GR-014, FM-11).
- **Cost:** *(estimate)* Small on top of ADR-0011: a checksum field per pointer and a verify call per read; CRC32C
  in-house is under 100 lines with a table, and can use the CPU's CRC instructions where present.
- **Precedent:** ZFS (Fletcher-4 by default, SHA-256/BLAKE3 optional, `fs/survey.md#zfs`), btrfs (several
  algorithms), RedoxFS (SeaHash), APFS for metadata only (GR-014).

### Option C: Cryptographic content addressing / hash trees (Plan 9 Venti; non-Unix precedent)

- **Description:** Blocks are named by a cryptographic hash of their content; the tree of hashes ends in one root
  hash. Integrity is a property of the address.
- **Evidence for:** Venti's design "enforces a write-once policy, preventing accidental or malicious destruction of
  data" (`fs/survey.md#plan-9-cwfs-fossil-and-venti`). If the root hash is authenticated (signed, or held by a trusted
  party), *tampering* is detected, not only accidents, which is what a verified system image wants (CP-26, GR-017).
  Deduplication for free. ZFS ships cryptographic per-block checksums (SHA-256, SHA-512, Skein, Edon-R, BLAKE3) as
  per-dataset options in production, so the approach is deployable on a read-write filesystem (`fs/survey.md#zfs`).
- **Evidence against:** A cryptographic hash per block costs far more CPU than a CRC on the read-heavy path
  (driver 5) `[inference]`. Without an authenticated root it gives no more against an attacker than a CRC (an
  attacker recomputes the tree). Mutable files under content addressing need a separate mutable layer (fossil on
  Venti, `fs/survey.md#plan-9-cwfs-fossil-and-venti`).
- **Cost:** *(estimate)* A cryptographic hash implementation in-house (or an external crate, a recorded decision)
  plus dedup/index structures for a full Venti-like design.
- **Precedent:** Venti and fossil (Plan 9), Git object stores `[outside evidence: Git's content-addressed objects,
  not in the record]`.

### Option D: No filesystem checksums; rely on the device (FAT, littlefs data, APFS data)

- **Description:** Trust the device's own error correction.
- **Evidence for:** Zero cost; littlefs leaves ECC to the device (`fs/survey.md#littlefs`); Apple pointed to device ECC
  for APFS data (GR-014, weak source).
- **Evidence against:** The drive study shows corruption the device did not report (GR-014). A virtio disk is a host
  file whose bytes the guest cannot vouch for (ADR-0015).
- **Cost:** *(estimate)* None.
- **Precedent:** FAT, littlefs, APFS data.

## Recommendation

**Recommendation (not a decision): Option B, with CRC32C, fail-stop on metadata mismatch, and a crafted-image
discipline that does not rely on checksums.**

1. **What is checksummed.** Every block the filesystem writes: data, metadata and root records. Each block pointer
   stores the block's checksum; each metadata block's header stores its kind, the volume identifier, its own block
   address and the generation of the commit that wrote it; each root record checksums itself (ADR-0011 rule 3).
2. **Algorithm.** CRC32C (Castagnoli), written in-house, using the CPU's CRC32C instructions when the CPU reports them
   and a table otherwise. The root record carries an algorithm identifier, and every pointer's checksum field is 64
   bits wide with CRC32C in the low half, so that a stronger algorithm can be introduced by a later ADR without a new
   pointer layout `[assumption: field width chosen for headroom, not measured]`.
3. **On a mismatch.**
   - *Data block:* that read fails with `Corrupt`; nothing else changes; the volume object records the event (a
     counter readable through its handle) so a supervisor can notice. No wrong data is ever returned.
   - *Metadata block:* the request fails with `Corrupt` and the volume enters `Failed` (ADR-0010 rule 6): no further
     writes, `StorageFailed` for every later mutating request and `sync`, reported to the supervisor. Root records
     are handled by mount (ADR-0011 rule 4).
   - *No automatic repair* in the first release: there is one copy of each block. Repair is `fsck`'s job, offline.
4. **Crafted images.** Every volume not created by this system's own `mkfs` on this machine is treated as hostile,
   and the code paths do not distinguish trusted from untrusted volumes:
   - **Isolation.** One filesystem service process per mounted volume, holding only that volume's block-device handle
     and the client channels for that volume (CP-15, CP-17). A compromised service can reach that volume's data and
     its clients' requests, nothing else.
   - **A total decoder.** The on-disk format crate has `#![forbid(unsafe_code)]`; every decoding function returns a
     `Result` for every input; it never panics, never indexes out of range, and never loops without a bound. Memory:
     mounting and traversing hold at most one block per tree level plus a fixed cache, so the bound is
     `MAX_DEPTH × BLOCK_SIZE + CACHE_CAP` bytes of decoder state, with the constants defined in the format crate
     `[assumption: values chosen there]`; no allocation size is taken from an on-disk field without clamping to that
     bound.
   - **Structural validation after the checksum**, because an attacker recomputes checksums: every block address is
     inside the device; every child's generation is not greater than its parent's; tree depth is at most a fixed
     maximum; keys are sorted and within the parent's key range; sizes and counts are within the block; names satisfy
     ADR-0013. A violation is `Corrupt`, handled as in rule 3.
   - **Shared decoder.** `fsck`, `mkfs` and the service use the same format crate.
5. **Not in this ADR.** Authenticity of a volume (signed root hash, Option C) belongs to the verified system image
   (CP-26) and needs its own ADR. Memory corruption is not addressed (GR-014's caveat).

**Why B.** Drivers 1 and 2 rule out A and D; driver 5 makes C's cryptographic hash too costly for the general
read-write filesystem when its benefit (tamper evidence) needs an authenticated root that this ADR does not provide.
CRC32C is the algorithm the two most-deployed Linux filesystems chose for blocks of this size (`fs/survey.md#xfs`,
`fs/survey.md#ext4`), and B puts it where it also catches lost writes.

**Principles this recommendation strains.** None directly. GR-014's caveat that memory corruption is not covered
remains.

## Consequences

- **Positive.** Corruption, lost and misdirected writes are detected for all blocks; no wrong data reaches clients; a
  crafted image is a contained, testable problem (a total decoder plus process isolation), not a kernel exploit.
- **Negative.** A CRC per read; with one copy of each block, a single bad metadata block makes the volume read-only
  until offline repair. CRC32C is not cryptographic: it detects accidents only.
- **Follow-up work.** CRC32C implementation and test vectors; the format crate's validation layer; the image fuzzer;
  per-volume service instantiation in the stage-4 service design; a later ADR on authenticated (verified) volumes.

## Invariants

- **INV-0012-1.** No read reply ever carries data whose checksum did not verify: corrupting any single bit of any block of an image makes every read that depends on that block return `Corrupt`, never different bytes. *Check:* a test that, for each block of a set of `mkfs`-built images with known content, flips bits and reads every file, asserting that each read returns either the original content or `Corrupt`.
- **INV-0012-2.** A block written at the right address but by an older commit (a lost write) or at the wrong address (a misdirected write) is detected on read. *Check:* tests that replace a block with its previous version, and that swap two blocks, then read through the tree.
- **INV-0012-3.** The on-disk format decoder never panics, never reads outside its input, and terminates on every input. *Check:* the format crate is `#![forbid(unsafe_code)]` (compile-time) and a fuzz test (field-level mutation of every field of every structure, XFS-style, plus random mutation, as a deterministic `cargo test` with a fixed seed and iteration count, like `zoraya-dtb`'s 30,000 mutations) asserts no panic and a bounded step count.
- **INV-0012-4.** Mounting and fully traversing any image, valid or not, terminates and never holds more than `MAX_DEPTH × BLOCK_SIZE + CACHE_CAP` bytes of decoder state (rule 4). *Check:* the same fuzz test with an allocation-counting allocator asserting the peak against the constant, and a step counter asserting a bound linear in the number of blocks.
- **INV-0012-5.** After any metadata `Corrupt` result, the volume is in `Failed`: it performs no further writes and answers every later mutating request and `sync` with `StorageFailed`. *Check:* a test device that records writes, with corruption injected into metadata blocks at random points of a request sequence, asserting the write log and the replies (shared with INV-0010-3).

## Open questions

- Metadata duplication (two copies on the same device, as btrfs `DUP`) would turn a single bad metadata block from
  "read-only volume" into "repaired on read". Cost: double metadata writes. Unmeasured.
- Is the 64-bit checksum field worth its space for 4 KiB blocks? Decide with the format crate.
- How does a user mount a foreign image read-write after an `fsck`? Policy for the stage-4/5 shell, not this ADR.
- Whether removable media in other formats (FAT) will ever be read: if so, that is a separate service with the same
  isolation and decoder discipline (FM-9 in `fs/survey.md#failure-modes-from-the-task-01-catalogue`).

## Revisit triggers

- A requirement to verify the system image or downloaded volumes cryptographically (CP-26): add authenticated roots
  (Option C) for those volumes.
- Measured read-path cost of CRC32C is significant on the prototype (unlikely with hardware instructions, unmeasured).
- Corruption is observed in practice on a supported device: reconsider redundancy (metadata duplication, mirrors).

## Architect's decision
