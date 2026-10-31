# ADR-0014: File metadata

- **Status:** accepted
- **Date:** 2026-10-25
- **Depends on:** ADR-0002, ADR-0010, ADR-0013
- **Evidence:** CP-01, CP-03, CP-20, CP-27, DM-003, DM-006, DM-009, DM-010, DM-016, DM-042, DM-063, GR-004, alternatives-postmortem.md#plan-9--inferno, fs/requirements.md#11-libc-functions, fs/requirements.md#4-metadata-read-and-written, fs/requirements.md#41-make-and-modification-times, fs/requirements.md#5-questions-the-data-cannot-answer, fs/survey.md#fat, fs/survey.md#nilfs2, fs/survey.md#plan-9-cwfs-fossil-and-venti, task-02/callsite-analysis.md#feature-5-identity-and-permissions, task-02/libc-surface.csv

## Context

This ADR decides which attributes the filesystem stores natively, the representation, resolution and range of
timestamps, how POSIX mode bits and `umask` are represented for the personality, and whether extended attributes and
sparse files exist. ADR-0002 rule 6 already decided that the native ABI has no uid/gid, that authority is what a
handle grants, and that "file modes are stored and reported by the filesystem service as POSIX metadata; the
personality does not enforce them against the identity, the handle's rights do". ADR-0005 notes that toybox's
mode/owner applets bring "advisory POSIX metadata into the filesystem service".

**What is measured** (`fs/requirements.md#4-metadata-read-and-written`):

- Metadata is read constantly: in W4, GNU make 1,892 `newfstatat` calls, dash 1,798, toybox 3,345. The data records
  calls, not fields (`fs/requirements.md#5-questions-the-data-cannot-answer`).
- **Modification times are load-bearing.** GNU make rebuilds a target that "is older than any of the prerequisites",
  deletes a target on interrupt only "if the target file's last-modification time has changed", and compares file
  times with `CLOCK_REALTIME`; one-second resolution is too coarse for builds whose dependent steps follow each other
  within a second (`fs/requirements.md#41-make-and-modification-times`).
- **Times are set explicitly** by toybox `touch` and `cpio` (`utimensat`, 9 calls in T0–T2)
  (`fs/requirements.md#4-metadata-read-and-written`).
- **Modes:** `umask` 4,336 calls; `fchmodat` 4 (toybox `chmod`); the execute bit is load-bearing because freshly built
  programs are executed and `$PATH` candidates are probed with `X_OK`.
- **Owners are never set** (`fchown`, `fchownat`, `lchown`, `chown` never called); **extended attributes are never
  used** (imported by toybox, never called); **no access-time use** was measured; `ftruncate` appears only in T3
  (`fs/requirements.md#11-libc-functions`; `task-02/libc-surface.csv`).
- Inode numbers and link counts: whether any program reads them is unknown
  (`fs/requirements.md#5-questions-the-data-cannot-answer`).

**Forces from TASK-01.** atime turns reads into writes, worse on copy-on-write (DM-042); 32-bit `time_t` and the 2038
problem (DM-063; CP-20: time is 64-bit); uid/gid access control is the ambient-authority model the native ABI rejects
(DM-009, DM-010, CP-01, CP-03); process-global `umask` is DM-006; Windows ACLs are complex (DM-016); FAT stores local
time and needs a time-zone option to convert (`fs/survey.md#fat`); PID reuse shows what reusable integer names cost
(DM-003).

## Decision drivers

1. **make's correctness depends on modification times that are on the wall-clock time base, change on every write,
   and order dependent writes strictly** (`fs/requirements.md#41-make-and-modification-times`, R-5).
2. **No identity in the native ABI** (ADR-0002 rule 6, CP-01, CP-03); modes are advisory metadata for the personality.
3. **Reads must not cause writes** (DM-042), especially under copy-on-write (ADR-0011).
4. **Time is 64-bit and wide enough never to wrap in practice** (DM-063, CP-20), and archives carry arbitrary times
   that `touch`/`cpio` set explicitly (`fs/requirements.md#4-metadata-read-and-written`).
5. **Store only what a measured program writes, or what POSIX `stat` (in ADR-0005's surface) must report and cannot
   be synthesised** (CP-27 applied to metadata). Which fields programs *read* is not measured
   (`fs/requirements.md#5-questions-the-data-cannot-answer`), so reads cannot justify or exclude a field.
6. **Stable object identity** for `st_ino`-style comparisons, without reuse races (DM-003's lesson applied to files).

## Considered options

### Option A: The POSIX inode (Linux baseline)

- **Description:** uid, gid, 12 mode bits, atime/mtime/ctime with nanoseconds, link count, inode number (reusable),
  size, blocks, extended attributes, ACLs.
- **Evidence for:** `stat` maps one to one; toybox `ls -l`, `cp -p`, `install`, `chmod`, `id` work as on Linux
  (`task-02/callsite-analysis.md#feature-5-identity-and-permissions`).
- **Evidence against:** uid/gid contradict ADR-0002 rule 6 and CP-01/CP-03; atime is DM-042; reusable inode numbers
  repeat DM-003's shape for files; xattrs and ACLs have no measured user (driver 5).
- **Cost:** *(estimate)* Most fields to store; an identity model the native side does not have.
- **Precedent:** every Unix filesystem.

### Option B: Plan 9 directory entries (non-Unix precedent)

- **Description:** Owner, group and last modifier stored as user *names* (strings), permission bits, access and
  modification times, a `qid` (path, version, type) identifying the file on its server.
- **Evidence for:** Names instead of numeric ids avoid id-mapping problems across machines; the `qid` version changes
  on every modification, which gives a cheap change detector [inference: Plan 9's `stat(5)` is not in the fetched
  sources]. A per-process-namespace system with no superuser (GR-004, `alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Still an identity model (string owners) that the native ABI rejects (driver 2); access time
  remains (DM-042).
- **Cost:** *(estimate)* Similar to A.
- **Precedent:** Plan 9 file servers (fossil, cwfs; `fs/survey.md#plan-9-cwfs-fossil-and-venti`).

### Option C: NT security descriptors and rich attributes (Windows)

- **Description:** ACLs as security descriptors, creation/modification/access/change times, attribute flags,
  alternate data streams.
- **Evidence for:** Fine-grained access control per object.
- **Evidence against:** ACL complexity is DM-016; the native ABI's authority is the handle, not an ACL (CP-01);
  last-access time is deferred and optional on NTFS for DM-042's reasons.
- **Cost:** *(estimate)* Largest.
- **Precedent:** NTFS.

### Option D: Minimal native attributes plus an advisory POSIX mode word

- **Description:** Natively: object identifier (64-bit, never reused within a volume), kind, size, modification
  time, change time; plus one 16-bit `posix_mode` word stored verbatim for the personality and
  enforced by nobody. No owner, no access time, no extended attributes.
- **Evidence for:** Every stored field is written by a measured program or needed to fill `struct stat` (driver 5); no identity (driver 2); no atime (driver 3); an
  identifier that is never reused removes stale-identity races [inference from DM-003]; the mode word gives `ls -l`,
  `chmod`, `test -x` and `$PATH` probes what they need (R-6).
- **Evidence against:** Tools that preserve owners (`cp -p`, `tar`) lose them; programs that rely on access times
  (mail readers, cleanup tools, DM-042) get modification times instead; xattr-dependent software cannot run.
- **Cost:** *(estimate)* Smallest record; `libposix` synthesises the rest of `struct stat`.
- **Precedent:** NILFS2 stores no access time (`fs/survey.md#nilfs2`); Linux `relatime`/`lazytime` and NTFS defer
  it (DM-042). No surveyed system drops owners entirely; that part follows from ADR-0002 rule 6.

## Recommendation

**Recommendation (not a decision): Option D**, specified as follows.

1. **Attributes stored per object.**

   | Attribute | Type | Set by |
   |---|---|---|
   | `id` | u64, unique per volume, never reused | service at creation |
   | `kind` | `File`, `Directory`, `Link` (ADR-0013 rule 7) | creation |
   | `size` | u64, at most 2^63 − 1 (fits `off_t`) | writes, set-size |
   | `mtime` | timestamp | service on every change of content (file data, directory entries, link target); client explicitly |
   | `ctime` | timestamp | service on every change of the object, including attributes and rename; never by clients |
   | `posix_mode` | u16: 9 permission bits plus set-user-id, set-group-id and sticky bits, stored verbatim | creation request; set-attributes |

2. **Timestamps.** Signed 64-bit seconds since 1970-01-01T00:00:00 UTC plus a 32-bit nanosecond field (0 to
   999,999,999), the shape of POSIX `struct timespec` and Linux `statx`. Range: beyond any date a client can name
   (driver 4); no 2038 or 2262 limit.
3. **Time base and strict order.** The service takes times from the system wall clock (UTC), so that `make` can
   compare them with `CLOCK_REALTIME` (driver 1). Every time the service assigns on one volume is strictly greater
   than every time it assigned before on that volume: `assigned = max(wall_clock_now, last_assigned + 1 ns)`. So a
   modification that completes after another (ADR-0010 completion order) has a strictly later `mtime`, whatever the
   clock's resolution and even if the wall clock steps backwards. If the clock steps back, assigned times run ahead of
   the wall clock until it catches up; make may then warn about files "in the future" `[inference]`. Times set
   explicitly by clients (`utimensat`) are stored as given and do not move `last_assigned`. `last_assigned` is
   persisted in every root record (ADR-0011 rule 3) and restored at mount, so the order also holds across remounts
   and crashes: after a crash the restored value is at least every time stored in the selected tree.
4. **No access time.** Not stored. `libposix` reports `st_atime = st_mtime`. Reads never write (driver 3).
5. **No owners.** `libposix` reports the session's fixed uid/gid for every file (ADR-0002 rule 6), which is also what
   `geteuid`/`getegid` return, so readers of the owner field behave as for a user's own files: `ls -l` prints the
   session's names, `test -O`/`-G` are true. If a superseding ADR adds `chown`/`fchown`/`lchown` to ADR-0005's surface
   (today only `fchown`, `fchownat` and `lchown` are tier rows, and none was ever called; `chown` is an
   `unused-applets` row of `task-02/libc-surface.csv`), changing to that same uid/gid succeeds and changes nothing, and
   any other value fails with `EPERM`.
6. **Modes are advisory.** The service stores `posix_mode` and enforces nothing; a handle's rights are the only
   authority (ADR-0002 rule 6). A create request carries the mode; a native client that passes none gets `0o644` for
   files and `0o755` for directories `[assumption]`. `umask` is `libposix` state, carried to children by
   `libposix` through its spawn protocol `[assumption]`, and applied before the request is sent (DM-006: the process-global part stays inside the
   personality). For consistency with POSIX programs, `libposix` applies the *owner* permission bits as POSIX would
   for a file's owner who is not root: `access(R_OK|W_OK|X_OK)`, `open` for reading or writing and `execve` fail with
   `EACCES` when the corresponding owner bit is clear (`test -r/-w/-x`, `rm`'s prompt for write-protected files,
   configure probes). This is consistent with ADR-0002 rule 6, because every file has the caller's identity as owner
   (rule 5), so no identity is compared: it is behaviour emulation, not a security check. A native client is not
   affected, and the set-id bits grant nothing (CP-03).
7. **Object identity.** `libposix` reports `st_ino = id` and `st_dev` = a per-volume identifier; since `id` is never
   reused, `(st_dev, st_ino)` identifies one object for the life of the volume.
8. **No extended attributes, no ACLs.** `libposix` returns `ENOTSUP` for the xattr calls (none was ever called).
9. **Sparse files.** A file's unwritten ranges read as zeros and need not occupy blocks; this is a representation
   freedom of ADR-0011's tree, not a client-visible feature. No `SEEK_HOLE`/`SEEK_DATA` in the first release.
   `st_blocks` reports allocated blocks.
10. **Link counts.** `libposix` reports `st_nlink = 1` for files and link objects; for directories see ADR-0013's
    open question.
11. **No FIFO or device nodes.** `kind` has no FIFO: FIFO nodes live in the per-session temporary directory service
    (ADR-0002 rule 1, ADR-0006), not in this filesystem (ADR-0010 open question 5).

**Why D.** The *writes* are measured: owners are never set, extended attributes never called, access times never
set, while modification times and modes are set and relied on (`fs/requirements.md#4-metadata-read-and-written`).
Which fields are *read* is not measured; the known readers of owners (`ls -l`, `test -O/-G`) are satisfied by the
fixed per-session identity (rule 5). `ctime` has no measured writer but is part of `struct stat` and cannot be
synthesised (it changes on renames and mode changes, not only on writes); a creation time has neither, so it is not
stored. The decisive drivers are 2 (no identity) and 3 (no atime). Rule 3 turns make's implicit assumption into a
guarantee the filesystem can test, which neither A nor B gives.

**Principles this recommendation strains.** CP-27's spirit is kept; the cost DM-042 records ("dropping atime removes
a cheap usage signal") is accepted.

## Consequences

- **Positive.** `make` never misses a rebuild because two dependent writes got the same time or the clock stepped
  back. No identity model leaks into storage. Reading never writes. Object identity is stable.
- **Negative.** Owners, access times and xattrs from foreign archives are lost. Assigned times can run ahead of the
  wall clock after a clock step. Modes look like security to POSIX users but are not.
- **Follow-up work.** The attribute record in the on-disk format crate; the `stat` synthesis and `umask` in
  `libposix`; the strictly increasing clock in the service.

## Invariants

- **INV-0014-1.** Times assigned by the service on one volume are strictly increasing in completion order, also when the wall clock stands still or steps backwards, and across remounts and crash-remounts. *Check:* a service test with a scripted test clock (constant, coarse-grained, stepping backwards, and behind the newest stored time after a remount) and random request sequences interleaved with clean remounts and crash-remounts from the crash harness, asserting that the `mtime`/`ctime` values assigned are strictly increasing in completion order across all of them.
- **INV-0014-2.** Every request that changes a file's content or a directory's entries sets that object's `mtime` (and `ctime`) to a newly assigned time, and no read or lookup changes any attribute. *Check:* a model-based test comparing attributes after each request with the reference model, including a check that read-only request sequences leave the image byte-identical.
- **INV-0014-3.** Object identifiers are never reused within a volume, across deletes, commits and remounts. *Check:* a property test that creates and deletes objects across many commits and crash-remounts, recording every identifier ever returned.
- **INV-0014-4.** Timestamps round-trip exactly: any value with seconds in the full i64 range and nanoseconds in 0..1e9 set explicitly is read back unchanged, and values outside the nanosecond range are rejected. *Check:* unit and property tests on the attribute encoder and through the service.
- **INV-0014-5.** The attribute record has exactly the fields of rule 1 and no owner, group, access-time or creation-time field. *Check:* a test in the on-disk format crate that lists the attribute record's field names (from a `const` field table the encoder is generated from) and compares them with rule 1's list, so adding a field requires changing this ADR.

## Open questions

- Should `libposix` persist set-id and sticky bits at all, given they grant nothing (CP-03)? Stored verbatim here for
  archive round-trips.
- Is reporting `st_atime = st_mtime` enough for the measured programs? None was shown to read access times.
- Whether any program in scope relies on `st_ino`/`st_dev` or link counts (`fs/requirements.md#5-questions-the-data-cannot-answer`):
  a cheap call-site search in a future measurement task.
- Clock source for the filesystem service before a time service exists (stage 4): the kernel's wall clock
  (device-tree RTC) is assumed.

## Revisit triggers

- A program in scope needs owners preserved (a package manager, a backup tool), access times, or extended
  attributes: add the attribute by superseding ADR, with its measured user.
- A program in scope needs `SEEK_HOLE`/`SEEK_DATA` or large sparse files.
- The strictly increasing rule produces user-visible "future" times often enough to matter (clock steps on the
  platform).

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. Reasons and open conditions: [filesystem-batch-selection.md](filesystem-batch-selection.md).
