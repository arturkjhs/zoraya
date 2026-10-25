# Filesystem survey

Part 2 of [TASK-06](../../tasks/TASK-06-filesystem-design.md). Evidence for the proposed ADRs 0010–0015, not a
decision.

## Evidence rules and their limits in this task

- **Primary sources first; every cited URL was fetched in this session.** The source list is at the end
  ([Sources](#sources)); each entry gives the exact URL fetched and the revision where one exists.
- **Network limit.** The cloud environment of this task could reach GitHub (raw files and `git clone`) and a web
  search engine, but not `kernel.org`, `gnu.org`, `usenix.org`, `fuchsia.dev`, `lwn.net` or most other hosts
  (connections were refused by the environment's egress policy). Primary sources were therefore taken from their
  GitHub-hosted copies: the Linux kernel's `Documentation/` tree, the btrfs-progs documentation, the OpenZFS
  manual pages, the OASIS virtio specification sources, QEMU, the Linux man-pages, GNU make's documentation mirror,
  the Plan 9 Foundation tree, littlefs and RedoxFS.
- **What could not be fetched** is marked `[search summary, not fetched]`: the content comes from a search engine's
  summary of the page, and the claim is weaker than the rest. This applies to Fuchsia's Fxfs and Minfs, WAFL, and one
  LWN article. These are listed separately in [Sources](#sources) (`U-` entries) so they are not mistaken for checked
  citations.
- **The TASK-01 catalogue is reused, not re-researched.** Claims already verified there are cited by their IDs
  (`DM-`, `GR-`; e.g. DM-037 for fsync error semantics, GR-014 for checksummed copy-on-write). Their sources are in
  `docs/research/sources.md`.
- **Implementation sizes were counted here**, by cloning each project at a stated revision and counting the lines of
  its `.c`/`.h` (or `.rs`) files, excluding files whose name contains `test` (source F-35). They are lines including
  comments and blank lines, a rough measure of size, not of complexity.
- Everything else is paraphrase. "Inference" marks a conclusion the sources do not state.

## Failure modes from the TASK-01 catalogue

Each design below is checked against these. The IDs `FM-n` are used only in this file.

| ID | Failure mode | Catalogue entry |
|---|---|---|
| FM-1 | Crash in the middle of an update leaves the namespace or a file inconsistent, or exposes data that was never written (zero-length files after rename) | DM-038 |
| FM-2 | A write-back error is reported once, or never, and the data is dropped; a retried `fsync` falsely succeeds | DM-037 |
| FM-3 | The device returns corrupted data silently (bit rot, misdirected or lost writes) | GR-014 |
| FM-4 | Path resolution races and symlink attacks | DM-039 |
| FM-5 | Hard links create names for objects the creator has no authority over | DM-040 |
| FM-6 | Reads cause writes (atime) | DM-042 |
| FM-7 | Name equality depends on case folding or Unicode tables; names are arbitrary bytes | DM-094, DM-095 |
| FM-8 | Fixed path limits and magic names | DM-043 |
| FM-9 | A crafted image attacks the code that parses it (new here; the device-trust form is GR-021's "drivers trust the device") | GR-021 |
| FM-10 | No way to make several changes atomic together | DM-045 |
| FM-11 | Overwrite-heavy files fragment under copy-on-write, and the escape hatch drops checksums | GR-014 |

## Designs

### ext4

- **Crash consistency.** A write-ahead journal (JBD2). Three modes (F-02): `data=journal` commits all data into the
  journal before writing it in place (deprecated; "support will be removed in January 2028"); `data=ordered`, the
  default, forces data blocks out before the metadata that refers to them is committed; `data=writeback` keeps no
  ordering between data and metadata. The transaction commit interval defaults to 5 seconds. A transaction without a
  commit record, or with a bad checksum, is discarded at replay (F-03). "Fast commits" log a minimal metadata delta
  between full commits (F-03).
- **Integrity.** Metadata checksums (`metadata_csum`) since 2012 on "all major ext4 and jbd2 data structures",
  CRC32c; some structures only have room for the low 16 bits (F-04). Journal checksums (v2/v3: each journal metadata
  block and each logged data block) (F-03). The documentation describes no checksum on file data.
- **Metadata structure.** Inodes in fixed tables, extent trees, hashed directory trees (htree) (F-04 lists the
  checksummed structures).
- **Failure modes.**
  - FM-1: the ext4 documentation itself describes the problem: "Many broken applications don't use fsync() when
    replacing existing files via patterns such as `open("foo.new")/write/close/rename("foo.new","foo")`, or worse
    yet, `open("foo", O_TRUNC)/write/close`". The `auto_da_alloc` option detects both patterns and forces the new
    data out before the rename commits, "roughly the same level of guarantees as ext3", to avoid "the zero-length
    problem" (F-02). This heuristic is the kernel's answer to DM-038. `data=writeback` "can potentially leave stale
    data exposed in recently written files in case of an unclean shutdown, which could be a security exposure"
    (F-02).
  - FM-2: ext4 marks pages clean after a write-back failure (DM-037, Rebello et al.).
  - FM-3: metadata only. A corrupted data block is returned to the application.
  - Error policy on detected corruption: `errors=continue|remount-ro|panic` (F-02).
- **Size and maturity.** About 64,700 lines for `fs/ext4` plus 9,500 for `fs/jbd2` (Linux v6.17, F-35).
- **What went wrong.** The 2009 delayed-allocation zero-length-file episode (DM-038); mode-dependent persistence
  properties found by Pillai et al. (DM-038); fsync error semantics (DM-037); casefolding initially could not be
  combined with encryption (DM-094).

### XFS

- **Crash consistency.** "XFS uses Write Ahead Logging for ensuring changes to the filesystem metadata are atomic and
  recoverable", combining "intents, logical and physical logging". Long-running operations are chained by intents so
  that "journal recovery can restart and finish an operation that was only partially done" (F-06). File data is not
  journaled: ext4's documentation describes its own writeback mode as "a similar level of journaling as that of XFS
  and JFS in its default mode - metadata journaling", where "a crash+recovery can cause incorrect data to appear in
  files which were written shortly before the crash" (F-02).
- **Integrity.** "Self-describing metadata" (F-05): every metadata block carries a CRC32c, the filesystem UUID, its
  own block number (a block number alone does not catch a write "misdirected to the wrong LUN"), an owner field (which
  inode or allocation group the block belongs to) and the log sequence number of its last modification. The authors
  chose CRC32c because metadata blocks are at most 64 KiB, and stated that a larger checksum "does [not] really provide
  any extra value over CRC32c, but it does add a lot of complexity" (the source omits "not", evidently a typo), so there is "no provision for changing the
  integrity checking mechanism". The motive was forensic: making corruption at petabyte scale diagnosable (F-05). No
  data checksums are described.
- **Metadata structure.** B-trees: free space per allocation group, extents per inode (F-05).
- **Failure modes.** FM-1 metadata only (data may be stale after a crash, as above). FM-2: PostgreSQL corruption was
  reported on XFS (DM-037). FM-3: metadata blocks are self-identifying, so misdirected and stale metadata writes are
  detectable; data corruption is not. FM-9: XFS maintains field-level fuzzing of every metadata field (see
  [Testing crash consistency](#testing-crash-consistency)).
- **Size and maturity.** About 222,000 lines in `fs/xfs` at Linux v6.17, which includes online scrub and repair
  (F-35).
- **What went wrong.** The documentation is candid about cost: an online fsck design needed because offline checking
  does not scale (F-07), and a maintainer's complaint about "fuzz kiddiez" who publish crafted-image bugs without
  fixing them (F-07), which shows crafted images are an ongoing burden for a filesystem not designed for them.

### btrfs

- **Crash consistency.** Copy-on-write B-trees; a new tree root is committed after the blocks under it (GR-014).
- **Integrity.** "Checksums on data and metadata (multiple algorithms available)", on-line "scrub", automatic repair
  from a correct copy when redundancy exists (F-08, F-09).
- **Metadata structure.** Copy-on-write trees; subvolumes and writable snapshots (F-08).
- **Failure modes.** FM-1 handled by CoW. FM-3 handled for data and metadata. FM-11: `nodatacow` turns off data
  checksums (GR-014). FM-2: btrfs also marks pages clean after a failed write-back (DM-037). FM-6: atime updates after a
  snapshot unshare metadata, so reading consumes space (DM-042).
- **Size and maturity.** About 155,000 lines in `fs/btrfs` (Linux v6.17, F-35). The project's own status page still
  lists RAID5/6 as "unstable: do not use for other than testing purposes, known severe problems" (F-09; GR-014).
- **What went wrong.** RAID56 (F-09). User-space transaction ioctls added for Ceph were deprecated (DM-045). Many
  crash-consistency bugs were found by automated testing: CrashMonkey and Ace found 8 previously unknown btrfs bugs
  (F-25).

### ZFS

- **Crash consistency.** Copy-on-write with the checksum of each block stored in its parent's block pointer, up to
  an uberblock (GR-014). Synchronous requests (`fsync`, `O_DSYNC`) follow the `sync` property: `standard` is "the
  POSIX-specified behavior of ensuring all synchronous requests are written to stable storage and all devices are
  flushed"; `always` commits every transaction before the call returns, with "a large performance penalty";
  `disabled` ignores synchronous requests and is "very dangerous" (F-10). With `logbias=latency` (the default), "ZFS
  will use pool log devices (if configured) to handle the write requests at low latency"; with `throughput` it will
  not (F-10). That synchronous writes go to a separate intent log (the ZIL) is the usual description of this
  mechanism `[outside evidence: not stated in the fetched sources beyond "pool log devices"]`.
- **Integrity.** Every block is checksummed; the default `checksum=on` currently selects Fletcher-4; SHA-256,
  SHA-512, Skein, Edon-R and BLAKE3 are available; turning checksums off is "NOT a recommended practice" (F-10).
  Self-healing from redundant copies and periodic scrubs (GR-014).
- **Names.** Per-dataset properties fixed at creation: `casesensitivity` (default sensitive), `normalization` (default
  none; when set, "file names are always stored unmodified, names are normalized as part of any comparison process"),
  and `utf8only` (reject names that are not valid UTF-8; default off) (F-10).
- **Failure modes.** FM-1 and FM-3 handled; FM-3 not for memory corruption (ZFS is "less resilient to memory
  corruption", GR-014). FM-11 is real for databases (GR-014).
- **Size and maturity.** About 283,000 lines in `module/zfs` plus `include/sys`, and about 466,000 for all of
  `module/` and `include/` including platform layers (OpenZFS master, F-35).
- **What went wrong.** Size and licensing kept it out of the Linux tree [inference; not stated in the fetched
  sources]. The integrity design itself held up (GR-014).

### F2FS

- **Crash consistency.** Log-structured (LFS) with "checkpointing": two copies of the checkpoint (CP) area, one of
  which "always indicates the last valid data" (the "shadow copy mechanism"), and the node-address table (NAT) and
  segment information table (SIT) shadowed the same way (F-11). A roll-forward recovery routine replays recent
  `fsync`ed data after a crash (it can be disabled with `disable_roll_forward`) (F-11).
- **Design problems it addresses.** The LFS "wandering tree" (an update to a data block propagates new locations up
  every index level) and cleaning overhead (F-11; the LFS definition quoted there is Rosenblum and Ousterhout's).
- **`fsync` policy.** `fsync_mode=posix` (default) "does a light operation"; `strict` "behaves in line with xfs, ext4
  and btrfs, where xfstest generic/342 will pass, but the performance will regress" (F-11). The default and the other
  Linux filesystems therefore differ in what a successful `fsync` guarantees.
- **Integrity.** The fetched documentation describes no data or metadata checksums.
- **Size and maturity.** About 49,500 lines (Linux v6.17, F-35). Widely deployed on Android flash [inference; not in
  the fetched sources].
- **What went wrong.** CrashMonkey and Ace found 2 F2FS crash-consistency bugs (F-25). The `fsync_mode` option itself
  records that the default deviates from the other filesystems (F-11).

### NILFS2

- **Crash consistency.** Log-structured; creates "checkpoints every few seconds or per synchronous write basis", so
  after a crash it recovers "like conventional LFS"; each checkpoint is a mountable read-only snapshot; a super root
  block per checkpoint roots the metadata files (F-12).
- **Integrity.** Not described in the fetched documentation.
- **Gaps.** No atime, extended attributes, POSIX ACLs, quotas, fsck or defragmentation; a user-space cleaner daemon is
  required (F-12).
- **Size and maturity.** About 23,600 lines (Linux v6.17, F-35). Niche.
- **Relevance.** Continuous snapshots are what a log gives for free; the cost is a mandatory cleaner (F-12).

### Fxfs and Minfs (Fuchsia)

All claims in this section are `[search summary, not fetched]` (U-01, U-02); they are recorded because Fuchsia is the
closest non-Unix precedent, and they should be checked against Fuchsia RFC-0136 before an ADR relies on them.

- **Minfs** was Fuchsia's mutable filesystem with a physical journal. RFC-0136 states it had limitations that made
  planned features and performance goals hard to reach.
- **Fxfs** was proposed by RFC-0136 as its replacement: a log-structured design whose metadata lives in log-structured
  merge (LSM) trees with immutable written layers, plus a *logical* journal. The journal is a file of
  variable-length records, written in blocks with a checksum at the end of each block seeded from the previous block;
  replay stops at the first checksum failure. Data written copy-on-write has checksums recorded in the journal and
  verified at replay for data written since the last device flush, so a torn copy-on-write write is detected and the
  file reverts to its last complete version.
- Fuchsia products migrated from Minfs to Fxfs in place at boot.
- **Size.** Not measured: the Fuchsia source host was not reachable.

### RedoxFS

- **Crash consistency.** Copy-on-write. The filesystem header is written to one of a ring of 256 header slots; at
  mount the header with the highest generation number wins (F-15: `header.rs` `HEADER_RING = 256`,
  `filesystem.rs`). A transaction therefore becomes visible when its header is written [inference from the code].
- **Integrity.** "Data/metadata checksums" (F-15 README); in the source each block pointer carries a 64-bit SeaHash of
  the block (`block.rs`, `BlockAddr` with a `hash` field computed by `seahash::hash`). SeaHash is a non-cryptographic
  hash.
- **Other.** Transparent encryption; "standard Unix file attributes"; MIT licence; inspired by ZFS (F-15).
- **Size and maturity.** About 6,500 lines of Rust in the core (excluding the FUSE/Redox mount layer, the command-line
  tools and the test file), 11,300 in `src/` in total (commit b87b097, F-35). The default filesystem of Redox; Redox
  itself is not yet self-hosting (`alternatives-postmortem.md`, Redox).
- **Relevance.** The closest peer: a Rust, copy-on-write, checksummed filesystem written for a microkernel, small
  enough to read in a day.

### littlefs

- **Problem it solves.** Power loss "at any time", flash wear, and bounded RAM on microcontrollers with about 32 KiB of
  RAM (F-14).
- **Crash consistency.** "Metadata pairs": two-block logs whose commits are protected by a 32-bit CRC; a commit counts
  only once its checksum is written, so a torn commit is ignored at mount and the previous state is used. Above the
  metadata pairs, a "copy-on-bounded-writes" tree. The authors' reasoning: logging gives atomicity but poor
  performance; copy-on-write performs but "pushes the atomicity problem upwards"; combining them bounds both (F-14).
- **The move problem.** Atomic rename *between directories* needed a dedicated mechanism ("global state": deltas
  XORed across metadata pairs) after an earlier mark-and-unmark scheme "worked, but not well" (F-14). Atomic
  cross-directory rename is a design problem of its own in a copy-on-write tree with per-directory commits.
- **Integrity.** CRCs on metadata commits; littlefs "does not provide ECC" and leaves error correction to the device
  (F-14).
- **Size and maturity.** About 7,700 lines of C (`lfs.c`, `lfs.h`, `lfs_util.*`; commit 6cb4e86, F-35). Widely used
  in embedded firmware [inference; not in the fetched sources].
- **Relevance.** Shows the smallest known design that is power-loss safe by construction, with a written design
  document that explains every choice.

### FAT

- **Crash consistency.** None: blocks are rewritten in place, so FAT is "not power-loss resilient" without
  modifications (F-14's taxonomy). Linux's driver offers a `flush` option to write back earlier, and
  `errors=panic|continue|remount-ro` (F-13).
- **Integrity.** None.
- **Names and metadata.** 8.3 short names with a separate long-name mechanism; Linux's driver is case-insensitive by
  default (`check=n`, "currently case insensitive"); timestamps are stored in local time, so the driver needs `tz=UTC`
  or `time_offset` to convert, and "time stamps in a different DST setting will be off by one hour" (F-13).
- **Size and maturity.** About 8,300 lines in Linux `fs/fat` (and 7,800 for exFAT) (v6.17, F-35). Universal for
  removable media.
- **What went wrong.** Reserved device names and 8.3 aliases (DM-043); the `git~1` alias was one vector of
  CVE-2014-9390 (DM-094). FAT is the format a removable or downloaded image is most likely to use, which is why FM-9
  matters for any driver that reads it.

### Plan 9: cwfs, fossil and Venti

- **cwfs / the original file server.** A "cached-worm file server": a disk cache in front of a write-once (WORM)
  store, with daily dumps (F-18). The postmortem records that the WORM jukebox held metadata and had to be present to
  serve files, and that a full restore left the system slow for days (`alternatives-postmortem.md`, Plan 9).
- **Venti.** Archival block storage in which "the SHA1 hash of a block's contents acts as the block identifier",
  which "enforces a write-once policy, preventing accidental or malicious destruction of data" and coalesces
  duplicates (F-17). Content addressing makes integrity a property of the address: a block cannot change without
  changing its address (F-17, `venti.html`).
- **Fossil.** A file server that keeps a live filesystem on local disk and archives snapshots to Venti. Each local
  block has a label with its allocation epoch; blocks from earlier epochs are immutable and copied on write, so a
  snapshot costs one counter increment (F-16). The block cache "uses soft updates to ensure that the on-disk file
  system is always self-consistent", so there is "no need to check a file system that was shut down without
  halting" (F-16).
- **Size.** fossil about 20,500 lines of C, Venti about 20,900, cwfs about 18,100 (Plan 9 Foundation tree, commit
  ed1a9c2, F-35).
- **Relevance.** A whole system built on per-process namespaces used snapshots and content addressing as its storage
  model; the parts that spread were the ideas (`alternatives-postmortem.md`, Plan 9).

### WAFL

`[search summary, not fetched]` (U-03): NetApp's Write Anywhere File Layout writes all blocks out of place and
commits periodic "consistency points" by writing a new root last; operations since the last consistency point are
logged in NVRAM and replayed after a crash. This is the copy-on-write-plus-intent-log shape that ZFS's synchronous
path also has. The original paper (Hitz, Lau, Malcolm, 1994) was not fetched. The postmortem records that Sprite's
log-structured filesystem ideas "reached NetApp products" (`alternatives-postmortem.md`, Sprite).

### bcachefs

- **Status.** The code is present in the Linux tree at v6.16 and v6.17 and absent at v6.18; MAINTAINERS lists it as
  "Externally maintained" (F-19). The reasons for the removal are not in the fetched sources.
- **Design.** Copy-on-write B-tree filesystem with checksums [inference from the project's description; not in the
  fetched sources]. Its in-tree documentation describes casefolding: case-insensitive lookups per directory with
  `chattr +F`, requiring `CONFIG_UNICODE`; enabling it sets a superblock feature bit so the filesystem can no longer be
  mounted by kernels without Unicode support; directory entries store both the original and the casefolded name
  (F-19).
- **Size.** About 115,600 lines in `fs/bcachefs` at v6.17 (F-35).
- **What went wrong.** A filesystem's fate depended on its maintainer relationship with the host project, not only on
  its design [inference from F-19's status line].

## Comparison

Sizes are from F-35 (lines of C or Rust, including comments, excluding test files).

| Design | Crash mechanism | Metadata checksums | Data checksums | Size (lines) | Applications' rename-over idiom safe without fsync? |
|---|---|---|---|---|---|
| ext4 | WAL journal, ordered data | CRC32c | no | 64,700 + 9,500 | only through the `auto_da_alloc` heuristic (F-02) |
| XFS | WAL metadata journal with intents | CRC32c, self-describing | no | 222,000 (incl. online repair) | not documented in the fetched sources |
| btrfs | CoW B-trees | yes | yes | 155,000 | not documented in the fetched sources |
| ZFS | CoW + log devices for synchronous writes (F-10) | yes | yes (Fletcher-4 default) | 283,000 core | not documented in the fetched sources |
| F2FS | LFS checkpoints + roll-forward | not documented | not documented | 49,500 | not documented |
| NILFS2 | LFS checkpoints | not documented | not documented | 23,600 | not documented |
| Fxfs | LSM trees + logical journal | journal block checksums | COW data, verified at replay | unknown | unknown (`[search summary]`) |
| RedoxFS | CoW, header ring | SeaHash | SeaHash | 6,500 core | by construction if each operation is one transaction [inference] |
| littlefs | metadata-pair logs + CoW | CRC32 | no | 7,700 | yes for its own `rename`, including across directories (F-14) |
| FAT | none | no | no | 8,300 | no |
| fossil | CoW by epoch + soft updates | via Venti scores when archived | via Venti scores when archived | 20,500 (+ Venti 20,900) | not documented |

Observations `[inference]`:

1. Every design that is consistent after a crash *by construction* (copy-on-write, logs with commit checksums) is
   either very large (ZFS, btrfs, bcachefs) because of features layered on top (RAID, compression, dedup, send/
   receive), or small (littlefs, RedoxFS) because it has none of them. The small ones show that the core mechanism is
   a few thousand lines.
2. The journaling designs had to add checksums later (ext4 2012, XFS self-describing metadata) and still have none on
   data. The copy-on-write designs had them from the start, because the parent pointer is a natural place for the
   child's checksum (GR-014).
3. The rename-over idiom is safe by documentation in only two places: ext4's heuristic and littlefs's design.
   Everywhere else the application needs `fsync` (section below), which the measured toolchain never calls
   (`requirements.md`, section 2.1).

## Crash consistency as applications experience it

**What applications assume.**

- Write-a-temp-file-then-rename to replace a file atomically, often without `fsync` (DM-038; quoted directly by the
  ext4 documentation as what "many broken applications" do, F-02).
- Truncate-and-rewrite in place (`open(O_TRUNC)/write/close`), "or worse yet" (F-02). tcc does exactly this
  (`requirements.md`, 2.2).
- That `rename` is atomic. POSIX and Linux promise atomicity only with respect to *other processes*: "there is no
  point at which another process attempting to access newpath will find it missing", and "there will probably be a
  window in which both oldpath and newpath refer to the file" (F-29). Nothing in that text speaks about crashes.
- That a retried `fsync` after an error means the data is safe. It does not on Linux (DM-037).

**What filesystems actually guarantee.**

- POSIX leaves crash behaviour undefined; persistence properties differ between filesystems and between modes of one
  filesystem; Pillai et al. found 60 crash vulnerabilities in 11 applications (DM-038). The ALICE tool from the same
  project checks an application's update protocol against an abstract model of what *any* filesystem might persist,
  so that it finds vulnerabilities "across all file systems, including future ones" (F-26).
- `fsync` on a file does not make its directory entry durable: "an explicit fsync() on a file descriptor for the
  directory is also needed" (F-31).
- Linux does not agree with itself on what `fsync` guarantees: F2FS's default `fsync_mode=posix` is lighter than
  ext4, XFS and btrfs, and only `strict` passes the xfstests case `generic/342` (F-11).
- Write-back errors: since Linux 4.13/4.16 errors are tracked by an `errseq_t` per mapping, so that each open file
  description sees an error once (F-23; DM-037); data that failed to write is dropped and the page marked clean
  (DM-037). PostgreSQL's response is to treat any flush failure as fatal: by default it raises a PANIC, because "the
  status of data in the kernel's page cache is unknown after a write-back failure ... the second attempt may be
  reported as successful, when in fact the data has been lost", and recovery from its WAL is "the only way to avoid
  data loss" (F-33). FreeBSD keeps the buffers dirty instead (DM-037).
- Explicit durability levels exist as flags: `O_SYNC` makes each `write` behave "as though each write was followed by
  a call to fsync" (F-30); ZFS's `sync` property can strengthen or (dangerously) disable synchronous semantics per
  dataset (F-10).
- Atomic creation of a complete file without a temporary name: Linux `O_TMPFILE` creates "an unnamed temporary
  regular file" that is lost on last close "unless the file is given a name" with `linkat` (F-30). Windows offers
  `ReplaceFile` as its atomic-replace primitive (DM-038). Exclusive rename (`RENAME_NOREPLACE`) and atomic exchange
  (`RENAME_EXCHANGE`) exist on Linux but "require support from the underlying filesystem" (F-29).
- Multi-file transactions (TxF, btrfs transaction ioctls) were tried and abandoned; TxOS showed modest overhead in
  research (DM-045, *contested*).

**What this means for a new design** `[inference]`. A new filesystem can make the common idioms correct by
construction rather than by heuristic: if every namespace operation and every `close` of a written file is a
transaction that commits atomically, then write-then-rename is safe without `fsync` (only *recency* is lost in a
crash, never consistency), and `fsync` reduces to "wait for the next commit". The open choice is whether a crash may
lose recent completed operations (bounded by a commit interval) or not (every operation durable before it returns,
ZFS `sync=always` and its "large performance penalty", F-10).

## Testing crash consistency

| Technique | What it does | Tools and evidence |
|---|---|---|
| **Record and replay of block writes** | Log every write the filesystem issues, in completion order, cut at flush boundaries; replay prefixes of the log onto a copy of the device and check the result | Linux `dm-log-writes` logs writes "in order of completion once we are sure the write is no longer in cache": a write is logged only when the next flush completes, and only writes that had completed before the flush was *issued* are attached to it, "to simulate the worst case scenario with regard to power failures"; FUA writes are logged at completion (F-24). |
| **Enumerating crash states within flush boundaries** | Between two flushes the device may persist any subset of the completed writes, in any order. Generate those subsets (bounded) and check each | CrashMonkey ("record-replay-and-test") with Ace, which "exhaustively generates sequences of file-system operations" within bounds (bounded black-box testing, B3): found 8 btrfs bugs, 2 F2FS bugs and 1 FSCQ bug (a verified filesystem whose unverified C bindings lost `fdatasync` data) (F-25). |
| **Model-based testing against a reference** | Run the same operation sequence on the implementation and on a simple in-memory model of the specification; after each operation, and after each simulated crash, the implementation's observable state must equal some state the model allows | CrashMonkey "automatically tests for data and metadata consistency of persisted files" after each simulated crash (F-25); ALICE replaces the filesystem by an abstract persistence model to find application-level bugs (F-26). xfstests/fstests is the regression suite for Linux filesystems, with "generic" cases shared by all of them (F-28). |
| **Fuzzing on-disk images** | Mutate images (whole blocks or individual fields) and mount, check and repair them | XFS: write garbage to every metadata block of an image containing every metadata type, then field-level fuzzing of "every metadata field of every metadata object" with eight transformations each, checking the kernel verifiers, `xfs_repair` and `xfs_scrub` (F-07). Janus fuzzes images and system calls together, running in-kernel filesystems in user space (LKL), and found memory-safety bugs in Linux filesystems (F-27). syzbot generates deliberately corrupted images; the HFS maintainer's position in 2023 was that "nothing" could be done about deliberately corrupted filesystems, while syzbot's maintainer pointed out that distributions auto-mount such images from USB devices (U-04, `[search summary, not fetched]`). |
| **Verification** | Prove crash safety | FSCQ, mentioned by F-25 as a target in which CrashMonkey still found a bug outside the verified part. |

`[inference]` The combination that fits a host-tested `no_std` crate is cheap: a block-device trait with an in-memory
implementation that records writes and flushes (dm-log-writes in a few hundred lines), a crash-state enumerator over
the recorded log (CrashMonkey's idea without kernel instrumentation), a reference model of the namespace and file
contents (ALICE/CrashMonkey's oracle), and a field-level image fuzzer driven by the on-disk format definitions (XFS's
approach), all running under `cargo test`.

## Names

| Choice | Who does it | Problems it caused | Evidence |
|---|---|---|---|
| Arbitrary bytes except `/` and NUL | Linux, BSDs, POSIX | newline, control and escape bytes break tools and terminals; leading `-` turns into options; names cannot be displayed or compared reliably; languages need a separate OS-string type | DM-095; Rust's `OsString` exists because "on Unix systems, strings are often arbitrary sequences of non-zero bytes" (F-34) |
| Valid UTF-8 only | APFS (with an unassigned-code-point rule pinned to Unicode 9.0); ZFS `utf8only=on` (off by default) | rejecting names breaks round-tripping of archives and media from elsewhere; pinning a Unicode version | DM-095; F-10 |
| 16-bit code units, unpaired surrogates allowed | NTFS | names need not be valid Unicode | DM-096 |
| Normalisation on compare, names stored unmodified | ZFS `normalization=formC/D/KC/KD` (default none) | equality becomes a function of Unicode tables | F-10; DM-094 |
| Normalisation on store | HFS+ (almost NFD) | HFS+ used Unicode 3.2 and APFS 9.0; migration mismatches | DM-094 |
| Case-insensitive | Windows, macOS by default; FAT on Linux by default; ext4 per directory (the superblock records the Unicode version and encoding; names are compared after canonical decomposition, stored as given, but large directories hash the casefolded name, so "the normalization format used actually has an impact on where the directory entry is stored", F-02; encrypted casefolded entries carry extra hash data, F-36), F2FS, bcachefs per directory | CVE-2014-9390 (`.git/config` overwrite via case, ignorable code points or `git~1`); dcache support "half baked"; feature bits make images unmountable on kernels without Unicode support | DM-094; F-13; F-19 |
| Forbidden characters and reserved names | Windows (bytes 1–31, punctuation, `CON`, `NUL`, ...) | names valid on NTFS unreachable through Win32; reserved names in every directory | DM-043; DM-095 |
| Length limits | Linux `NAME_MAX` 255 bytes, `PATH_MAX` 4,096 bytes including NUL (F-32); Win32 `MAX_PATH` 260 characters | fixed buffers leak into every application (DM-043); relative paths cannot escape the limit on Windows | F-32; DM-043 |
| Confusable names | any Unicode-aware system | "users can be tricked into doing things they do not desire when malicious actors exploit quirks of Unicode to place misleading names in directories": XFS's online checker reports such names (F-07) | F-07 |
| Lexical `..` and no symlinks | Plan 9; Fuchsia (no `..` on file servers) | breaks POSIX expectations; Fuchsia's client-side canonicalisation works only because it has no symlinks | DM-039 |

`[inference]` The record supports three separate decisions: the *encoding* (validate UTF-8 or not), the *equality*
(exact bytes, or folded/normalised), and the *forbidden set* (control characters, `/`, NUL). They are separable: ZFS
validates without normalising (`utf8only=on`, `normalization=none`); ext4 folds per directory. Exact-byte equality
over validated UTF-8 with control characters forbidden avoids every failure in the table except the archive
round-trip cost, which then falls on the POSIX personality (DM-095, X-8).

## Block devices

What a filesystem on QEMU `virt` can assume about its disk, from the virtio specification and QEMU's implementation.

- **Ordering.** Requests are "serviced (probably out of order) by the device"; the device uses them "not necessarily
  in order" (F-20).
- **Units.** Sector numbers and lengths are always 512-byte units; `blk_size` is only the "optimal sector size" and
  "does not affect the units used in the protocol" (F-20).
- **Durability.** "A write is considered volatile when it is submitted; the contents of sectors covered by a volatile
  write are undefined in persistent device backend storage until the write becomes stable." A completed write becomes
  stable if (1) `VIRTIO_BLK_F_FLUSH` was offered but neither it nor `VIRTIO_BLK_F_CONFIG_WCE` was negotiated, (2) the
  cache was in writethrough mode for the whole time between submission and completion, (3) the write carried the FUA
  flag (`VIRTIO_BLK_F_REQ_FLAGS_OUT_FUA`), or (4) a flush request "is sent after the write is completed and is
  completed itself". A device backed by persistent storage "MUST ensure that stable writes are committed to it". A
  device that does not offer `VIRTIO_BLK_F_FLUSH` "MAY" commit before completion but need not, and then "will not be
  resilient to data loss in case of crashes" (F-20).
- **Atomicity.** The specification states none: the contents of unstable sectors are "undefined" (F-20).
- **Errors.** Each request ends with `VIRTIO_BLK_S_OK`, `VIRTIO_BLK_S_IOERR` ("device or driver error") or
  `VIRTIO_BLK_S_UNSUPP`; after a failed multi-segment discard or write-zeroes, each segment's state is indeterminate
  (F-20).
- **QEMU.** In `virtio_blk_get_features` the virtio-blk model adds the flush feature (`VIRTIO_BLK_F_WCE`, the legacy
  name of `VIRTIO_BLK_F_FLUSH`) if `blk_enable_write_cache(s->blk) || (s->conf.x_enable_wce_if_config_wce &&
  virtio_has_feature(features, VIRTIO_BLK_F_CONFIG_WCE))`; the device properties `config-wce` (which sets
  `VIRTIO_BLK_F_CONFIG_WCE`) and `x-enable-wce-if-config-wce` both default to true. With default properties QEMU
  therefore offers flush in **every** cache mode, and a guest that negotiates `CONFIG_WCE` can switch the cache
  between writeback and writethrough itself. Its source has no support for the FUA request flag. `-drive cache=`
  defaults to `writeback`; `writethrough` and `directsync` only change the default of the device's write cache;
  `unsafe` sets `cache.no-flush`, telling QEMU "it never needs to write any data to the disk" (F-21).
- **Linux's model for comparison.** `REQ_PREFLUSH` guarantees "that previously completed write requests are on
  non-volatile storage before the flagged bio starts"; `REQ_FUA` completes a request "only ... after the data has been
  committed to non-volatile storage"; for devices without a volatile cache the block layer strips both flags, and it
  emulates them on devices that have one (F-22). The device-trust problem of virtio drivers is GR-021.

## Sources

All fetched on 2026-10-25. `raw` means `https://raw.githubusercontent.com/`.

| ID | Source | URL fetched (revision) |
|---|---|---|
| F-01 | GNU make manual, Texinfo source (GitHub mirror of the GNU make repository): "Rule Syntax", "Interrupting or Killing make", `.LOW_RESOLUTION_TIME` | raw `mirror/make/master/doc/make.texi` |
| F-02 | Linux kernel, "ext4 General Information" (admin guide): mount options `data=`, `commit=`, `auto_da_alloc`, `errors=`, journaling modes | raw `torvalds/linux/master/Documentation/admin-guide/ext4.rst` |
| F-03 | Linux kernel, ext4 on-disk format: journal (JBD2), fast commits, journal checksums | raw `torvalds/linux/master/Documentation/filesystems/ext4/journal.rst` |
| F-04 | Linux kernel, ext4 on-disk format: checksums (`metadata_csum`) | raw `torvalds/linux/master/Documentation/filesystems/ext4/checksums.rst` |
| F-05 | Linux kernel, "XFS Self Describing Metadata" | raw `torvalds/linux/master/Documentation/filesystems/xfs/xfs-self-describing-metadata.rst` |
| F-06 | Linux kernel, "XFS Logging Design" | raw `torvalds/linux/master/Documentation/filesystems/xfs/xfs-delayed-logging-design.rst` |
| F-07 | Linux kernel, "XFS Online Fsck Design" (risks, fuzz testing plan, Unicode confusables) | raw `torvalds/linux/master/Documentation/filesystems/xfs/xfs-online-fsck-design.rst` |
| F-08 | Linux kernel, btrfs overview | raw `torvalds/linux/master/Documentation/filesystems/btrfs.rst` |
| F-09 | btrfs-progs documentation, feature status page | raw `kdave/btrfs-progs/devel/Documentation/Status.rst` |
| F-10 | OpenZFS `zfsprops(7)`: `checksum`, `sync`, `casesensitivity`, `normalization`, `utf8only` | raw `openzfs/zfs/master/man/man7/zfsprops.7` |
| F-11 | Linux kernel, "Flash-Friendly File System (F2FS)" | raw `torvalds/linux/master/Documentation/filesystems/f2fs.rst` |
| F-12 | Linux kernel, NILFS2 | raw `torvalds/linux/master/Documentation/filesystems/nilfs2.rst` |
| F-13 | Linux kernel, VFAT | raw `torvalds/linux/master/Documentation/filesystems/vfat.rst` |
| F-14 | littlefs, "The design of littlefs" | raw `littlefs-project/littlefs/master/DESIGN.md` |
| F-15 | RedoxFS README and source (`header.rs`, `filesystem.rs`, `block.rs`), GitHub mirror | raw `redox-os/redoxfs/master/README.md` and `src/*.rs`; clone at b87b0976ee12 |
| F-16 | Quinlan, McKie, Cox, "Fossil, an Archival File Server" (Plan 9 documentation) | raw `plan9foundation/plan9/main/sys/doc/fossil.ms` |
| F-17 | `venti(7)` (plan9port) and Quinlan, Dorward, "Venti: a new approach to archival storage" (HTML in the Plan 9 tree) | raw `9fans/plan9port/master/man/man7/venti.7`; `sys/doc/venti/venti.html` from the clone in F-35 |
| F-18 | `cwfs(4)`, Plan 9 manual | raw `plan9foundation/plan9/main/sys/man/4/cwfs` |
| F-19 | Linux kernel bcachefs documentation (v6.16), MAINTAINERS (master, v6.17), presence of `fs/bcachefs/Kconfig` at v6.16, v6.17 (HTTP 200) and v6.18, master (HTTP 404) | raw `torvalds/linux/v6.16/Documentation/filesystems/bcachefs/{index,casefolding}.rst`; raw `torvalds/linux/master/MAINTAINERS` |
| F-20 | OASIS VIRTIO specification source, Block Device section | raw `oasis-tcs/virtio-spec/master/device-types/blk/description.tex` |
| F-21 | QEMU: virtio-blk device model and `-drive cache=` documentation | raw `qemu/qemu/master/hw/block/virtio-blk.c`; raw `qemu/qemu/master/qemu-options.hx` |
| F-22 | Linux kernel, "Explicit volatile write back cache control" (`REQ_PREFLUSH`, `REQ_FUA`) | raw `torvalds/linux/master/Documentation/block/writeback_cache_control.rst` |
| F-23 | Linux kernel, `errseq_t` | raw `torvalds/linux/master/Documentation/core-api/errseq.rst` |
| F-24 | Linux kernel, `dm-log-writes` | raw `torvalds/linux/master/Documentation/admin-guide/device-mapper/log-writes.rst` |
| F-25 | CrashMonkey and Ace README (Mohan et al., OSDI 2018) | raw `utsaslab/crashmonkey/master/README.md` |
| F-26 | ALICE README (Pillai et al., OSDI 2014) | raw `madthanu/alice/master/README.md` |
| F-27 | Janus README (Xu et al., IEEE S&P 2019) | raw `sslab-gatech/janus/master/README.md` |
| F-28 | xfstests/fstests README | raw `kdave/xfstests/master/README` |
| F-29 | Linux man-pages, `rename(2)` | raw `mkerrisk/man-pages/master/man2/rename.2` |
| F-30 | Linux man-pages, `open(2)` (`O_SYNC`, `O_TMPFILE`) | raw `mkerrisk/man-pages/master/man2/open.2` |
| F-31 | Linux man-pages, `fsync(2)` | raw `mkerrisk/man-pages/master/man2/fsync.2` |
| F-32 | Linux UAPI `limits.h` (`NAME_MAX`, `PATH_MAX`) | raw `torvalds/linux/master/include/uapi/linux/limits.h` |
| F-33 | PostgreSQL documentation, `data_sync_retry` | raw `postgres/postgres/master/doc/src/sgml/config.sgml` |
| F-34 | Rust standard library, `std::ffi::OsString` documentation comment | raw `rust-lang/rust/master/library/std/src/ffi/os_str.rs` |
| F-35 | Line counts made for this survey: `git clone --depth 1` (sparse where large) of Linux v6.17 (e5f0a698b34e), OpenZFS master (ea111ca50fed), littlefs master (6cb4e86540ec), RedoxFS master (b87b0976ee12), Plan 9 Foundation `plan9` main (ed1a9c21e329); `find <dir> -name '*.[ch]' ! -name '*test*' \| xargs cat \| wc -l` over `fs/<name>` (Linux), `module/zfs` + `include/sys` (OpenZFS core) or `module` + `include` (total); littlefs: only `lfs.c`, `lfs.h`, `lfs_util.c`, `lfs_util.h` (the whole tree gives 11,314); RedoxFS: `*.rs` in `src/` without `src/mount/`, `src/bin/` and `tests.rs` (core, 6,503) and all of `src/` (11,321); Plan 9: `sys/src/cmd/{fossil,venti,cwfs}` | `https://github.com/<project>` |
| F-36 | Linux kernel, ext4 directory format (casefolded and encrypted directory entries) | raw `torvalds/linux/master/Documentation/filesystems/ext4/directory.rst` |
| F-37 | GNU Autoconf, `lib/m4sugar/m4sh.m4` (`_AS_LN_S_PREPARE`: `ln -s` probe with `ln` and `cp -pR` fallbacks) and `lib/autoconf/programs.m4` (`AC_PROG_LN_S`), GitHub mirror | raw `autotools-mirror/autoconf/master/lib/m4sugar/m4sh.m4`; raw `autotools-mirror/autoconf/master/lib/autoconf/programs.m4` |
| F-38 | Linux man-pages, `link(2)` (`EPERM`: "the filesystem ... does not support the creation of hard links") | raw `mkerrisk/man-pages/master/man2/link.2` |

**Not fetched** (search-engine summaries only; weaker evidence):

| ID | What | Pages the search engine summarised |
|---|---|---|
| U-01 | Fuchsia RFC-0136 "Fxfs" (Minfs limitations, LSM trees, logical journal, COW data checksums verified at replay) | `https://fuchsia.dev/docs/contribute/governance/rfcs/0136_fxfs` |
| U-02 | Fxfs journal source documentation (block checksums seeded by the previous block; replay stops at a checksum failure); Minfs-to-Fxfs migration | `https://docs.fuchsia.dev/rust/src/fxfs/object_store/journal.rs.html`; `https://fuchsia.dev/reference/assembly/DataFvmVolumeConfig` |
| U-03 | WAFL consistency points and NVRAM log (Hitz, Lau, Malcolm, "File System Design for an NFS File Server Appliance", 1994) | `https://www.cs.princeton.edu/courses/archive/fall21/cos318/readings/netapp.pdf` and a secondary summary |
| U-04 | Corbet, "Unmaintained filesystems as a threat vector", LWN, July 2023, and the LKML thread on syzbot HFS reports | `https://lwn.net/Articles/939097`; `https://lkml.iu.edu/hypermail/linux/kernel/2307.2/06577.html` |
