# ADR-0011: Filesystem crash-consistency mechanism

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** ADR-0010, ADR-0012, ADR-0015
- **Evidence:** X-14, DM-038, GR-014, GR-017, alternatives-postmortem.md#sprite, fs/requirements.md#12-linux-syscalls, fs/requirements.md#13-open-flags-and-who-uses-them, fs/requirements.md#21-durability-is-never-requested, fs/requirements.md#22-atomic-rename, fs/requirements.md#6-requirements-derived-for-the-adrs, fs/survey.md#btrfs, fs/survey.md#comparison, fs/survey.md#ext4, fs/survey.md#f2fs, fs/survey.md#fxfs-and-minfs-fuchsia, fs/survey.md#littlefs, fs/survey.md#nilfs2, fs/survey.md#plan-9-cwfs-fossil-and-venti, fs/survey.md#redoxfs, fs/survey.md#testing-crash-consistency, fs/survey.md#wafl, fs/survey.md#xfs, fs/survey.md#zfs

## Context

ADR-0010 (proposed) requires that every request is atomic, that the post-crash state is a prefix of the completion
order, that `sync` makes everything completed before it durable, and that completed requests become durable within
`T_COMMIT` anyway. This ADR chooses the on-disk mechanism that delivers that, states its expected size, and says how
it would be tested. The device it runs on is the one ADR-0015 describes: writes may be reordered and torn until a
flush, and only a flush makes earlier completed writes stable.

**What is measured.** The workload is write-heavy and never calls `fsync`: W4 alone issues about 93,000 `write` and
13,000 `writev` calls and 1,110 `unlinkat` (`fs/requirements.md#12-linux-syscalls`). Files are created, written once
and either kept or deleted; in-place rewriting of existing files is limited to tcc truncating its outputs
(`fs/requirements.md#22-atomic-rename`). No database-like overwrite pattern was measured. File mappings do not occur
below T3 (`fs/requirements.md#13-open-flags-and-who-uses-them`).

**What is assumed.** Volume sizes of a few GiB on QEMU `virt` (stage 4 of the roadmap). One filesystem service
instance per volume (ADR-0012). No crash was injected in any measurement.

**Survey** (`fs/survey.md#comparison`): journaling (ext4, XFS) is the Linux baseline; copy-on-write trees (ZFS,
btrfs, bcachefs, RedoxFS, WAFL) and log-structured designs (F2FS, NILFS2, Sprite's LFS, littlefs's metadata logs,
Fxfs's LSM trees) are the alternatives; fossil used soft updates. The small consistent-by-construction designs are a
few thousand lines (littlefs about 7,700, RedoxFS about 6,500); the large ones are large because of features on top.

## Decision drivers

1. **ADR-0010's prefix rule must hold for data as well as metadata**, including overwrites
   (`fs/requirements.md#6-requirements-derived-for-the-adrs`, R-1 and R-4; DM-038).
2. **The device gives no write atomicity and no ordering except flush** (ADR-0015; `fs/survey.md#testing-crash-consistency`,
   dm-log-writes models exactly this).
3. **Small and verifiable by tests**: the number of distinct crash states per commit decides how exhaustively crash
   testing can run (`fs/survey.md#testing-crash-consistency`; CrashMonkey's bounded enumeration).
4. **Checksums are wanted on all blocks** (ADR-0012, GR-014); the mechanism should give them a natural home.
5. **Throughput without `fsync`** matters more than `sync` latency for the measured workload (driver 1 of ADR-0010,
   X-14).
6. **No escape hatch that drops guarantees** for some files (GR-014: `nodatacow` drops checksums).

## Considered options

### Option A: Write-ahead journal with in-place home locations (ext4, XFS; Linux baseline)

- **Description:** Metadata changes (and optionally data) are first written to a journal; a commit record with a
  checksum makes a transaction valid; blocks are then written to their home location; recovery replays committed
  transactions.
- **Evidence for:** The most deployed design; ext4 and XFS are mature (`fs/survey.md#ext4`, `fs/survey.md#xfs`). In-place
  updates keep files contiguous, so no fragmentation from overwrites (contrast FM-11, GR-014). Metadata-only journaling
  writes metadata twice and data once.
- **Evidence against:** Prefix semantics for *data* overwrites requires journaling data too (`data=journal`), which
  writes all data twice and is being removed from ext4 (`fs/survey.md#ext4`); the default ordered mode tears
  in-place overwrites. Checksums were retrofitted to metadata and are absent for data (`fs/survey.md#comparison`).
  Recovery code (replay, idempotency, intents for long operations) is a large separate path (`fs/survey.md#xfs`).
  Crash states include partially written home locations, so the space to test is larger (driver 3).
- **Cost:** *(estimate)* A minimal metadata journal plus allocator and trees: 10,000–15,000 lines; data journaling
  doubles write volume. ext4 plus JBD2 are about 74,000 lines in Linux (`fs/survey.md#ext4`).
- **Precedent:** ext3/ext4, XFS, NTFS. Works; DM-038 is its crash-consistency record.

### Option B: Copy-on-write tree with atomic root switch (ZFS, btrfs, RedoxFS, WAFL)

- **Description:** No live block is ever overwritten. A batch of requests (a *commit group*) writes new copies of
  every changed block, and of every tree node up to the root, to free space; then a small root record (the
  superblock) is written to one of several fixed slots with a generation number and checksum. Mount picks the valid
  slot with the highest generation. Space freed by a commit becomes reusable only after that commit is durable.
- **Evidence for:** Prefix semantics are natural: commit groups are cut in completion order and each becomes visible
  atomically when its root record is stable (driver 1). The only in-place write is the root record, protected by a
  checksum and multiple slots; RedoxFS uses a ring of 256 header slots and picks the highest generation
  (`fs/survey.md#redoxfs`). A crash yields exactly "old root" or "new root" per commit once flush ordering is right,
  so crash testing can be exhaustive (driver 3). The parent pointer is the natural place for the child's checksum,
  which is why CoW designs had checksums from the start (`fs/survey.md#comparison`, GR-014, driver 4). ZFS's
  checksummed CoW held up under fault injection (GR-014). Snapshots fall out of it (fossil's epochs,
  `fs/survey.md#plan-9-cwfs-fossil-and-venti`).
- **Evidence against:** Every commit rewrites the path from changed leaves to the root (write amplification),
  mitigated only by batching. Overwrite-heavy files fragment (GR-014, FM-11). Space accounting must reserve
  copy-on-write space at request time (ADR-0010 rule 6). btrfs's RAID56 is still "unstable" after years
  (`fs/survey.md#btrfs`), though that is RAID on top of CoW, not CoW itself. `sync` latency is one full commit.
- **Cost:** *(estimate)* Core engine (B-tree, allocator, commit, recovery) 6,000–10,000 lines of Rust, judging by
  RedoxFS (about 6,500) and littlefs (about 7,700 C) (`fs/survey.md#redoxfs`, `fs/survey.md#littlefs`); plus format
  definitions, `mkfs` and `fsck` (see `implementation-outline.md`).
- **Precedent:** ZFS and WAFL (decades in production, WAFL from search summary only: `fs/survey.md#wafl`), btrfs,
  bcachefs, RedoxFS, APFS (GR-014).

### Option C: Log-structured filesystem (Sprite LFS, F2FS, NILFS2; non-Unix: littlefs metadata logs, Fuchsia Fxfs)

- **Description:** All writes append to a log divided into segments; periodic checkpoints record a consistent
  index; recovery rolls forward from the last checkpoint; a cleaner compacts segments to reclaim space.
- **Evidence for:** Sequential writes; ordered recovery gives prefix semantics; continuous snapshots for free
  (NILFS2, `fs/survey.md#nilfs2`). Fxfs (Fuchsia's replacement for Minfs) uses LSM trees and a logical journal
  (`fs/survey.md#fxfs-and-minfs-fuchsia`, search summary only). LFS ideas survived Sprite and reached NetApp and flash
  controllers (`alternatives-postmortem.md#sprite`).
- **Evidence against:** The cleaner is a second subsystem with its own failure modes and latency spikes; F2FS's
  design centres on cleaning overhead and the "wandering tree" (`fs/survey.md#f2fs`); NILFS2 needs a user-space
  cleaner daemon and still has no `fsck` (`fs/survey.md#nilfs2`). The crash state space includes cleaner
  interactions (driver 3). Its main advantage, sequential writes, matters for flash and rotating disks, not for a
  virtio disk backed by a host file (ADR-0015).
- **Cost:** *(estimate)* Comparable to B plus a cleaner: 10,000–15,000 lines. F2FS is about 49,500 and NILFS2 about
  23,600 lines in Linux (`fs/survey.md#comparison`).
- **Precedent:** F2FS (Android), NILFS2 (niche), Fxfs (Fuchsia, search summary only), littlefs (embedded).

### Option D: Copy-on-write tree plus an intent log for fast `sync` (ZFS's synchronous path, WAFL's NVRAM log)

- **Description:** Option B, plus a small sequential log where a `sync` or `durable` request records its intent and
  returns after one flush, without waiting for a full tree commit; the log is replayed onto the tree at mount.
- **Evidence for:** Keeps B's crash model for the tree while making `sync` latency one log write plus flush; ZFS
  offers synchronous semantics this way (`fs/survey.md#zfs`), WAFL with NVRAM (`fs/survey.md#wafl`, search summary
  only).
- **Evidence against:** Two recovery paths (tree and log replay) and their interaction must be tested; the measured
  workload never calls `sync` (`fs/requirements.md#21-durability-is-never-requested`), so the benefit has no
  measured user.
- **Cost:** *(estimate)* B plus 2,000–4,000 lines and a second crash-test dimension.
- **Precedent:** ZFS, WAFL.

### Option E: Soft updates (ordered in-place writes without a journal; Plan 9 fossil, BSD FFS)

- **Description:** In-place updates whose write order is tracked so that the disk is always consistent apart from
  leaked space.
- **Evidence for:** No journal and no copy of the tree; fossil uses it so that "there is no need to check a file
  system that was shut down without halting" (`fs/survey.md#plan-9-cwfs-fossil-and-venti`).
- **Evidence against:** Consistency, not prefix semantics: the order of *user-visible* operations is not preserved
  across files, and in-place data overwrites tear (driver 1). Dependency tracking is per block and per operation type,
  the hardest kind of code to test exhaustively (driver 3). No checksum commit point (driver 4).
- **Cost:** *(estimate)* fossil is about 20,500 lines (`fs/survey.md#plan-9-cwfs-fossil-and-venti`).
- **Precedent:** fossil (its paper says it had "not yet replaced" Plan 9's main file server, `fs/survey.md#plan-9-cwfs-fossil-and-venti`), BSD FFS.

## Recommendation

**Recommendation (not a decision): Option B**, with these rules:

1. **Never overwrite a live block.** Every block reachable from any root record that mount could choose is
   immutable. Data is copy-on-write too; there is no per-file switch to in-place writes (driver 6).
2. **Commit groups in completion order.** The service cuts the completion order of ADR-0010 into consecutive commit
   groups. A group is committed when `T_COMMIT` expires, when dirty state exceeds a threshold, or when a `sync` or
   `durable` request is waiting.
3. **Commit protocol** (with ADR-0015's device): (a) write every new block of the group; (b) wait for all of them to
   complete; (c) flush; (d) write the root record, with generation `g+1`, the tree root pointer and its checksum, the
   allocator root, and its own checksum, into slot `(g+1) mod N_ROOT`; (e) wait, flush; (f) only now reply to
   `sync`/`durable` requests of the group and treat the group as durable. `N_ROOT` ≥ 2
   `[assumption: 4; RedoxFS uses 256]`.
4. **Mount** reads all root slots, validates each (checksum, generation, bounds, ADR-0012), and picks the valid one
   with the highest generation. No replay, no `fsck` is needed after a crash.
5. **Deferred reuse.** A block that commit `g` stops referencing may be written again only after commit `g` is
   durable (step 3f). This keeps the previous root's tree intact for as long as mount could still choose it.
6. **Reservation.** A request is accepted only if the space its share of the next commit needs (new blocks, plus
   copied tree nodes, bounded by tree height) can be reserved; reservations are released at commit (ADR-0010 rule 6).
7. **Unnamed objects** (ADR-0010 rule 7) live in a per-volume orphan list in the tree; mount frees every entry of the
   list (a crash before `publish` leaves nothing).
8. **Structure.** One copy-on-write B+tree per volume (or a small fixed set of them) keyed by object and record kind;
   the precise key layout is an implementation choice in the on-disk format crate, not part of this decision.
9. **Deferred:** an intent log (Option D) is not built until a measured workload needs low `sync` latency (revisit
   trigger).

**How it would be tested** (details in `implementation-outline.md`): a recording in-memory block device that applies
ADR-0015's crash model; for every commit, every crash point (each prefix of completed writes, each subset allowed
between flushes, torn root records) is remounted and compared with the ADR-0010 reference model (INV-0010-1); an
offline checker that verifies the invariants below on every remounted image; and the image fuzzer of ADR-0012.

**Why B.** It is the only option where the prefix rule for data overwrites costs nothing extra (A needs data
journaling, E cannot give it), where the crash states per commit are two (driver 3), and where checksums have a
natural place (driver 4); C would add a cleaner whose benefit (sequential writes) the virtual device does not need.
D is B plus a feature with no measured user.

**Principles this recommendation strains.** None of the candidate principles directly; GR-014's caveat (overwrite-
heavy files fragment) is accepted with no escape hatch, which costs performance for future database-like workloads.

## Consequences

- **Positive.** Crash recovery is "choose the newest valid root": no replay code, no mandatory `fsck`. Snapshots and
  cheap rollback become possible later (GR-017 notes A/B updates want this). Every block has a parent that carries its
  checksum (ADR-0012).
- **Negative.** Write amplification per commit; fragmentation of overwritten files; free space must be larger than
  the largest commit; `sync` costs a full commit (two flushes).
- **Follow-up work.** On-disk format crate; allocator with deferred reuse; space reservation; the crash-test harness;
  `mkfs` and `fsck`; a measurement of commit cost and write amplification on W1/W4 on the prototype.

## Invariants

- **INV-0011-1.** No write issued while commit `g` is in progress targets a block reachable from the root record of any generation that mount could select (the durable root and every root written after it). *Check:* the recording test device tracks reachable sets per root record and fails the test on any write to a protected block.
- **INV-0011-2.** A block freed by commit `g` is not written before the flush that makes commit `g`'s root record stable has completed. *Check:* the same recording device, checking each write against the free-generation of its block.
- **INV-0011-3.** The root record of commit `g` is written only after every block of commit `g` has completed and a flush issued after those completions has completed; `sync` replies of commit `g` are sent only after a flush issued after the root record's completion has completed. *Check:* the recording device asserts the write/complete/flush order per commit.
- **INV-0011-4.** Every image the crash harness produces mounts without repair, and the offline checker finds every reachable block's checksum valid, no block reachable twice (except shared by design, none in the first release), and the allocator's free set equal to the complement of the reachable set (minus blocks pending deferred reuse). *Check:* run `fsck` (offline checker) on every remounted image in the crash harness.
- **INV-0011-5.** A torn or corrupt root record is never selected: mount picks the highest generation whose record and whose tree root pass validation. *Check:* tests that tear each root slot at every 512-byte boundary and corrupt each field.

## Open questions

- Commit-group sizing (time, dirty bytes) and its interaction with `T_COMMIT`: needs prototype measurement.
- Write amplification on W1/W4 (tree height times commits): unmeasured; may motivate larger leaf nodes or batching.
- Whether metadata blocks should be written twice (btrfs `DUP`-style) so that one bad sector does not make a volume
  read-only (ADR-0012 question).
- Whether snapshots should be exposed to clients (GR-017 A/B updates) is out of scope here.

## Revisit triggers

- A workload in scope calls `sync` often and commit latency is measured to dominate: build Option D's intent log.
- Fragmentation of overwritten files measurably slows a workload in scope (GR-014's caveat): consider in-place data
  updates *with* data journaling for such files, never without checksums.
- The volume size grows so that commit cost (tree height, allocator) dominates: reconsider the tree layout.

## Architect's decision
