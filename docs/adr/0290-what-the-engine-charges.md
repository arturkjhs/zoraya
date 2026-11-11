# ADR-0290: What the filesystem engine charges to a limit: files' parents, unnamed and unlinked files, and the wire

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0010, ADR-0011, ADR-0013, ADR-0016, ADR-0055, ADR-0263, ADR-0265
- **Evidence:** CP-01, CP-14, quotas/survey.md#storage

## Context
ADR-0265 fixes the limit record, the charge (one block per object plus the data's logical size in blocks), the walk
over limited ancestors, rename across a boundary and the statuses. Building it in Q5 (TASK-26) met questions it leaves
open. (1) The engine keeps a parent link for directories only, so a growing write to a file could not find the
limited directories above it. (2) An unnamed file (ADR-0010 rule 7) has no directory until `publish`, and may grow
before that. (3) A file unlinked while a handle is open stays on the orphan list and can still grow; the directories
that held it may be removed meanwhile. (4) The file protocol's `Code` was `Below<16>`, which cannot carry a status
above 15. (5) Who may set a limit on the wire, and how a test sets one before a room keeper exists.

## Decision drivers
1. A room must not escape its limit by a path the charge does not see (an unnamed file, an unlinked one; CP-14).
2. `used` must equal the recomputed sum after every request and every crash (INV-0265-1), so every rule needs a
   recomputation `fsck` can do from the tree alone.
3. A request that frees space is never refused (ADR-0265 rule 2).
4. The simplest format change that works; volumes are rebuilt until a format is declared stable. There are no owners to charge (CP-01), so the tree is the only record (`quotas/survey.md#storage`).

## Considered options

### Option A: a parent link for every object, charge held until the object is freed
- **Description:** Format version 2 adds `K_FPARENT` (the directory a file or link is charged to) beside `K_PARENT`
  of directories. A new object is charged to the directory it is made in; an unnamed file to the directory it was
  made in, and `publish` moves the charge like a rename; an object unlinked while open keeps its charge and its link
  until it is freed (the last close or the next mount). A directory with such an orphan charged to it, and a limit on
  or above it, cannot be removed (`NotEmpty`: the limit above would otherwise lose the file; XFS keeps the charge
  with the project for the same reason); with no limit on or above it, removal always works and the orphans are charged
  to its parent. A directory costs one block and gives it back when it is unnamed. A link costs
  one block and its target in blocks. `Code` becomes `Below<18>`; `OverQuota` and `CrossesLimit` are statuses 16
  and 17. `set_limit(name, bytes | none)` needs `WRITE` on the connection; `get_limit` is open to any connection.
  `fsck` recomputes every `used` and `repair` rewrites it.
- **Evidence for:** Drivers 1 to 4: no write escapes a limit, the recomputation walks `K_PARENT` and `K_FPARENT` and
  needs no other state.
- **Evidence against:** One more record per file (about 28 bytes in a leaf); a file unlinked while open keeps its
  charge until closed (as Linux keeps its blocks).
- **Cost:** *(estimate)* 300 lines in the engine and checker; measured in Q5's report.
- **Precedent:** XFS project quotas keep the project id in the inode and charge an unlinked-but-open file until it is
  freed; ZFS dataset quotas count a deleted but open file until its last reference goes.

### Option B: find a file's directories by search
- **Description:** No new record; a growing write searches the tree for the file's directory entry.
- **Evidence for:** No format change.
- **Evidence against:** The directory entries are keyed by `(directory, name)`, so the search is a scan of the whole
  tree per growing request.
- **Cost:** *(estimate)* Unbounded per request.
- **Precedent:** none.

### Option C: charge only named files, let unnamed and unlinked files grow without a limit
- **Description:** An unnamed file or one unlinked while open is charged to no one.
- **Evidence for:** No orphan rules.
- **Evidence against:** Driver 1: `open`, `unlink`, `write` forever fills the volume from inside a limited room.
- **Cost:** *(estimate)* Fewest lines; a hole.
- **Precedent:** none.

## Recommendation
**Recommendation (not a decision): Option A.** It is what Q5 built; `make test-unit` runs its model test
(`crates/zoraya-fs/tests/limits.rs`), and `fsck` checks the rules.

## Consequences
- **Positive.** No path writes past a limit; `fsck` is the oracle; the format change is one version.
- **Negative.** A new limit on a directory scans its subtree in one request (the engine serves one request at a time).
  The scan is bounded at 50,000 objects (`SCAN_BOUND`, an assumption): a larger tree is refused with `ScanTooLarge`
  (`LIMIT_REACHED` on the wire, `ENOMEM` in `libposix`), and a sum past `u64::MAX` with `OverQuota`. Within the bound a
  client with `WRITE` may still repeat the scan; ADR-0292 considers a `LIMIT` right.
- **Follow-up.** Q6a's keeper uses `set_limit`; until then `zoraya-mkfs --limit PATH=SIZE` and the POSIX images' `/limited`
  are the test-only way.

## Invariants
- **INV-0290-1.** Every file and link has a charged directory that is a directory, equal to the one that names it;
  an orphan's is a directory. *Check:* `fsck` (`zoraya_fs::check`), run on every crash image of
  `crates/zoraya-fs/tests/limits.rs` by `make test-unit`.

## Open questions
- Whether one block per object is the right metadata charge (ADR-0265's own question): the engine's real blocks per
  object are measured in Q5's report.
- Whether the scan bound of 50,000 objects is right, or the scan should be a background step.
- Whether `FsError::OverQuota` on `open` with `O_CREAT` should leave the empty file it created. It does not: the
  request fails as a whole.

## Revisit triggers
- Hard links or reflinks (ADR-0265's own trigger): a file would have two parents.

## Architect's decision

(Left empty for the architect.)
