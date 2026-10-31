# ADR-0016: Filesystem implementation choices the accepted ADRs leave open

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0014, ADR-0015
- **Evidence:** CP-12, DM-037, GR-014, alternatives-postmortem.md#redox, fs/survey.md#btrfs, fs/survey.md#littlefs, fs/survey.md#redoxfs, fs/survey.md#testing-crash-consistency

## Context

TASK-07 implemented ADR-0010 to ADR-0015 as the `zoraya-fs` library. Writing the code met points where the accepted
ADRs are silent, leave a value as an assumption for the implementation, or ask for something the implementation
cannot do literally. TASK-07's rules say: write a short proposed ADR and implement the least committing option
behind a clearly marked boundary. This ADR lists those points. Each is implemented as described in the
Recommendation, and each place in the code that depends on it cites "proposed ADR-0016, item N".

What is measured: nothing here is measured on a workload. The test results that bear on each item are in
`docs/tasks/TASK-07-review.md` and in the tests named under Invariants.

The items:

1. **Object ids across a crash.** ADR-0014 rule 7 and INV-0014-3 say ids are never reused, also across crash
   remounts. An id handed out by a request whose commit a crash then discards was seen by a client; if the next
   mount restarted from the last durable counter it would hand the same id out again. ADR-0011 rule 2 lists when a
   group is committed and does not mention ids.
2. **A valid newest root record whose tree root is invalid.** ADR-0011 rule 4 says mount selects the highest
   generation with a valid copy and never a lower one; INV-0011-5 says "a valid copy whose tree root passes
   validation". Both copies of a generation point to the same tree root, so when that root is invalid there is no
   copy that passes, and rule 4 forbids the older generation.
3. **Deleting on a full volume.** ADR-0010 rule 6 reserves space when a request is accepted. In a copy-on-write tree
   a removal also needs new blocks (the changed path to the root), so a volume filled to the last block could refuse
   to remove anything. In a copy-on-write tree even metadata updates consume space (btrfs: reading consumes space
   after a snapshot unshares metadata, `fs/survey.md#btrfs`), and copy-on-write filesystems keep a reserve for this
   `[outside evidence: btrfs's "global reserve" and ZFS's "slop space" are documented by those projects, not in the
   fetched sources]`.
4. **Allocator layout and volume size.** ADR-0011 rule 3 names "the allocator root" without a layout.
5. **Directory size.** ADR-0014 rule 1's `size` is set "by writes and set-size"; directories have neither.
6. **CRC32C instructions.** ADR-0012 rule 2 asks for the CPU's CRC32C instructions when the CPU reports them.
   ADR-0012 rule 4 asks for `#![forbid(unsafe_code)]` in the format crate. The instructions are reached through
   `unsafe` intrinsics in Rust, so the two rules cannot both hold in one crate.
7. **Orphans at mount.** ADR-0011 rule 7 frees the orphan list at mount; INV-0011-6 says a commit with no dirty state
   writes nothing. Freeing orphans is dirty state, so a read-write mount of a volume with orphans writes.
8. **Reporting `Failed`.** ADR-0010 rule 6 leaves the mechanism to the channel ADRs.
9. **Commit triggers beyond ADR-0011 rule 2.** Besides `T_COMMIT`, dirty state and `sync`, the implementation
   commits before handing out an id at its durable limit (item 1) and when a request is refused for space while the
   open group holds space it freed (so that the space becomes reusable, ADR-0011 rule 5).
10. **Request semantics the ADRs do not fix.** Rename of an entry onto itself; publish of an object whose last name
    was removed while open; a write of zero bytes; open-with-create of an existing name without truncate.

## Decision drivers

1. Follow the accepted ADRs exactly where they speak; where they do not, choose what commits least and can be changed
   without a format change where possible.
2. Never weaken a guarantee of ADR-0010 to make an item easier: errors stay typed and never get lost (CP-12, DM-037).
3. Keep the decoder total and the format crate free of `unsafe` (ADR-0012 rule 4, GR-014's lesson that checksums
   only help if every path verifies them).
4. Every choice must be testable by the harness that already exists (`fs/survey.md#testing-crash-consistency`).

## Considered options

### Option A: Do what Linux filesystems do (baseline)

- **Description:** Inode numbers come from the allocator and are reused after deletion (ext4, XFS), with a
  generation counter for NFS; a full copy-on-write volume refuses even deletions until space is reclaimed by other
  means; checksumming uses whatever the kernel's crypto layer picks at run time.
- **Evidence for:** Simplest; no extra commit. Kernel code may use `unsafe` freely, so hardware CRC is free.
- **Evidence against:** Reuse breaks INV-0014-3 outright. A volume that cannot delete when full is the btrfs
  experience the reserve exists to avoid (`fs/survey.md#btrfs`).
- **Cost:** *(estimate)* Lowest.
- **Precedent:** ext4, XFS, btrfs.

### Option B: Least committing choices inside the accepted design (this implementation)

- **Description:** The Recommendation's list: durable id batches, mount refusal instead of fallback, a fixed delete
  reserve, a one-level bitmap allocator, directory size = entry count, table CRC32C with the hardware path deferred,
  an immediate commit after freeing orphans at mount, a polled failure report, and the request semantics of item 10.
- **Evidence for:** Each item keeps every ADR-0010 guarantee and is covered by the TASK-07 tests. The on-disk format
  carries what a later change needs (the id limit and the allocator root are root record fields; the checksum
  algorithm field exists).
- **Evidence against:** Item 1 adds a commit after mount on the first create; item 6 leaves CRC32C slower than it
  could be; item 4 caps a volume at about 20.9 GiB.
- **Cost:** Built, measured with `wc -l`: about 5,100 lines in `zoraya-fs` including the checker, about 4,200
  without comments and blank lines.
- **Precedent:** RedoxFS's ring of headers selected by highest generation (`fs/survey.md#redoxfs`,
  `alternatives-postmortem.md#redox`), littlefs's rule that a torn commit is ignored and the previous state used
  (`fs/survey.md#littlefs`).

### Option C: Larger mechanisms (non-Unix precedent: littlefs and RedoxFS style)

- **Description:** Make every item a feature: a free-space B-tree instead of bitmaps; ids derived from a per-mount
  epoch written at every mount; a hardware CRC32C path in a separate crate allowed to use `unsafe`; metadata
  duplication so that an invalid tree root falls back to its copy (ADR-0012's open question).
- **Evidence for:** Removes the 20.9 GiB cap; tolerates a single bad tree root block; full CRC speed.
- **Evidence against:** Each is new format or new crate surface with no measured need; a mount-time epoch write
  contradicts INV-0011-6 for every read-write mount; metadata duplication is an open question of ADR-0012, not a
  decision.
- **Cost:** *(estimate)* 1,500–3,000 more lines and two format features.
- **Precedent:** littlefs metadata pairs (`fs/survey.md#littlefs`), RedoxFS (`fs/survey.md#redoxfs`).

## Recommendation

### Decision

Accepted 2026-10-31. Until the on-disk format is declared stable by a later ADR, on-disk compatibility between format versions is not guaranteed; volumes are rebuilt with zoraya-mkfs. Raising the volume limit is deferred to a format version 2 when a real need appears. Hardware CRC32C may be added later in a separate crate with the table implementation as fallback; it does not change the format.

The original recommendation follows, accepted with the 20.9 GiB limit and software CRC32C.

**Recommendation (not a decision): Option B**, as implemented:

1. **Ids in durable batches.** The root record holds `id_limit`: every id below it may have been handed out. Mount
   starts handing out at `id_limit`. Before a request would hand out an id at or above the limit, the engine raises
   the limit by `ID_BATCH` (1,024) and commits, so the new limit is durable before any id under it is used. A crash
   can lose ids, never repeat one.
2. **Mount refuses an invalid newest tree.** If the newest generation with a valid root record copy has a tree root
   that fails validation, mount fails with `Corrupt`. It does not fall back (ADR-0011 rule 4). Repair is offline and
   needs a second copy that this format does not have.
3. **Delete reserve.** `DELETE_RESERVE` (64 blocks) is kept free for requests that only free space (remove,
   remove directory, rename, set size, closing the last handle of a nameless object, freeing orphans). Other
   requests that would eat into it fail with `NoSpace` and change nothing.
4. **Allocator.** One bit per block in bitmap blocks of 32,448 bits, pointed to by an allocator root that holds up to
   169 pointers: at most 5,483,712 blocks (about 20.9 GiB at 4 KiB). Bitmap blocks are copy-on-write like every
   other block; a commit rewrites only the bitmap blocks whose bits changed, and the allocator root. A larger device
   is used up to that size.
5. **Directory size** is its number of entries; `remove directory` requires 0.
6. **CRC32C** is table-driven in `zoraya-fs`. A hardware path, if a measurement shows the table matters, goes in a
   separate small crate that is allowed `unsafe` and passes a checksum function in; the results are identical.
7. **Orphans at mount.** A read-write mount that finds objects on the orphan list frees them and commits at once.
   A volume with an empty orphan list mounts without writing (INV-0011-6).
8. **Failure report.** `Volume::take_failure_report` returns the failure reason exactly once after the volume
   enters `Failed`; the service polls it after each request and forwards it to its supervisor when the channel ADRs
   say how.
9. **Extra commit triggers** as listed in item 9 of the Context. Committing earlier than ADR-0011 rule 2 requires
   never weakens ADR-0010: a commit group is still a consecutive part of the completion order.
10. **Request semantics.** Renaming an entry onto itself succeeds and changes nothing (no time assigned). Publish
    takes any file on the orphan list, including one whose last name was removed while open; a file that has a
    name gets `InvalidArgument`. A zero-byte write succeeds and changes nothing. Open-with-create of an existing name
    without truncate changes nothing.

## Consequences

- **Positive.** Every ADR-0010 guarantee holds with these choices, as the TASK-07 tests check. No format feature was
  added beyond the two root record fields the ADRs already imply (`id_limit`, the allocator root).
- **Negative.** The first create after each mount costs one commit (two flushes). Volumes are capped at about
  20.9 GiB. CRC32C runs at table speed. A volume whose newest tree root block is bad does not mount.
- **Follow-up work.** If accepted: nothing further. If the architect changes an item, the code that cites it changes
  with a test.

## Invariants

- **INV-0016-1.** No object id is handed out twice within a volume, including ids of objects whose creation a crash discarded. *Check:* `times_and_ids_across_crash_remounts` and `lost_ids_are_not_reused` in `crates/zoraya-fs/tests/space_time.rs`.
- **INV-0016-2.** Mount never selects a generation below the highest one with a valid root record copy; if that generation's tree root fails validation, mount fails with `Corrupt`. *Check:* `invalid_tree_root_of_the_newest_generation_fails_the_mount` in `crates/zoraya-fs/tests/integrity.rs`.
- **INV-0016-3.** On a volume refusing requests with `NoSpace`, every name can still be removed. *Check:* `full_volume_refuses_cleanly` in `crates/zoraya-fs/tests/space_time.rs`, which removes every name after filling the volume.
- **INV-0016-4.** A directory's size equals its number of entries. *Check:* the checker reports any difference, and runs on every image of the model, crash and fault tests.

## Open questions

- Is `ID_BATCH` = 1,024 right? It trades one commit per 1,024 creates (and one per mount) against nothing else;
  measure with the stage-4 service.
- Does a toolchain workload ever fill a volume to the delete reserve? Unmeasured.
- Is the 20.9 GiB cap acceptable for stage 4's system disk (a few GiB, ADR-0011's assumption)? Yes for now.

## Revisit triggers

- A measurement shows CRC32C cost matters on the read path: build the hardware crate of item 6.
- A volume larger than 20.9 GiB is needed: a second allocator level or a free-space tree.
- ADR-0012's metadata duplication question is decided for duplication: item 2 then falls back to the copy.

## Architect's decision

### Decision

Accepted 2026-10-31. Until the on-disk format is declared stable by a later ADR, on-disk compatibility between format versions is not guaranteed; volumes are rebuilt with zoraya-mkfs. Raising the volume limit is deferred to a format version 2 when a real need appears. Hardware CRC32C may be added later in a separate crate with the table implementation as fallback; it does not change the format.
