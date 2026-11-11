# ADR-0265: Storage: a limit on a directory tree in `fsd`, and an untrusted room's own `/tmp`

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0016, ADR-0051, ADR-0055, ADR-0056, ADR-0107, ADR-0172, ADR-0262, ADR-0263, ADR-0266, ADR-0267
- **Evidence:** CP-01, CP-14, quotas/survey.md#storage

## Context
An untrusted room writes in the directories its line grants (`+w ~/sandbox/agent-1`, ADR-0172) and in `/tmp` if a
POSIX program in it is given one. Today nothing bounds either but the volume: one `fsd` serves the whole volume
(ADR-0012's one service per volume), so a room that fills its directory fills the person's home, and every room's
`/tmp` is the session's `tmpd`, an in-memory `fsd` of 16 MiB `[assumption]` (ADR-0107 point 12) shared by every
program of the shell's session. Zoraya has no users, so owner quotas have no owner to charge (CP-01), and no hard
links (ADR-0013), so a tree quota is exact (`quotas/survey.md#storage`). The engine keeps parent links of directories
in its B+tree (`docs/design.md`, "Filesystem engine"), so a directory's ancestors can be found.

## Decision drivers
1. A disk filler in a room must fail its own writes and leave the rest of the volume writable (CP-14).
2. The limit must be on what the person names: the room's writable directories and its `/tmp`.
3. Deleting must always work, also at the limit and on a full volume (ADR-0016's delete reserve exists for this).
4. The limit must survive a reboot, as the room's line does (ADR-0172 point 1).
5. Copy-on-write must not make a rewrite count twice (ADR-0011).

## Considered options

### Option A: Owner quotas (Linux and BSD user and group quotas)
- **Description:** Blocks are charged to the file's owner; `EDQUOT` past the owner's limit (`quotas/survey.md#storage`).
- **Evidence for:** The classic mechanism, in `tmpfs` too.
- **Evidence against:** Zoraya has no owners (CP-01); a room is not a user.
- **Cost:** *(estimate)* An owner field and a table; meaningless here.
- **Precedent:** Every Unix.

### Option B: A limit on a directory tree, kept by the filesystem (XFS project quotas, ZFS dataset quotas, Windows folder quotas)
- **Description:** A directory carries a limit; everything below it counts; every limited ancestor applies; past the
  hard limit `EDQUOT` (`quotas/survey.md#storage`).
- **Evidence for:** Drivers 1, 2, 4. XFS has to refuse hard links across projects to keep it exact; Zoraya has no hard
  links. ZFS applies every ancestor's quota. Persistent with the volume.
- **Evidence against:** A format change and a counter to keep on every allocation; moving a directory across a
  limit's boundary needs its size; XFS enforces a little early because it reserves the most an allocation may need.
- **Cost:** *(estimate)* 400 lines in `zoraya-fs` and the service; a format version.
- **Precedent:** XFS project quotas; ZFS `quota` and `refquota`; Windows' File Server Resource Manager, whose quotas
  limit a folder and can apply to every existing and future subfolder (`quotas/survey.md#storage`).

### Option C: A volume per room (a file-backed volume, as ADR-0056 named for sandboxes)
- **Description:** Each untrusted room gets its own volume in a file of the main volume, served by its own `fsd`;
  the file's size is the limit.
- **Evidence for:** Exact and total isolation, even of metadata bugs; no change to the engine's format; the room's
  `fsd` runs in the room's job and its memory is the room's.
- **Evidence against:** A loop driver (`fsd` serving a file of another `fsd` as a block device) and two copy-on-write
  layers; the room's files are not in the person's home as files, so `+w ~/sandbox/agent-1` would no longer be a
  directory the person can open from `files`; a fixed size, set at creation.
- **Cost:** *(estimate)* A loop block driver (300 lines) and per-room volume management.
- **Precedent:** disk images for containers and VMs; ZFS volumes have an implicit quota of their size.

### Option D: A limit on the client's account, not persistent
- **Description:** `fsd` counts blocks written through an account (ADR-0262) and refuses past a budget.
- **Evidence for:** No format change; uses the accounts that exist anyway.
- **Evidence against:** Driver 4: the count is lost at reboot or when the room's connections close, and a file
  written by one account and grown by another is charged to neither in full.
- **Cost:** *(estimate)* 150 lines.
- **Precedent:** none found.

## Recommendation
**Recommendation (not a decision): Option B for writable directories, and an untrusted room's own `tmpd` for `/tmp`.**

1. **The limit.** A directory may carry a *limit record* `{ limit, used }` in bytes, stored in the volume's tree next
   to the directory's own record. `used` counts, for every object below the directory at any depth: one 4 KiB block
   for the object itself, and its data's logical size rounded up to 4 KiB. Logical, not physical: a rewrite in place
   that copy-on-write puts on new blocks does not count twice (driver 5); snapshots and old roots are not counted.
2. **Charging.** A request that grows `used` (creating a file, directory or link object, a write past the end, a larger
   `set_size`) walks the directory's ancestors to the volume root, finds the limited ones, and checks each `used + n <=
   limit`; if any fails, the request fails with `OverQuota` and is rolled back, as a request that fails for space is
   today. Requests that shrink (`unlink`, removing a directory, a smaller `set_size`) are never refused for a limit, and use
   the delete reserve if the volume is full, as ADR-0016 lists them (driver 3). A rewrite in place does not change
   `used` and so is never refused for a limit; on a full volume it fails with `NoSpace` as today and does not touch
   the reserve. The counters are part of the copy-on-write tree, so a crash
   leaves them consistent with the files (ADR-0010's prefix rule).
3. **Moving.** A rename of a file across a limit's boundary moves its charge, and is refused with `OverQuota` if the
   destination's limited ancestors lack room. A rename of a directory across a boundary is refused with
   `CrossesLimit`, mapped by `libposix` to `EXDEV`, so `mv` copies and deletes, as it does across volumes; inside one
   limited tree a directory moves freely. (XFS refuses links across projects for the same reason,
   `quotas/survey.md#storage`.)
4. **Who sets it.** `set_limit(name, bytes)` on a connection to a directory sets the limit of the child directory
   `name`, and needs `WRITE` on the connection. A limit is therefore set from above: a room's programs hold
   `~/sandbox/agent-1` itself and cannot change its limit, though they may limit subdirectories of their own, which
   only narrows. `set_limit(name, none)` removes it. A limit below `used` is allowed; growth is then refused until
   `used` falls.
5. **Rooms.** `room new NAME --disk SIZE` sets `SIZE` on each of the room's outermost `+w` directories (one below
   another gets none of its own) through the creating shell's connection to its parent (a directory under `~` is
   reached through `+w ~`), keeping a smaller limit already there. An untrusted room must have one (ADR-0266's
   default), and its `+w` grants must name directories: `room new agent-1 --untrusted +w ~/notes.txt` is refused with
   `room: ~/notes.txt: an untrusted room writes only in directories` and `hint: +w ~/sandbox/agent-1`, since a file
   cannot carry a limit and would grow without one. `room set NAME --disk SIZE` changes them; `room drop` leaves the
   limit in place.
6. **`/tmp` of an untrusted room.** Every shell has a broker beside it, which starts its `tmpd` (ADR-0107 item 12,
   `fsd --memory SIZE`); in an untrusted room both run in the room's job, with `SIZE` from `--tmp` (default 16 MiB,
   ADR-0107's). The `tmpd` commits its memory volume at start (ADR-0051), so its whole size is in the room's `--mem`
   from the first moment, once per shell of the room: two terminals in the room are two `/tmp`s of `--tmp` each, as
   they are two POSIX sessions today. Its `fsd` is the room's, not the session's: a room's `/tmp` is neither shared
   with nor visible to another room. A trusted room keeps today's arrangement.
7. **Statuses.** `OverQuota` (ADR-0263) and `CrossesLimit` join `zoraya-proto-file`'s statuses; `libposix` maps them
   to `EDQUOT` and `EXDEV`. A full volume stays `NoSpace` (`ENOSPC`).
8. **Reading it.** `stat` of a directory reports its limit record when it has one; `top` and `room` show `disk USED
   of LIMIT` per room (ADR-0267).
9. **`sync` from an untrusted room.** ADR-0055 records that one client calling `sync` in a loop stalls every other
   client of a volume, since the engine serves one request at a time, and that limits do not fix it. So a `sync`
   on a connection of an untrusted account does not commit at once: it waits for the next commit the volume makes
   anyway (the timed commit every 5 s, ADR-0016, or another client's `sync`) and returns when that commit is durable.
   The guarantee of ADR-0010 holds (what was written before the `sync` is durable when it returns); only its latency
   changes, and a `sync` loop in a room costs the volume nothing beyond its timed commits.

**Tensions with accepted ADRs.** *ADR-0011 and ADR-0016:* a new record in the tree, a format version (volumes are
rebuilt with `zoraya-mkfs` until a format is declared stable, `docs/design.md`); the delete reserve now also serves
deletes at a limit. *ADR-0055:* two requests and two statuses; its "cheap answer is a volume per sandbox" (Option C here) is not taken,
and its `sync` stall is answered by point 9. *ADR-0010:* `sync` from an untrusted account is durable on return but
waits for the next commit. *ADR-0012:* one service per volume stays; limits are inside it. *ADR-0107 item 12:* an
untrusted room's brokers and their `tmpd`s run in the room's job. *ADR-0172:* `--disk` and `--tmp` in the room line (ADR-0266).

**Examples.**

```
zosh ~> room new agent-1 --untrusted --disk 2G +w ~/sandbox/agent-1
zosh ~> stat ~/sandbox/agent-1
~/sandbox/agent-1: directory, limit 2G, used 1.2G
agent-1 ~/sandbox/agent-1> cp big.iso copy.iso
cp: copy.iso: over quota (2G of 2G disk)
hint: ~/sandbox/agent-1 is at its disk limit (2G of 2G)
to allow more: room set agent-1 --disk 4G
agent-1 ~/sandbox/agent-1> rm copy.iso          # always allowed
```

In `files`, a limited directory's info line reads `2G limit, 1.2G used` beside its size.

### Effect of the vision
An agent's writable directory is a bounded tree the person can still open as files, and its `/tmp` is its own,
inside its memory. For the general-purpose base, limits are optional on any directory, as project quotas are, and
cost nothing where none is set beyond the walk to the root.

## Consequences
- **Positive.** Exact limits, persistent, on the trees a person names; deletes always work; `/tmp` stops being a shared
  channel between rooms.
- **Negative.** A format change; every growing request walks to the root; a directory cannot be moved across a limit
  without a copy.
- **Follow-up.** Q5 in `quotas/plan.md`; a disk-filler test.

## Invariants
- **INV-0265-1.** For every limited directory, `used` equals the sum over its subtree of one block per object plus
  each data's logical size in blocks, after every request and after a crash and remount. *Check:* host model test in
  `zoraya-fs` with random requests, crashes at every write (the engine's crash harness) and recomputation.
- **INV-0265-2.** A request that would pass any limited ancestor's limit fails with `OverQuota` and changes nothing;
  a shrinking request never fails for a limit. *Check:* host tests in `zoraya-fs`, including a full volume.
- **INV-0265-3.** A disk filler in an untrusted room's directory and in its `/tmp` stops at its limits, and another
  room and the person's home can still create and write files. *Check:* in-QEMU test (`tests/posix/`), with `dd` in
  the room and a write outside it.
- **INV-0265-4.** An untrusted room's `/tmp` is served by a `tmpd` in the room's job, and no other room's `/tmp`
  names it. *Check:* host test of the shell's launch table for an untrusted room; in-QEMU test that a file created in
  one room's `/tmp` is not found from another's.
- **INV-0265-5.** A `sync` loop in an untrusted room delays a `home` program's `sync` by at most one timed commit.
  *Check:* in-QEMU test: a room running `while :; do sync; done` while a `home` program writes and syncs 100 times,
  each within 5 s plus one commit's time.

## Open questions
- Whether one block per object is the right charge for metadata: measure the engine's real blocks per object.
  *Note (Q5, 2026-11-11):* the engine charges one block per object, a file's size in blocks and, for a link, its target's block
  too; the real metadata is a few dozen bytes per object in a leaf, so the charge over-counts and is safe. Not measured
  against a real directory tree. Details the ADR left open are in proposed ADR-0290.
- Whether a soft limit with a grace period (XFS) is wanted for trusted directories.

## Revisit triggers
- Hard links or shared extents (reflinks) are added: the tree is no longer a tree.
- A second volume type that a room may write.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
