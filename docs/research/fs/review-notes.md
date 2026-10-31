# Review notes: TASK-06 filesystem batch (ADR-0010 to ADR-0015 and their evidence)

- **Date:** 2026-10-25
- **Reviewer:** independent agent that did not draft the documents under review. No ADR, research file or index was
  edited; this file is the only output.
- **Scope:** `docs/adr/0010-filesystem-client-guarantees.md`, `0011-filesystem-crash-consistency-mechanism.md`,
  `0012-filesystem-integrity.md`, `0013-filesystem-names-and-namespace.md`, `0014-filesystem-metadata.md`,
  `0015-block-device-contract.md`; their evidence `docs/research/fs/requirements.md`, `survey.md`,
  `implementation-outline.md`.

## Method

1. Read in full: `docs/tasks/TASK-06-filesystem-design.md`, `CLAUDE.md`, `docs/adr/0000-template.md`,
   `0001-decision-process.md`, `0002-native-posix-boundary.md`, `0005-posix-scope.md`, `docs/adr/review-notes.md`
   (structure and severity scale copied from it), the six ADRs, `requirements.md`, `survey.md`,
   `implementation-outline.md`. Read the Recommendation sections of ADR-0003, ADR-0004 and ADR-0006 (ADR-0006 in
   full for rule 1, rule 7 and the FIFO jobserver mapping; ADR-0003 for the `memfd` path).
2. Opened every DM/GR/CP/X entry the six ADRs cite and compared the ADR text with the entry: DM-003, DM-006, DM-009,
   DM-010, DM-016, DM-037, DM-038, DM-039, DM-040, DM-042, DM-043, DM-044 (context), DM-045, DM-063, DM-094, DM-095,
   DM-096; GR-003, GR-004, GR-014, GR-015, GR-017, GR-021; CP-01, CP-03, CP-04, CP-12, CP-14, CP-15, CP-17, CP-19,
   CP-20, CP-22, CP-26, CP-27; X-8, X-14; `alternatives-postmortem.md` sections Plan 9 / Inferno, Sprite,
   Fuchsia / Zircon. Section slugs were resolved by `zoraya-adr-check` (step 7) and the cited sections read.
3. Recomputed every number in `requirements.md` with my own scripts (`python3 -I`, standard library only, plus
   `awk`/`grep` on the TSV), from:
   - `docs/research/task-02/syscall-surface.csv`: per-tier totals with the tier→workload map T0 = `W5-dash`;
     T1 = `W1-configure`, `W1-make`, `W2-make-g`, `W3-build`; T2 = `W4-configure`, `W4-make`; T3 = `W6-compile`,
     `W6-link`, for all 22 rows of the §1.2 table plus `mmap`; presence/absence of the 20 syscalls listed as absent.
   - `docs/research/task-02/libc-surface.csv`: tier, static importers and dynamic callers of every function in §1.1
     and of the 26 functions listed below it.
   - `tools/posix-surface/results/strace-patterns.tsv`: every `core` row for `openat`, `newfstatat`, `faccessat`,
     `faccessat2`, `fcntl`, `unlinkat`, `mkdirat`, `symlinkat`, `readlinkat`, `fchmodat`, `renameat`, `mmap`
     (open-flag table §1.3, §2.2, §2.3, §3.2, §4 table).
   - `docs/research/task-02/callsites.csv`: feature counts (no `DM-037` row; 3 `DM-038` rows; 20 `DM-039` rows) and
     every row the file cites by file:line.
   - `docs/research/task-02b/syscall-delta.csv`: `fcntl`, `mknodat`, `unlinkat`, `openat` rows.
   - `docs/research/task-02b/event-timelines.md`, `parallel-surface.md`, `research-log.md` for the W9 statements;
     `tools/posix-surface/workloads/w8/leaf.mk`; `tools/posix-surface/analysis/callsites.py` (M3 scope).
   All numbers match except where a finding says otherwise (see the paragraph under the `requirements.md` table).
4. Fetched these survey sources myself (HTTP 200 each, into the scratchpad, read as data) and checked every quoted
   or paraphrased statement the survey or an ADR takes from them. `raw` = `https://raw.githubusercontent.com/`.
   - F-01 raw `mirror/make/master/doc/make.texi`
   - F-02 raw `torvalds/linux/master/Documentation/admin-guide/ext4.rst`
   - F-03 raw `torvalds/linux/master/Documentation/filesystems/ext4/journal.rst`
   - F-04 raw `torvalds/linux/master/Documentation/filesystems/ext4/checksums.rst`
   - F-05 raw `torvalds/linux/master/Documentation/filesystems/xfs/xfs-self-describing-metadata.rst`
   - F-07 raw `torvalds/linux/master/Documentation/filesystems/xfs/xfs-online-fsck-design.rst`
   - F-09 raw `kdave/btrfs-progs/devel/Documentation/Status.rst`
   - F-10 raw `openzfs/zfs/master/man/man7/zfsprops.7`
   - F-11 raw `torvalds/linux/master/Documentation/filesystems/f2fs.rst`
   - F-12 raw `torvalds/linux/master/Documentation/filesystems/nilfs2.rst`
   - F-13 raw `torvalds/linux/master/Documentation/filesystems/vfat.rst`
   - F-14 raw `littlefs-project/littlefs/master/DESIGN.md`
   - F-15 raw `redox-os/redoxfs/master/src/header.rs`, `src/block.rs`
   - F-16 raw `plan9foundation/plan9/main/sys/doc/fossil.ms`
   - F-18 raw `plan9foundation/plan9/main/sys/man/4/cwfs`
   - F-19 raw `torvalds/linux/v6.16/Documentation/filesystems/bcachefs/casefolding.rst`; `fs/bcachefs/Kconfig` at
     v6.16 (200), v6.17 (200), v6.18 (404)
   - F-20 raw `oasis-tcs/virtio-spec/master/device-types/blk/description.tex`
   - F-21 raw `qemu/qemu/master/hw/block/virtio-blk.c`, raw `qemu/qemu/master/qemu-options.hx`
   - F-22 raw `torvalds/linux/master/Documentation/block/writeback_cache_control.rst`
   - F-23 raw `torvalds/linux/master/Documentation/core-api/errseq.rst`
   - F-24 raw `torvalds/linux/master/Documentation/admin-guide/device-mapper/log-writes.rst`
   - F-25 raw `utsaslab/crashmonkey/master/README.md`
   - F-29 raw `mkerrisk/man-pages/master/man2/rename.2`
   - F-30 raw `mkerrisk/man-pages/master/man2/open.2`
   - F-31 raw `mkerrisk/man-pages/master/man2/fsync.2`
   - F-32 raw `torvalds/linux/master/include/uapi/linux/limits.h`
   - F-33 raw `postgres/postgres/master/doc/src/sgml/config.sgml`
   - F-37 raw `autotools-mirror/autoconf/master/lib/m4sugar/m4sh.m4`
   - F-38 raw `mkerrisk/man-pages/master/man2/link.2`
5. Recounted F-35 line counts with the survey's command
   (`find <dir> -name '*.[ch]' ! -name '*test*' | xargs cat | wc -l`, `*.rs` for RedoxFS):
   - littlefs, `git clone --depth 1 https://github.com/littlefs-project/littlefs.git`, HEAD 6cb4e86540ec:
     `lfs.c lfs.h lfs_util.c lfs_util.h` = 7,670 (survey: about 7,700). The whole tree with the stated command gives
     11,314 (see F-SUR-5).
   - RedoxFS, `git clone --depth 1 https://github.com/redox-os/redoxfs.git`, HEAD b87b0976ee12: `src/` without
     `src/mount/`, `src/bin/` and `tests.rs` = 6,503 (survey: about 6,500); all of `src/` = 11,321 (survey: 11,300).
   - Linux, `git clone --depth 1 --branch v6.17 --filter=blob:none --sparse https://github.com/torvalds/linux.git`,
     HEAD e5f0a698b34e: `fs/ext4` 64,740, `fs/jbd2` 9,525, `fs/f2fs` 49,484, `fs/nilfs2` 23,628, `fs/fat` 8,347
     (survey: 64,700, 9,500, 49,500, 23,600, 8,300). ADR-0011's "ext4 plus JBD2 about 74,000" = 74,265.
6. For each `U-` item, searched all six ADRs for the claims that come from it and checked the label.
7. Ran `cargo run -q -p zoraya-adr-check -- docs` from `/home/user/repo`. Result: `12 ADRs checked`, 34 warnings
   (unchecked invariants of accepted ADRs, expected), and exactly 6 errors, all
   `INV-0001-5: index.md: 00NN-….md is not listed` for 0010–0015, which this task may not fix. Nothing else was
   reported: headings, status lines, Evidence lines and every citation slug of the six ADRs pass.
8. For each invariant, asked whether the stated check is an observable property a host test, lint or CI script could
   decide, whether it can fail, and whether it contradicts another rule in the batch or in ADR-0002 to ADR-0006.

Severity: **error** = claim contradicts the source, or two rules contradict each other; **imprecision** = right in
substance, wrong in detail or attribution; **note** = labelling, wording, or a steelman that could be stronger.
Class: a = citation does not support claim, b = option strawmanned / evidence missing, c = recommendation does not
follow, d = invariant not testable, other.

## ADR-0010: Filesystem guarantees to clients

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0010-1 | a | Context, "What is measured", bullet 1 | "no `fsync` … in any run of T0–T3, and no compiled call site in any measured program". The runs part is right (no such row in `syscall-surface.csv`). The call-site part is not: M3 (`tools/posix-surface/analysis/callsites.py`) searched dash, oksh, mrsh, tcc, gmake, pdpmake and the **minimal T2 builds** of toybox and busybox ("dead code and unconfigured applets excluded"); clang and ld.lld were never searched, and `libc-surface.csv` shows `fsync`, `fdatasync`, `sync` imported by the full `busybox;toybox` binaries (`unused-applets`). Inherited from `requirements.md` §2.1 (F-REQ-2). | imprecision | "no call site in the code paths of the T0–T2 programs as built for the workloads (M3 did not search T3 or the full multi-call binaries)". |
| F-0010-2 | a | Option C, Evidence for and Precedent | fossil is cited as a precedent for "every operation atomic, crash state is a prefix": "fossil keeps the disk 'always self-consistent' so no check is needed after a crash". The source says this is achieved with soft updates (F-16: "The block cache uses soft updates [1] to ensure that the on-disk file system is always self-consistent"), and ADR-0011 Option E states of soft updates: "Consistency, not prefix semantics: the order of *user-visible* operations is not preserved across files". The batch thus uses fossil as evidence for the prefix rule in one ADR and as a counterexample in the next. | imprecision | Cite fossil only for "no check after a crash", not for prefix semantics; keep littlefs (and Fxfs, labelled) as the Option C precedents. |
| F-0010-3 | a | Option C, Evidence for | "Copy-on-write designs give prefix semantics naturally by committing batches in order (`fs/survey.md#comparison`)". The Comparison section says designs are "consistent after a crash *by construction*" (observation 1) and nothing about prefix order; the closest text is the survey's own `[inference]` under "What this means for a new design". | note | Mark `[inference]` or cite the inference paragraph. |
| F-0010-4 | a | Option C, Evidence against | "the ext3 'fsync flushes everything' cost that made Firefox's `fsync` slow (DM-038)". DM-038 says only "Ts'o cites the fsync performance problems of Firefox 3.0"; the mechanism (ext3 flushing everything on `fsync`) is not in DM-038. | note | Label the mechanism `[outside evidence]` or reduce to "the Firefox 3.0 fsync cost (DM-038)". |
| F-0010-5 | b | Option E | DM-045 is *contested* and its own text weakens the "abandoned" argument: the btrfs abandonment "says more about Ceph's needs than about transactions in general", and TxOS built a transactional ext3 "in under a month". Neither is in Option E's Evidence for. Precedent "btrfs transaction ioctls (removed)": DM-045 says "deprecated in 2017 (slated for removal in 4.17)". | note | Add both points to Evidence for; say "deprecated, slated for removal". |
| F-0010-6 | d | INV-0010-1 (and INV-0010-2) | "every device state ADR-0015 allows at each cut is remounted". Under ADR-0015 rule 2 an unstable sector may hold "arbitrary bytes", and between flushes any subset of writes may persist, so the set is not enumerable; `implementation-outline.md` test 2 bounds it ("all subsets when there are at most `k` unstable writes [assumption: k = 12], random subsets otherwise"). As written the check cannot be run. | imprecision | State the bound in the check: exhaustive over subsets up to `k` unstable writes and over a fixed set of torn-sector patterns (old, new, zero, a seeded random pattern), sampled with a fixed seed and count beyond `k`. |
| F-0010-7 | a | Option D, Evidence against | "The toolchain issues tens of thousands of writes per W4 run …, each of which would cost at least one device flush". The `write`/`writev` counts in `requirements.md` §1.2 include writes to pipes, terminals and `/dev/null` and by `built` programs; the data does not say how many target files. Only file writes would flush. | note | "tens of thousands of `write` calls, an unknown share of them to files". |
| F-0010-8 | other | Rule 6, write-back failure | "reads continue to be served" in state `Failed` does not say from which state: the in-memory state that includes completed-but-not-durable requests (which rule 6 says "are lost") or the durable prefix. A client can read data after the error that disappears at remount, without seeing `StorageFailed` if it never mutates. | note | Say which state reads see; if the in-memory one, say that reads after `Failed` may return data that a remount discards. |

ADR-0010 otherwise checks out. DM-037 (error reported once, retry falsely succeeds, FreeBSD keeps buffers dirty,
PostgreSQL PANIC and WAL), DM-038 (60 vulnerabilities in 11 applications; zero-length files after ext4; ext3 ordered
mode a security measure; delayed allocation exists because ordering costs throughput), DM-045 (*contested*, TxOS
about 10 %), CP-19, X-14 say what the ADR attributes to them. The F-29 rename quote, the F-31 directory-`fsync`
quote, F-11's `fsync_mode`, F-10's `sync=always` "large performance penalty", F-02's `auto_da_alloc` and 5-second
commit default, and F-33's PostgreSQL text are verbatim in the fetched sources. Five options, Linux baseline (A),
non-Unix precedents (littlefs, fossil, Fxfs labelled "search summary only"). The recommendation follows from driver
1 (no measured program ever requests durability), which the data supports. The personality mapping's conflicts with
ADR-0005 and ADR-0013 are in the cross-ADR section (F-X-1, F-X-2).

## ADR-0011: Crash-consistency mechanism

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0011-1 | a | Context, "What is measured" | "The workload is write-heavy …: W4 alone issues about 93,000 `write` and 13,000 `writev` calls". The numbers are right (T2 column: 93,001 and 13,136), but they count all `write` calls, including pipes, terminals, `/dev/null` and `built` programs (`requirements.md` §1.2 "including `built` programs"); "write-heavy" for the filesystem does not follow. ADR-0012 reads the same table the opposite way (F-0012-1). | imprecision | Say what the numbers count and that the file share is unknown; drop "write-heavy" or base it on file-specific data (e.g. 1,110 `unlinkat`, 1,231 `O_CREAT|O_TRUNC` opens in T2 by tcc and dash). |
| F-0011-2 | a | Context, "What is measured" | "in-place rewriting of existing files is limited to tcc truncating its outputs". `requirements.md` §1.3 also has dash opening with `O_WRONLY|O_CREAT|O_TRUNC` 7 (T1) / 892 (T2) times for `>` redirections and with `O_APPEND` 13 / 380 times (configure's log, appended by many processes in sequence). Both rewrite or extend existing files in place. | imprecision | "in-place rewriting is tcc's output truncation, shell `>` redirections and `>>` appends to logs; no database-like random overwrite was measured". |
| F-0011-3 | b | Considered options | The task requires the ADR to "state the expected implementation size and how each option would be tested". Sizes are given per option; testing is given only for the recommended Option B ("How it would be tested"). Options A, C, D, E have at most a remark on crash-state space. | imprecision | Add one line per option: A (journal replay idempotency, crash between home-location writes), C (cleaner interleavings, roll-forward), D (log replay plus tree, two recovery paths), E (dependency-order enumeration). |
| F-0011-4 | other | Option C, Evidence against | "Its main advantage, sequential writes, matters for flash and rotating disks, not for a virtio disk backed by a host file (ADR-0015)." ADR-0015 says nothing about host-file performance; the host file may itself sit on flash. Unlabelled inference. | note | Label `[inference]` and drop the ADR-0015 citation. |
| F-0011-5 | a | Option D, Evidence for | "ZFS offers synchronous semantics this way (`fs/survey.md#zfs`)". The survey's ZFS section describes the `sync` property only; it does not describe an intent log. (F-10 mentions "pool log devices" under `logbias`, which the survey does not quote.) | note | Cite F-10's `logbias` text in the survey's ZFS section, or label the ZIL description `[outside evidence]`. |
| F-0011-6 | a | Context, "Survey" paragraph | "copy-on-write trees (ZFS, btrfs, bcachefs, RedoxFS, WAFL) and log-structured designs (…, Fxfs's LSM trees)". WAFL and Fxfs are `U-` items and bcachefs being copy-on-write is `[inference …; not in the fetched sources]` in the survey; the Context carries no label, while the option text does. | note | Add "(WAFL, Fxfs: search summary only; bcachefs: inference)". |
| F-0011-7 | other | Option B Evidence for; "Why B" | "the crash states per commit are two". The ADR's own test plan enumerates "each prefix of completed writes, each subset allowed between flushes, torn root records" per commit. Two is the number of *post-mount logical* states, not of device states, which is what driver 3 is about. | note | "two logical outcomes per commit (old or new root), whatever the device state". |

ADR-0011 otherwise checks out. RedoxFS's 256-slot header ring (`HEADER_RING = 256`, F-15), littlefs about 7,700 and
RedoxFS about 6,500 lines, ext4+JBD2 74,265, F2FS 49,484 and NILFS2 23,628 (my recount), NILFS2's cleaner and missing
`fsck` (F-12), btrfs RAID56 "unstable" (F-09), fossil's "not yet replaced" and soft updates (F-16), Sprite's LFS
reaching NetApp and flash controllers (`alternatives-postmortem.md#sprite`), GR-014's caveats (fragmentation,
`nodatacow`) all match. Five options, Linux baseline (A), non-Unix precedents (littlefs, Fxfs labelled, fossil).
The commit protocol (rule 3) is exactly what ADR-0015 rules 3–4 require: blocks complete, flush, root record, flush,
then reply. The recommendation follows from ADR-0010's prefix rule for data overwrites, which only B gives without
data journaling.

## ADR-0012: Integrity

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0012-1 | a | Context, "What is measured" | "the toolchain reads far more than it writes (W4: about 66,000 `read` calls; T1: 9,237 …)". From the same `requirements.md` §1.2 table, W4 (T2 column) has `write` 93,001 + `writev` 13,136 against `read` 65,748 + `readv` 7,772: W4 issues more write calls than read calls. Only T1 is read-dominated (9,237 + 54 vs 43 + 581). ADR-0011 reads the same table as "write-heavy". Neither count is file-only (F-0011-1). | error | Restate with the right numbers and caveat (all fds, `built` included); the read-path argument can rest on T1, or on the number of distinct files opened read-only (tcc's 3,998 / 24,796 `O_RDONLY` opens). |
| F-0012-2 | a | Context, "What the record says", bullet 4 | "one Linux maintainer's position is that nothing can be done about a deliberately corrupted filesystem, while distributions auto-mount images from USB devices; … (`fs/survey.md#testing-crash-consistency`, `fs/survey.md#xfs`)". These two statements come from U-04 (LWN / LKML), marked `[search summary, not fetched]` in the survey, but the ADR gives them as plain citations. The syzbot mention comes from the same item. | imprecision | Add "(U-04, search summary, not fetched)" after the two statements. |
| F-0012-3 | a | Option C, Precedent | "Fuchsia's verified blobs (search summary only, `fs/survey.md#fxfs-and-minfs-fuchsia`)". The cited survey section covers Minfs, Fxfs's LSM trees and logical journal, and the migration; it says nothing about blobs or verified storage. | imprecision | Drop the precedent or label it `[outside evidence]` with a source. |
| F-0012-4 | a | Context bullet 1; Option B title and Evidence | "ZFS and btrfs checksum everything, store each block's checksum in its parent" and Option B "(ZFS, btrfs, RedoxFS)". GR-014 states the parent-pointer placement for ZFS ("each block's checksum is kept in metadata (the block pointer …)"), F-15 for RedoxFS. For btrfs, F-08 and the survey say only "checksums on data and metadata"; the placement is not in the record. | imprecision | Attribute parent-pointer storage to ZFS and RedoxFS only; list btrfs as "checksums all blocks". |
| F-0012-5 | d | INV-0012-4 vs rule 4 | INV-0012-4: memory "bounded by a fixed function of the device size", checked with "an allocation-counting allocator". The function is not given, so no test can decide the bound. Rule 4 states a different bound ("derived from validated fields and a fixed cap"). | imprecision | Give the bound (e.g. at most `C × tree depth × block size` plus a fixed cap, with C set in the format crate) and use the same wording in rule 4 and the invariant. |
| F-0012-6 | b | Option C, Evidence for | The cost argument against C is an `[inference]` ("far more CPU than a CRC"). The survey's ZFS section records that ZFS ships SHA-256, SHA-512, Skein, Edon-R and BLAKE3 as per-dataset options in production, the strongest evidence that cryptographic per-block checksums are deployable; it is not mentioned. | note | Add it to Evidence for; the recommendation can still prefer CRC32C on driver 5. |
| F-0012-7 | a | Context bullet 1 | Journaling filesystems "do not detect silent data corruption" (GR-014) is in quotation marks, but GR-014 reads "Journaling doesn't detect silent data corruption". | note | Quote exactly or drop the quotation marks. |

ADR-0012 otherwise checks out. GR-014 (1.53 million drives, more than 400,000 mismatches, "robust to a wide range of
disk faults", repair only with redundancy, memory not covered, APFS/device ECC as a weak source), GR-017, GR-021,
CP-15, CP-17, CP-26 match. F-05's CRC32c reasoning (64 KiB, "more than sufficient", "no provision for changing the
integrity checking mechanism", misdirected writes "to the wrong LUN"), F-07's "Fuzz Kiddiez" and "ongoing risk"
and its eight field transformations, F-15's SeaHash in `BlockAddr`, F-14's "does **not** provide ECC" are in the
sources. Four options, Linux baseline (A), non-Unix precedent (Venti, C). The crafted-image discipline (rule 4) is
concrete and its invariants (INV-0012-1, -2, -3, -5) are decidable.

## ADR-0013: Names and namespace structure

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0013-1 | other | Recommendation rule 8 (link following) vs INV-0002-4 | Link following "inside the same namespace-table entry, lexically …; a target that would leave the entry fails with `ENOENT` `[assumption]`". Accepted INV-0002-4 says path resolution in `libposix` never escapes an entry because "`..` at an entry's root resolves to the entry's root", i.e. it clamps. A link target `../../x` would thus clamp when typed as a path and fail when stored in a link. Absolute targets (`/usr/lib/…`), which Linux resolves from the root, are not addressed: under "the same entry" they fail although the namespace table could resolve them. | imprecision | Pick one rule for leaving an entry (clamp per INV-0002-4, or fail, stated as a deviation needing a superseding ADR) and say how absolute targets resolve (through the whole namespace table, or `ENOENT`). |
| F-0013-2 | b | Option D, Evidence against and Precedent | GR-003 is cited as a precedent for D ("object-namespace links followed with restrictions for sandboxed processes"), but its caveat is omitted: links low-privilege users could create "enabled a class of 'resource planting' attacks in which a sandboxed process redirects a privileged service". That is the main risk of a client-side follower in a native service that runs `libposix`-style resolution. | note | Add GR-003's caveat to Evidence against. |
| F-0013-3 | c | Rule 4; "Principles this recommendation strains" | Rule 4 fixes 255 bytes per component and 4,096 bytes per request path. DM-043, cited in Forces, concludes "avoid magic names and fixed buffer limits in a path ABI". The strain is not stated (the rationale "so `libposix` adds no new limit" is fine, but it is a tradeoff against DM-043). | note | List DM-043 under strained principles with the rationale. |
| F-0013-4 | a | Option B, Evidence for | "Users do not distinguish 'Readme' from 'README' (DM-094 Position B)": that sentence is DM-094's "Why it was chosen"; Position B is Overstreet/Krisman/Ts'o's "real user and application needs (Android, Samba)". "Wine" is in neither DM-094 nor the survey's bcachefs text (it is in F-19's `casefolding.rst`, line 12, which the survey does not quote). | note | Cite "Why it was chosen" for the first point; replace Wine by Android/Samba or quote F-19 in the survey. |

The conflict between rule 8 (`link`/`linkat` → `EPERM`) and ADR-0010's `linkat(AT_EMPTY_PATH)` → `publish` is
F-X-2; INV-0013-4 and unnamed objects are F-X-5. ADR-0013 otherwise checks out: DM-039 (14 runc CVEs, Plan 9
lexical `..`, Fuchsia no `..`, client canonicalisation "works only because it has no symlinks"), DM-040, DM-094
(*contested*, used as contested; HFS+ 3.2 vs APFS 9.0; CVE-2014-9390), DM-095 ("numerous security vulnerabilities";
POSIX.1-2024 `EILSEQ` for newline), DM-096, GR-015, CP-04, CP-22 match. F-32 (`NAME_MAX` 255, `PATH_MAX` 4,096),
F-38 (`EPERM` "does not support the creation of hard links") and F-37 (`_AS_LN_S_PREPARE`: `ln -s`, then `ln`,
then `cp -pR`) are verbatim. The measured link data (9 `symlinkat`, 7 `readlinkat` by toybox `rm`, no `link`) is
right. Four options, Linux baseline, Plan 9/Fuchsia precedent. Rule 1–3 and INV-0013-1 agree exactly on the
forbidden set.

## ADR-0014: Metadata

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0014-1 | c | "Why D" | "every attribute in D has a measured reader or writer; everything dropped (owners, atime, xattrs) has none." `requirements.md` §5: "Which `stat` fields … each program reads" cannot be answered ("M1/M2 record calls, not fields"); the ADR's own Context says inode numbers and link counts are unknown. `btime` and `ctime` have no measured reader or writer at all. Owners do have a reader in `requirements.md` §4: toybox `ls -l` "(mode, owner, size, mtime printed)" and dash `test -O/-G` (`geteuid`/`getegid` compared with the file owner, call sites `src/bltin/test.c:432,434`, `emulate`). The claim contradicts the evidence file. | error | Restate: writes are measured (owners never set, xattrs never called, times set by `utimensat`), reads of individual fields are not; keep D on driver 2 (no identity) and driver 3 (no atime), and justify `ctime`/`btime` separately or drop them. |
| F-0014-2 | d | Rule 3 and INV-0014-1 | "Every time the service assigns on one volume is strictly greater than every time it assigned before on that volume". Across a remount (or crash) this needs `last_assigned` to be persisted or recovered from the image; neither rule 3 nor ADR-0011 says so. If the wall clock is behind the newest stored `mtime` after a reboot, the next assigned time can be lower. INV-0014-1's check uses "random request sequences" without remounts. | imprecision | Persist `last_assigned` in the root record (or derive it at mount as the maximum assigned time in the tree) and add remounts and crash-remounts to INV-0014-1's check. |
| F-0014-3 | other | Rule 6 vs ADR-0002 rule 6 | Rule 6 has `libposix` return `EACCES` from `execve` and `access(X_OK)` when no execute bit is set. Read and write bits are not covered, although the measured programs probe them: toybox `rm` checks write permission to decide whether to prompt (`toys/posix/rm.c:39`), `rm -r` chmods 0700 an unreadable directory (`rm.c:57`), dash `test -r/-w` (`test.c:537`). ADR-0002 rule 6 says the personality "does not enforce" modes against identity; rule 6 is consistent with that only because no identity is involved, which is not said. | note | Specify `R_OK`/`W_OK` and `open` behaviour for mode bits (emulate for the fixed owner, or never refuse) and state why it is consistent with ADR-0002 rule 6. |
| F-0014-4 | a | Driver 4 | "archives carry arbitrary times that `touch`/`cpio` set explicitly (R-5)". R-5 is "Modification time changes on every write, is on the wall-clock time base, and has sub-second resolution"; the explicit-time data is `requirements.md` §4 ("`utimensat` 2 (T0) and 7 (T2), toybox `touch` and `cpio`"). | note | Cite `fs/requirements.md#4-metadata-read-and-written`. |
| F-0014-5 | d | INV-0014-2 | "read-only request sequences leave the image byte-identical". ADR-0011 rule 2 commits "when `T_COMMIT` expires" without saying that a commit with nothing dirty is skipped; a generation-only commit would rewrite a root record and fail the check without any attribute changing. | note | Add "a commit with no dirty state writes nothing" to ADR-0011 rule 2, or compare the tree instead of the image. |
| F-0014-6 | d | INV-0014-5 | "a compile-time size/field assertion, so adding one requires changing this ADR". A size assertion catches any layout change, not specifically an owner or access-time field; it is a layout freeze. | note | Call it a layout-freeze test, or check field names in the format crate's record definition. |

ADR-0014 otherwise checks out. The measured numbers (1,892 / 1,798 / 3,345 `newfstatat` in T2, 9 `utimensat`,
4,336 `umask`, 4 `fchmodat`) match the CSV/TSV. The make manual quotes (F-01: "older than any of the
prerequisites", "if the target file's last-modification time has changed since make first checked it",
`.LOW_RESOLUTION_TIME`) are verbatim. DM-006, DM-042 (relatime, lazytime, NTFS deferral, "cheap usage signal"),
DM-063, DM-003, DM-016, CP-20, NILFS2 without atime (F-12), FAT local time (F-13) match. Four options, Linux
baseline, Plan 9 precedent with its `[inference]` label. Rule 3 (strictly increasing assigned times) is the right
answer to make's ordering requirement and is testable, apart from F-0014-2.

## ADR-0015: Block device contract

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-0015-1 | a | Open questions, bullet 1; Context, "What QEMU does" | "QEMU in `cache=writethrough` or `cache=directsync` mode (which do not offer flush, F-21) … By rule 5 they mount read-only". F-21's own source contradicts this. `hw/block/virtio-blk.c`, `virtio_blk_get_features`: `features \|= s->host_features;` then `if (blk_enable_write_cache(s->blk) \|\| (s->conf.x_enable_wce_if_config_wce && virtio_has_feature(features, VIRTIO_BLK_F_CONFIG_WCE))) virtio_add_feature(&features, VIRTIO_BLK_F_WCE);`, with properties `config-wce` default `true` and `x-enable-wce-if-config-wce` default `true`. So with default device properties QEMU offers `VIRTIO_BLK_F_WCE` (= `FLUSH`) in every cache mode, and the guest can switch the cache mode through `CONFIG_WCE`. The Context's "whenever the drive's write cache is enabled" is the first half of that condition only. Rule 5's read-only fallback would not trigger on QEMU writethrough. | error | Correct the Context and the open question: QEMU offers flush in all cache modes unless `config-wce=off` (or an old machine type); state whether the driver should negotiate `CONFIG_WCE` and what it does with the `writeback` field. |
| F-0015-2 | a | Option A, Precedent | "ext4, XFS (journal commit blocks with checksums partly exist because the assumption is not always true, `fs/survey.md#ext4`)". The survey's ext4 section says only that a transaction "without a commit record, or with a bad checksum, is discarded at replay (F-03)"; it gives no reason why journal checksums exist. | note | Label the reason `[inference]`. |
| F-0015-3 | d | INV-0015-1 | "comparing the set of possible crash images with an independent brute-force model". Rule 2 allows "arbitrary bytes" per unstable sector, so the set is infinite; the test needs a finite representation. | imprecision | Model "arbitrary" as a distinguished symbolic value (or a fixed set of patterns) in both the test device and the brute-force model, and state that in the check. |

ADR-0015 otherwise checks out. Every quotation from the virtio specification (F-20) is verbatim: "serviced (probably
out of order)", "not necessarily in order", 512-byte units and `blk_size` "does not affect the units used in the
protocol", the four stable-write cases, "MUST ensure that stable writes are committed", the `FLUSH`-not-offered
note, the three status values and indeterminate multi-segment discard. The specification indeed says nothing about
atomicity (no occurrence of "atomic" in `description.tex`). QEMU has no FUA support in `virtio-blk.c` (no occurrence
of `FUA`). `cache.no-flush` "never needs to write any data to the disk" and `cache=writeback` as default are
verbatim. Rules 2–4 are a faithful and conservative reading (cases 1 and 2 of the specification are deliberately not
relied on, which driver 1 justifies). F-22 and F-24 are paraphrased correctly. Four options, Linux baseline,
littlefs flash model as the non-Unix option.

## `requirements.md` (Part 1)

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-REQ-1 | a | §2.4, bullet 1 | "every GNU make deletes the target its running recipe was writing: 4 partial targets per run, in 36 of 36 runs". `task-02b/conflict-impact.md`: "35 runs deleted 4 partial targets; one SIGTERM run deleted 3"; `task-02b/research-log.md` item 3 records "'4 deletions in 36/36' … were false". Also only the 4 leaf makes delete targets, not every make. | imprecision | "each leaf make deleted its partial target; 4 per run in 35 runs, 3 in one SIGTERM run; none left behind in 36/36". |
| F-REQ-2 | a | §2.1 | "The M3 extractor searched every compiled call site of every measured program for `fsync\|fdatasync\|sync\|syncfs` … no measured program has such a call in its compiled code." `callsites.py` covers dash, oksh, mrsh, tcc, gmake, pdpmake and the minimal T2 builds of toybox and busybox; clang and ld.lld are not in `PROGS`, and the full toybox/busybox binaries import `fsync`, `fdatasync`, `sync` (`libc-surface.csv`, `unused-applets`). | imprecision | Scope the sentence to the M3 program set and builds. The conclusion "never asks for durability" still stands on the syscall data. |
| F-REQ-3 | a | §3.2, "Call sites that depend on symlink semantics" | "DM-039 rows, 20 sites in core programs, 16 compiled". `callsites.csv` has 20 `6 DM-039` rows in total: 16 in core programs (toybox 14, gmake 2) and 4 in oksh (comparison). Every row is a compiled site by M3's definition. 3 of the 16 core rows are duplicate `lchown` rows ("same call as previous row"). | imprecision | "20 DM-039 rows, 16 in core programs (13 distinct sites), 4 in oksh". |
| F-REQ-4 | other | §1.2 table, "Limit that matters here" | The limit paragraph says the totals include `built` programs but not that `read`, `write`, `writev`, `readv`, `lseek` count calls on all fds (pipes, terminals, `/dev/null`), not file I/O. ADR-0010 Option D, ADR-0011 Context and ADR-0012 Context draw file-I/O conclusions from these columns (F-0010-7, F-0011-1, F-0012-1). | imprecision | Add the limit: the CSV does not separate file fds from pipes and terminals. |
| F-REQ-5 | a | §2.4, bullet 1, and R-3 | "The `unlinkat` happens while the recipe process may still hold the file open". `task-02b/event-timelines.md` shows the opposite order in W9a: the 4 `mktab` processes die at +0.1…+0.5 ms and the leaf makes delete `t4.h` at +0.5 ms; in W9b each leaf make kills its `mktab` (+2.4…+2.8) before deleting (+2.6…+2.9). The FIFO case does show unlink while open: make@top unlinks `/tmp/GMfifo<pid>` (+0.5 ms W9a, +0.3 ms W9b) while sub-makes still hold it. | note | Base R-3 on the FIFO case; say the partial-target data does not show an open holder at unlink time. |
| F-REQ-6 | c | §4.1, "Required ordering behaviour" | The inference argues from build-step spacing (12 of 32 tables in 1.5 s; chains `mktab` → `tN.h` → `mN.o`). In those chains the target is written *after* its prerequisite, where equal times are harmless (make treats equal as up to date, as the same paragraph says). The hazard the paragraph names, a prerequisite rewritten after its target within one clock tick, is not what the timing data shows. A data-backed argument exists: make's interrupt rule needs a rewritten target's time to *change* (F-01), which a coarse clock does not guarantee within one tick. | note | Rest the sub-second requirement on the manual's statement and the interrupt rule; keep the timing data as context. |
| F-REQ-7 | a | §2.2, "T3 does it too" | "`renameat` 1 each in W6; Q-P". `strace-patterns.tsv` has no `renameat` rows at all (the syscall CSV's `notable_args` is empty for it); the counts are `W6-compile:1;W6-link:1` from `syscall-surface.csv`, and attributing them to clang and ld.lld is by workload. | note | Cite the syscall CSV and say the attribution is by workload. |
| F-REQ-8 | a | §1.3 last paragraph, §2.6 | "TASK-02b adds `fcntl(F_SETFL, O_WRONLY\|O_APPEND)` 12 times …" and "Under parallel make, GNU make sets `O_APPEND` on some fd". The delta CSV gives the identical 12 / 6 / 2 `F_SETFL` patterns for `-j2` and `-j4`, pipe and FIFO style, and the TASK-02 serial core runs already show gmake `F_SETFL` 10 (T1) and 15 (T2) in `strace-patterns.tsv` (flags not recorded). Nothing shows this is parallel-specific. | note | "GNU make sets `O_APPEND` with `F_SETFL` (12 in each W2 `-j` variant; serial runs also call `F_SETFL`, flags not recorded)". |
| F-REQ-9 | a | §1.1 rows `opendir…` and `getcwd`/`chdir` | "Called by … busybox" for these rows: the only busybox dynamic entry is `busybox[W5-busybox]`, the busybox-sh comparison run, not a T0–T2 core program. | note | Drop busybox from these two "Called by" cells or mark it comparison. |
| F-REQ-10 | other | §4 table, toybox row | "Known purpose" lists "`ls -l` (mode, owner, size, mtime printed)". The data shows `toybox:ls` ran (W4, W5), not that `-l` was used; the column mixes what the applet can do with what was measured. ADR-0014 depends on which fields are read (F-0014-1). | note | Say "`ls` (with `-l` it prints …; flags not recorded)". |

Everything else recomputes exactly: all 22 rows of the §1.2 table (T0/T1/T2/T3 and program lists), the 20 syscalls
"absent from every run", every §1.1 tier/importer/caller cell other than F-REQ-9, the 26 functions listed under §1.1
with their tiers, every count in §1.3 (tcc 3,998/24,796; gmake 10/386 and 97/141; dash 1/291, 1/113, 7/892, 13/380;
`O_EXCL` gmake 2, clang 1, ld.lld 2; toybox `O_EXCL|O_NOFOLLOW` 1, `O_RDONLY|O_CREAT` 2/2; the `/tmp` prefix
counts), "every `mmap` in T0–T2 core runs is `MAP_PRIVATE|MAP_ANONYMOUS`" and the T3 file mappings (clang 18,
ld.lld 14, 7, 2), §2.2 (`renameat` 1/66/24; tcc `unlinkat` 55/258), §3.2 (`symlinkat` 9 = toybox 6 + built 3;
`readlinkat` toybox 7, clang 22 with 12 under `/proc`), the §4 table (gmake 624/1,892 and `faccessat2` 80/201; dash
29/1,798 and 0/49; toybox 4/3,345 and `faccessat` 29; busybox 3/106; tcc 0/0), `umask` 4,336, `O_APPEND` 393,
`utimensat` 2+7, `getdents64` 999, `mknodat` once per FIFO build. The F-01 and F-37 quotations are verbatim. The
cited call sites (`cp.c:454`, `xwrap.c:476/484`, `ln.c:77/89`, `main.c:320`, `remake.c:1635/1655`, `file.c:965`,
pdpmake `make.c:484`, `find.c:715`, `libtcc.c:503`, `findprog-in.c` ×2) exist with the classes stated. Section 5
honestly lists what the data cannot answer.

## `survey.md` (Part 2)

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-SUR-1 | a | "Block devices", QEMU bullet | "The virtio-blk model offers the flush feature … whenever the drive's write cache is enabled …; `writethrough` and `directsync` disable the guest-visible write cache". In F-21's source the flush feature is also offered whenever `config-wce` (default on) and `x-enable-wce-if-config-wce` (default true) hold, i.e. in every cache mode by default; `cache=` only sets the default of the device's `write-cache`, which a guest that negotiated `CONFIG_WCE` can change. This feeds the error in F-0015-1. | imprecision | Quote the `virtio_blk_get_features` condition and the two property defaults. |
| F-SUR-2 | a | XFS, Integrity | "a larger checksum 'does not really provide any extra value over CRC32c'". F-05 reads "Adding support for larger integrity fields and/or algorithms does really provide any extra value over CRC32c" (an evident typo for "does not"). The quotation is silently corrected. | note | Quote with "[not]" or paraphrase without quotation marks. |
| F-SUR-3 | a | Comparison table (ZFS row) and WAFL | ZFS's mechanism is "CoW + synchronous request log" and WAFL is "the copy-on-write-plus-intent-log shape that ZFS's synchronous path also has", but the ZFS section never describes a log; F-10 only mentions "pool log devices" under `logbias`, unquoted. ADR-0011 Option D relies on it (F-0011-5). | note | Add the `logbias` text to the ZFS section, or label the ZIL `[outside evidence]`. |
| F-SUR-4 | other | Sources, F-36 | F-36 (ext4 directory format, casefolded and encrypted entries) is listed but cited nowhere in the text. | note | Cite it where ext4 casefolding is discussed, or remove it. |
| F-SUR-5 | other | F-35 and littlefs size | The F-35 command run on the littlefs clone gives 11,314 lines; the 7,700 counts only `lfs.c`, `lfs.h`, `lfs_util.*` (which the littlefs section says, but F-35 does not). The RedoxFS core figure likewise excludes directories the command includes. | note | Put the per-project file selection into F-35. |

The survey otherwise checks out. Of the 30 sources fetched (Method step 4), every quotation and paraphrase I checked
is in the source: ext4 (`data=journal` "removed in January 2028", `auto_da_alloc` text, `data=writeback` "security
exposure", "similar level of journaling as that of XFS and JFS", 5-second commit, `metadata_csum` since early 2012
with crc32c), XFS (LUN, 64 KiB, forensic motive at 1 PB, eight transformations, "Fuzz Kiddiez", Unicode confusables
quote), btrfs RAID56 "Unstable", ZFS (`sync` texts, Fletcher-4 default, "NOT a recommended practice", normalization
"stored unmodified", `utf8only`), F2FS (shadow copy, wandering tree, `fsync_mode`, `generic/342`), NILFS2, VFAT,
littlefs ("worked, but not well", "push the atomicity problem upwards", no ECC, 32 KiB), RedoxFS (`HEADER_RING = 256`,
`seahash::hash`), fossil, cwfs, bcachefs (present at v6.16/v6.17, absent at v6.18; `CONFIG_UNICODE` feature bit),
virtio, QEMU (no FUA; `cache.no-flush`), Linux block layer, `errseq_t`, dm-log-writes (worst-case attachment of
completed writes), CrashMonkey/Ace (8 btrfs, 2 F2FS, 1 FSCQ bug), `rename(2)`, `open(2)` (`O_SYNC`, `O_TMPFILE`),
`fsync(2)`, `link(2)`, `limits.h`, PostgreSQL `data_sync_retry`, autoconf `_AS_LN_S_PREPARE`, GNU make manual. The
line counts I recounted match (Method step 5). Every listed design is covered, the four `U-` items are clearly
separated and marked inside the survey, and the inferences are labelled.

## `implementation-outline.md`

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-OUT-1 | other | Crates table, `zoraya-fs-format` | The outline puts "directory entry and `Name` validation (ADR-0013)" in `zoraya-fs-format`; ADR-0013 Follow-up work and INV-0013-1 put "A `Name` type in the `abi` crate … alongside `RelativePath`". Two homes for one type. | note | Say the format crate uses the `abi` `Name` type (or why it has its own). |
| F-OUT-2 | other | Test strategy | The test families name the invariants they check, but INV-0013-3 (service never follows a link object), INV-0013-5 (request decoder rejects `.`, `..`, absolute paths), INV-0014-1 (strictly increasing times) and INV-0015-3 (no read-write mount without flush) appear in none of them. | note | Map them (unit tests for INV-0013-5, INV-0015-3; model tests with a scripted clock for INV-0014-1; service tests for INV-0013-3). |

The outline otherwise checks out: every item is marked as depending on acceptance, crate names use the `zoraya-`
prefix, the host tools are `std` per CLAUDE.md, the size total (10,200–17,300 from the rows) matches "about
10,000–17,000", the XFS fuzz transformations are reproduced correctly, `zoraya-dtb`'s 30,000 mutations exist
(`crates/zoraya-dtb/tests/malformed.rs:474`), QEMU goes through `tools/run-qemu.sh`, crash tests avoid
`cache=unsafe`, and the bounded crash enumeration is listed as a known limitation.

## Cross-ADR consistency (with each other and with ADR-0002 to ADR-0006)

| ID | Class | Location | Finding | Severity | Suggested fix |
|---|---|---|---|---|---|
| F-X-1 | other | ADR-0010 "Personality mapping" (and ADR-0014 rule 5) vs accepted ADR-0005 INV-0005-1 | ADR-0010 maps "`fsync`, `fdatasync` and `sync`/`syncfs` → `sync`". In `task-02/libc-surface.csv` these four are `unused-applets` rows, so they are not in ADR-0005's tier list, and INV-0005-1 says every symbol `libposix` exports is in that list "or in an accepted superseding ADR"; ADR-0005: "The surface changes only by a superseding ADR that adds rows with a measured workload". ADR-0010 neither supersedes ADR-0005 nor has a measured workload (requirements §2.1: none calls them). Same for `chown` in ADR-0014 rule 5 (`unused-applets`). As written, implementing the mapping fails INV-0005-1. | error | State that the mapping specifies behaviour for when an ADR adds these rows to the surface, and that until then `libposix` does not export them; or propose the rows explicitly with ADR-0005's procedure. |
| F-X-2 | other | ADR-0010 "Personality mapping" vs ADR-0013 rule 8 | ADR-0010: "`O_TMPFILE` → unnamed create, `linkat(AT_EMPTY_PATH)` → `publish`". ADR-0013 rule 8: "`link`/`linkat` fail with `EPERM`", without exception. The two rules give different results for `linkat(fd, "", dirfd, name, AT_EMPTY_PATH)` on an `O_TMPFILE` fd. | error | In ADR-0013 rule 8: "`link`, and `linkat` other than `AT_EMPTY_PATH` on an unpublished unnamed object, fail with `EPERM`". |
| F-X-3 | other | ADR-0010 rule 6, ADR-0012 rule 3, ADR-0015 rule 6 | Two failure states with different client contracts. A device write or flush error → `Failed`: every later mutating request and `sync` returns `StorageFailed`, which is how clients learn that completed work was lost (INV-0010-3). A metadata checksum mismatch, or (ADR-0015 rule 6) any metadata read `IoError`, → "read-only (no further commits)" with the same `FAILED` signal, but ADR-0012 does not say what later mutating requests and `sync` return, nor that the completed-but-uncommitted requests of other clients are lost. INV-0012-5 checks only "no further writes". A single transient read error can thus drop up to `T_COMMIT` of other clients' work with no error to them. | imprecision | One failure state for all four causes, with ADR-0010 rule 6's client-visible errors (`sync` never succeeds again); extend INV-0010-3 to inject metadata corruption and read errors. |
| F-X-4 | other | ADR-0010 rule 6 vs ADR-0006 rule 1 | "the volume object asserts a `FAILED` signal (ADR-0006 rule 1)". ADR-0006 rule 1 defines signal enums per *kernel* object type (byte streams, process, job, timer, channel, event `USER_0..7`); the volume is an object of a user-space service and no `FAILED` signal exists. | note | Say how: e.g. the service hands supervisors an event object and asserts a `USER_n` bit, or defer to the channel/message ADRs. |
| F-X-5 | other | ADR-0013 rule 5 and INV-0013-4 vs ADR-0010 rule 7, R-3, ADR-0011 rule 7 | ADR-0013: "Every object except the volume root has exactly one name"; INV-0013-4: "the namespace stays a tree with one name per object". Unnamed objects (ADR-0010 rule 7) and files unlinked while open (R-3, ADR-0006's FIFO "`unlink` removes the name while open handles stay valid") have zero names, so the invariant fails by design. ADR-0011 rule 7 puts only never-published unnamed objects on the orphan list; an object whose last name is removed while handles are open is not covered, so after a crash it either leaks or must have been freed at unlink, breaking R-3. | imprecision | "at most one name"; put objects whose last name was removed while open on the orphan list; extend INV-0010-6 to them. |
| F-X-6 | other | ADR-0013/0014 vs ADR-0002 rule 1, ADR-0006 (FIFO jobserver), requirements R-10 | ADR-0002 rule 1 and ADR-0006 require a temporary directory service that "implements FIFO nodes" created by `mkfifo`/`mknodat` in `/tmp`, and R-10 lists it. ADR-0014's `kind` is `File`, `Directory`, `Link` only, ADR-0010 rule 2's atomic requests have no FIFO creation, and none of the six ADRs says whether `/tmp` is a volume of this filesystem or a separate service. If separate, it is not said which ADR-0010 guarantees hold there (gmake's `mkstemp` with `O_EXCL` in `/tmp`, R-2). | imprecision | State whether the temporary directory service is an instance of this filesystem; if yes, add a FIFO node kind; if no, say R-2 and R-3 are that service's obligations. |
| F-X-7 | other | ADR-0010 rule 5 vs ADR-0011 rule 2 and INV-0010-4 | Rule 5: "every completed request becomes durable within `T_COMMIT` after it completes". ADR-0011 rule 2 starts a commit "when `T_COMMIT` expires", and a commit then takes two flushes; INV-0010-4 tests "idle for `T_COMMIT` plus one commit". The rule promises more than the mechanism and the invariant deliver. | imprecision | "within `T_COMMIT` plus the duration of one commit", and say in ADR-0011 rule 2 from which instant `T_COMMIT` is measured. |
| F-X-8 | other | ADR-0011 rule 4 / ADR-0012 rule 3 vs ADR-0010 rule 3 and ADR-0011 rule 5 | Mount "picks the valid one with the highest generation" and "ignores root records that fail validation". If the newest *durable* root later fails validation (media corruption, not a torn write), mount silently selects generation g−1: requests acknowledged by a `sync` in commit g are lost, against ADR-0010 rule 3 ("Any request that completed before a `sync` that returned is in the prefix"); and blocks freed by commit g may already have been reused (rule 5 allows it once g is durable), so g−1's tree can be damaged. INV-0011-1 protects only "the durable root and every root written after it", although with `N_ROOT` = 4 older slots remain selectable. | imprecision | Distinguish a torn slot (only the slot being written at the crash) from corruption of an older-than-newest slot; on the latter, mount read-only and report instead of falling back silently; add a case to INV-0011-5. |
| F-X-9 | other | `Depends on` lines | ADR-0011 depends on ADR-0012 and ADR-0012 on ADR-0011 (a cycle). ADR-0015 lists only ADR-0001, although its rules 6 and 8 define behaviour by reference to ADR-0010 rule 6 and ADR-0012 rules 3–4. | note | Break the cycle (0012 depends on 0011, not the reverse, or both on 0010 only) and list 0010 and 0012 under ADR-0015, or say the references are informative. |

Also noted, without a separate row: ADR-0011 calls the workload "write-heavy" and ADR-0012 calls it read-dominated
from the same table (F-0011-1, F-0012-1); the link-target rule versus INV-0002-4 is F-0013-1.

Checked and consistent: the commit protocol of ADR-0011 rule 3 (blocks complete → flush → root record → flush →
reply) satisfies ADR-0015 rules 3–4 and INV-0011-3; deferred reuse (ADR-0011 rule 5) waits for the same flush;
ADR-0010 rule 4's `sync` and ADR-0011 rule 2's commit trigger agree; the strictly increasing times of ADR-0014 rule 3
give make's ordering (R-5) and its interrupt rule; link objects (ADR-0013 rule 7) appear as `kind = Link` with
`st_nlink = 1` in ADR-0014 (rule 1 also updates `mtime` on a "link target" change, an operation ADR-0013 does not
define, harmlessly); unnamed objects of ADR-0010 rule 7 have an orphan list in ADR-0011 rule 7 and INV-0010-6;
ADR-0002: handles plus `RelativePath` (INV-0002-1), the `Name` type alongside it, no uid/gid with a fixed
per-session identity (rule 6, ADR-0014 rule 5), modes stored and reported but not enforced against identity; ADR-0005:
`MAP_SHARED` (R-8), record locks (ADR-0010 rule 8) excluded consistently; ADR-0006: regular files `NotWaitable`, the
`Completion` packet left reserved, nothing in the batch depends on it.

## Summary

### Counts

| Document | error | imprecision | note | total | by class (a / b / c / d / other) |
|---|---|---|---|---|---|
| ADR-0010 | 0 | 3 | 5 | 8 | 5 / 1 / 0 / 1 / 1 |
| ADR-0011 | 0 | 3 | 4 | 7 | 4 / 1 / 0 / 0 / 2 |
| ADR-0012 | 1 | 4 | 2 | 7 | 5 / 1 / 0 / 1 / 0 |
| ADR-0013 | 0 | 1 | 3 | 4 | 1 / 1 / 1 / 0 / 1 |
| ADR-0014 | 1 | 1 | 4 | 6 | 1 / 0 / 1 / 3 / 1 |
| ADR-0015 | 1 | 1 | 1 | 3 | 2 / 0 / 0 / 1 / 0 |
| requirements.md | 0 | 4 | 6 | 10 | 7 / 0 / 1 / 0 / 2 |
| survey.md | 0 | 1 | 4 | 5 | 3 / 0 / 0 / 0 / 2 |
| implementation-outline.md | 0 | 0 | 2 | 2 | 0 / 0 / 0 / 0 / 2 |
| cross-ADR | 2 | 5 | 2 | 9 | 0 / 0 / 0 / 0 / 9 |
| **all** | **5** | **23** | **33** | **61** | 28 / 4 / 3 / 6 / 20 |

### Judgement per document

- **ADR-0010.** Sound and the most important of the batch; the recommendation follows from the strongest datum in
  Part 1 (no measured program ever requests durability). The contract's personality mapping collides with ADR-0005's
  surface rule (F-X-1) and with ADR-0013 (F-X-2). The fossil precedent contradicts ADR-0011's own reading of soft
  updates (F-0010-2). INV-0010-1 needs its enumeration bound (F-0010-6).
- **ADR-0011.** The choice of copy-on-write follows from ADR-0010 and ADR-0015, and the commit protocol is correct
  against the device contract. The workload characterisation over-reads the syscall counts (F-0011-1, F-0011-2), and
  the task's "how each option would be tested" is answered only for B (F-0011-3). Root-slot fallback after
  corruption needs a rule (F-X-8).
- **ADR-0012.** The recommendation (CRC32C in parent pointers, total decoder, per-volume isolation) is well argued.
  One measured claim is the reverse of the data (F-0012-1); a `U-` item is used unlabelled (F-0012-2); one precedent
  and one btrfs attribution are unsupported (F-0012-3, F-0012-4); INV-0012-4 needs a number (F-0012-5).
- **ADR-0013.** Strong use of the data and the catalogue. The link-following rule must be reconciled with INV-0002-4
  and say how absolute targets resolve (F-0013-1); the `linkat` rule must allow `publish` (F-X-2).
- **ADR-0014.** Option D is reasonable, but its "Why D" claims measured readers that the data cannot show and
  ignores the measured owner readers (F-0014-1). Strictly increasing times must survive remounts (F-0014-2).
- **ADR-0015.** The virtio reading is exact. The QEMU statement about writethrough/directsync is contradicted by the
  cited QEMU source (F-0015-1), which matters for rule 5's read-only fallback.
- **requirements.md.** The numbers are reproducible and §5 is honest about gaps. Three statements repeat or extend
  earlier errors (W9 "4 in 36/36", F-REQ-1; M3 scope, F-REQ-2; DM-039 row counts, F-REQ-3), and the read/write
  columns need the "all fds" caveat because three ADRs lean on them (F-REQ-4).
- **survey.md.** Primary sources are real and the quotations check; `U-` items are cleanly separated. The QEMU flush
  condition is incomplete (F-SUR-1).

### Most important three

1. **F-0015-1 / F-SUR-1** (error): QEMU offers flush in every cache mode by default (`config-wce` on,
   `x-enable-wce-if-config-wce` true), contrary to ADR-0015's open question and the survey; rule 5's read-only
   fallback is built on the wrong premise.
2. **F-X-1** (error): ADR-0010 maps `fsync`/`fdatasync`/`sync`/`syncfs`, which are `unused-applets` rows outside
   ADR-0005's accepted surface; as written this fails accepted INV-0005-1. **F-X-2** (error) is the same kind of
   rule collision between ADR-0010 and ADR-0013 on `linkat(AT_EMPTY_PATH)`.
3. **F-0014-1 / F-0012-1** (errors): two recommendations argue from measurements the data contradicts ("every
   attribute in D has a measured reader"; "the toolchain reads far more than it writes" while W4 has more write than
   read calls).

## Disposition of findings (drafter)

Every finding was addressed in the documents after the review; none was declined. After the fixes, the `Evidence`
lines of the six ADRs were regenerated from their bodies by script, and `cargo run -q -p zoraya-adr-check -- docs`
reports only the six expected INV-0001-5 errors (on a scratch copy with index rows added it reports 0 errors).

| Finding | Disposition |
|---|---|
| F-0010-1, F-REQ-2 | Fixed: the M3 search is scoped to the programs and builds it covered; clang, lld and the full multi-call binaries rest on the syscall data alone. |
| F-0010-2 | Fixed: fossil is cited only for "no check after a crash" and identified as soft updates (consistency, not prefix order); removed from Option C's title and precedent as a prefix precedent. |
| F-0010-3 | Fixed: marked `[inference]` and pointed at the survey's inference paragraph. |
| F-0010-4 | Fixed: reduced to the Firefox 3.0 `fsync` problems cited by DM-038, mechanism labelled `[inference]`. |
| F-0010-5 | Fixed: TxOS "in under a month" and DM-045's Ceph remark added to Option E's evidence for; btrfs ioctls "deprecated in 2017, slated for removal". |
| F-0010-6 | Fixed: INV-0010-1 states the bound (all subsets up to `k` = 12 unstable writes, seeded samples beyond) and the finite torn-sector patterns. |
| F-0010-7 | Fixed: Option D says an unknown share of the `write` calls are to files. |
| F-0010-8 | Fixed: rule 6 says reads in `Failed` come from the in-memory state and may return data a remount discards. |
| F-0011-1, F-0012-1, F-REQ-4 | Fixed: `requirements.md` states that `read`/`write`/`readv`/`writev`/`lseek` count all fd kinds; ADR-0011 now rests on file creation and deletion counts (1,110 `unlinkat`, 1,231 `O_CREAT|O_TRUNC` opens) and drops "write-heavy"; ADR-0012 rests on tcc's read-only opens (3,998 / 24,796) and drops "reads far more than it writes". |
| F-0011-2 | Fixed: in-place modification now lists tcc's truncation, shell `>` and `>>`. |
| F-0011-3 | Fixed: a "Testing" line was added to every option (A–E). |
| F-0011-4 | Fixed: labelled `[inference]`, ADR-0015 citation dropped. |
| F-0011-5, F-SUR-3 | Fixed: the survey's ZFS section quotes F-10's `logbias` text ("pool log devices"), the ZIL description is labelled `[outside evidence]`, the comparison row and ADR-0011 Option D use the quoted wording. |
| F-0011-6 | Fixed: WAFL and Fxfs marked as search summaries and bcachefs as inference in the Context. |
| F-0011-7 | Fixed: "two logical outcomes per commit, whatever the device state". |
| F-0012-2 | Fixed: the syzbot, maintainer and auto-mount statements are labelled U-04, not fetched. |
| F-0012-3 | Fixed: the Fuchsia blob precedent was removed; Git labelled `[outside evidence]`. |
| F-0012-4 | Fixed: parent-pointer placement attributed to ZFS (GR-014) and RedoxFS only; btrfs listed as "checksums all blocks". |
| F-0012-5 | Fixed: rule 4 and INV-0012-4 use the same bound, `MAX_DEPTH × BLOCK_SIZE + CACHE_CAP` bytes of decoder state, with constants defined in the format crate. |
| F-0012-6 | Fixed: ZFS's production cryptographic checksum options added to Option C's evidence for. |
| F-0012-7 | Fixed: paraphrased without quotation marks. |
| F-0013-1 | Fixed: link targets resolve exactly like a program-supplied path: relative targets with lexical `..` clamped at the entry root (INV-0002-4, no deviation), absolute targets through the namespace table. |
| F-0013-2 | Fixed: GR-003's resource-planting caveat added to Option D's evidence against. |
| F-0013-3 | Fixed: DM-043 listed under principles strained, with the rationale. |
| F-0013-4 | Fixed: DM-094's "Why it was chosen" and Position B cited correctly; Wine replaced by Android and Samba, bcachefs cited for compatibility. |
| F-0014-1 | Fixed: "Why D" now says writes are measured and reads are not; owners' known readers are satisfied by the fixed identity; `ctime` is kept because `struct stat` needs it and it cannot be synthesised; the creation time (`btime`, no reader or writer) was dropped; driver 5 rewritten accordingly. |
| F-0014-2 | Fixed: `last_assigned` is persisted in every root record (ADR-0011 rule 3) and restored at mount; INV-0014-1 adds remounts and crash-remounts and a clock behind the newest stored time. |
| F-0014-3 | Fixed: `libposix` applies the owner permission bits for `access`, `open` and `execve`, as POSIX does for a non-root owner; consistency with ADR-0002 rule 6 explained (every file is owned by the caller's fixed identity, so nothing is compared). |
| F-0014-4 | Fixed: driver 4 cites `fs/requirements.md#4-metadata-read-and-written`. |
| F-0014-5 | Fixed: ADR-0011 rule 2 says a commit with no dirty state writes nothing; new INV-0011-6 checks it. |
| F-0014-6 | Fixed: INV-0014-5 compares the record's field-name table with rule 1's list. |
| F-0015-1, F-SUR-1 | Fixed: the survey quotes `virtio_blk_get_features` and both property defaults; ADR-0015's Context says QEMU offers flush in every cache mode by default; rule 5 notes it matters for other devices or `config-wce=off`, and says the driver does not negotiate `CONFIG_WCE` in the first release; the open question now asks whether to use `CONFIG_WCE` for writethrough. |
| F-0015-2 | Fixed: reason labelled `[inference]`. |
| F-0015-3 | Fixed: INV-0015-1 represents "arbitrary" by a fixed set of patterns in both the device and the model. |
| F-REQ-1 | Fixed: 4 targets in 35 runs and 3 in one SIGTERM run, none left behind in 36/36, deleted by leaf makes; citing TASK-02b's correction. |
| F-REQ-3 | Fixed: 20 DM-039 rows, 16 in core programs (13 distinct sites), 4 in oksh. |
| F-REQ-5 | Fixed: R-3's unlink-while-open now rests on the FIFO case (sub-makes hold it open); the partial-target data is said not to show an open holder. |
| F-REQ-6 | Fixed: the sub-second requirement rests on the manual's rebuild and interrupt rules; the W8/W9 pace is kept as context, and the paragraph says the chains do not show either hazard. |
| F-REQ-7 | Fixed: T3 `renameat` cited from the syscall CSV, attributed by workload. |
| F-REQ-8 | Fixed: `O_APPEND` via `F_SETFL` is not claimed to be parallel-specific; serial `F_SETFL` calls noted. |
| F-REQ-9 | Fixed: busybox marked as comparison-run only in the two rows; pdpmake marked as comparison. |
| F-REQ-10 | Fixed: `ls` with a note that `-l`'s fields are printed only with that flag and the flags were not recorded. |
| F-SUR-2 | Fixed: quoted as "does [not] really provide", with a note on the source's typo. |
| F-SUR-4 | Fixed: F-36 cited in the names table, together with F-02's text on ext4 casefolding (Unicode version in the superblock, hashing of the casefolded name). |
| F-SUR-5 | Fixed: F-35 now states the per-project file selection (littlefs core files; RedoxFS core and total; OpenZFS core and total; Plan 9 directories). |
| F-OUT-1 | Fixed: the format crate uses the `abi` crate's `Name` type. |
| F-OUT-2 | Fixed: INV-0013-3, INV-0013-5, INV-0014-1, INV-0015-3 and INV-0011-6 are mapped to test families. |
| F-X-1 | Fixed: ADR-0010 says `fsync`/`fdatasync`/`sync`/`syncfs` are outside ADR-0005's surface and not exported; the mapping applies only if a superseding ADR adds the rows. ADR-0014 rule 5 does the same for `chown` (and notes which owner functions are tier rows). |
| F-X-2 | Fixed: ADR-0013 rule 8 exempts `linkat(fd, "", dir, name, AT_EMPTY_PATH)` on an unpublished unnamed object, which is `publish`. |
| F-X-3 | Fixed: one `Failed` state for write, flush and metadata-read errors and metadata validation failures, with ADR-0010 rule 6's replies; ADR-0012 rule 3 and INV-0012-5 refer to it; INV-0010-3 injects metadata corruption and read errors. |
| F-X-4 | Fixed: the `FAILED` "signal" is replaced by the service reporting `Failed` to its supervisor, mechanism `[assumption]` pending the channel/message ADRs, with a note that ADR-0006 rule 1 covers kernel objects only. |
| F-X-5 | Fixed: "at most one name" in ADR-0013 rule 5 and INV-0013-4; ADR-0011 rule 7's orphan list covers objects whose last name was removed while open; INV-0010-6 tests both kinds. |
| F-X-6 | Fixed: ADR-0010 states the temporary directory service is out of these ADRs' crash scope but must provide exclusive create and unlink-while-open (open question 5 asks whether it is an instance of this filesystem); ADR-0014 rule 11 says FIFO nodes are not stored here. |
| F-X-7 | Fixed: ADR-0010 rule 5 says "within `T_COMMIT` plus the duration of one commit"; ADR-0011 rule 2 measures `T_COMMIT` from the completion of the oldest uncommitted request. |
| F-X-8 | Fixed: ADR-0011 writes each root record to two slots in the same step, selects the highest generation with one valid copy, never selects an older generation, mounts read-only if an older pair is invalid while a newer one is valid, and states the residual double-fault risk; INV-0011-1 and INV-0011-5 updated. |
| F-X-9 | Fixed: ADR-0011 no longer depends on ADR-0012 (cycle broken); ADR-0015's references to ADR-0010 and ADR-0012 are declared informative on its Depends-on line. |
