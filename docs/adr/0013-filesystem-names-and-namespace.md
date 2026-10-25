# ADR-0013: File names and namespace structure

- **Status:** proposed
- **Date:** 2026-10-25
- **Depends on:** ADR-0002, ADR-0005, ADR-0010
- **Evidence:** CP-01, CP-03, CP-04, CP-22, CP-27, X-8, DM-039, DM-040, DM-043, DM-094, DM-095, DM-096, GR-003, GR-004, GR-015, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, fs/requirements.md#3-links, fs/requirements.md#31-hard-links, fs/requirements.md#32-symbolic-links, fs/requirements.md#5-questions-the-data-cannot-answer, fs/requirements.md#6-requirements-derived-for-the-adrs, fs/survey.md#bcachefs, fs/survey.md#names, fs/survey.md#sources, fs/survey.md#zfs, task-02/callsite-analysis.md#feature-6-other-task-01-design-mistakes-in-the-data, task-02/conflict-impact.md#x-8-strict-names-vs-posix-byte-names, task-02/syscall-surface.csv

## Context

This ADR decides how the filesystem names objects: the encoding of a name, when two names are equal, what is
forbidden, the limits, whether symbolic links and hard links exist natively, and how `..` behaves. It also states
what the POSIX personality must emulate. ADR-0002 already fixed the frame: native calls name objects by handle plus a
relative path (`RelativePath` rejects a leading `/`, INV-0002-1); `..` is handled lexically in `libposix` and never
escapes a namespace-table entry (INV-0002-4); "names are UTF-8 at the native boundary (CP-22)" and the mapping of
POSIX byte names that are not valid UTF-8 was left open (X-8).

**What is measured** (`fs/requirements.md`):

- No workload exercised a name that is not valid UTF-8, contains control characters or differs only in case
  (`task-02/conflict-impact.md#x-8-strict-names-vs-posix-byte-names`); the longest name and path were not recorded
  (`fs/requirements.md#5-questions-the-data-cannot-answer`).
- **Hard links are never created** in any tier: `link`/`linkat` are imported but never called, and no `link` syscall
  appears in any run (`fs/requirements.md#31-hard-links`; `task-02/syscall-surface.csv`).
- **Symbolic links are created only in T2**, 9 times in W4-configure, by toybox `ln -s` and configure test programs;
  autoconf scripts create them only to test whether `ln -s` works, and fall back to `ln` and then `cp -pR`
  (`fs/requirements.md#32-symbolic-links`). Symlinks are *read* by toybox `rm` (7 times) and by clang (`/proc/self/exe`,
  T3). Every symlink-*creating* call site was classed `emulate` "only if the native FS lacks symlinks", every
  *following* site `direct` or `restructure`; none needs native symlinks to work
  (`task-02/callsite-analysis.md#feature-6-other-task-01-design-mistakes-in-the-data`).
- The measurement setup itself used symlinks for every `PATH` set, which the launcher's namespace table replaces
  (`fs/requirements.md#32-symbolic-links`; ADR-0002 rule 5).

**Forces from TASK-01 and the survey.** Byte-string names allow control characters and option injection (DM-095);
case folding and normalisation made name equality depend on Unicode tables and vendor quirks and enabled
CVE-2014-9390 (DM-094, *contested*); UTF-16 names need not be valid Unicode (DM-096); UTF-8 held up (GR-015); fixed
limits and magic names leak into applications (DM-043). Path-based resolution with symlinks and `..` is the TOCTOU
class (DM-039); hard links let a user create a name for an object they cannot access (DM-040). Plan 9 has no symlinks
and lexical `..`; Fuchsia has no `..` on file servers (DM-039, GR-004). CP-04 asks for handle-relative resolution and
name-equality rules fixed once; CP-22 for validated UTF-8. The survey's name table shows the encoding, equality and
forbidden-set choices are separable (`fs/survey.md#names`).

## Decision drivers

1. **Name equality is part of the storage ABI and the security boundary; decide it once** (DM-094, CP-04).
2. **One native string encoding, validated at the boundary** (CP-22, DM-096, GR-015).
3. **No bytes that break tools or terminals** (DM-095).
4. **No new ambient path authority**: resolution stays handle-relative and confined (CP-01, CP-04, DM-039, ADR-0002).
5. **Creating a name for an object requires authority over the object** (CP-04, DM-040).
6. **The measured toolchain must keep working through `libposix`** with what it was measured to use
   (`fs/requirements.md#6-requirements-derived-for-the-adrs`, R-7; ADR-0005).
7. **Round-tripping foreign names (archives, media) has a cost that must be stated** (DM-095, X-8).

## Considered options

### Option A: POSIX names and links (Linux baseline)

- **Description:** A name is any byte string without `/` or NUL, up to 255 bytes; equality is byte equality; the
  filesystem stores and follows symbolic links during resolution; hard links create additional names; `..` is a
  directory entry the server resolves.
- **Evidence for:** Never rejects a name another system produced (DM-095), so archives always round-trip; no Unicode
  tables (DM-094 Position A: anything but a fixed table is unjustified); zero emulation in `libposix`; symlink-based
  layouts (multi-call binaries, `/bin` sets) work as on Linux (`fs/requirements.md#32-symbolic-links`).
- **Evidence against:** DM-095 (newline, escape, leading `-`; "numerous security vulnerabilities"); DM-039 (symlink
  races, 14 runc path CVEs); DM-040 (hard links to inaccessible files). Contradicts CP-04 and CP-22.
- **Cost:** *(estimate)* Lowest implementation cost in the filesystem; the cost moves to every program that handles
  names.
- **Precedent:** Linux, BSDs (with `openat2` and `protected_*` retrofits, DM-039, DM-040).

### Option B: Unicode names with folding and normalisation (Windows, macOS; non-Unix precedent NTFS)

- **Description:** Names are Unicode; equality ignores case and/or normalisation form; additional aliases (8.3 names)
  may exist.
- **Evidence for:** Users do not distinguish "Readme" from "README" (DM-094 Position B); interoperability with
  Windows/macOS media and software (Wine, Samba) is why Linux added casefolding (`fs/survey.md#bcachefs`, DM-094).
  ZFS shows normalisation can be done on comparison while storing names unmodified (`fs/survey.md#zfs`).
- **Evidence against:** Equality depends on a Unicode version and vendor tables (HFS+ 3.2 vs APFS 9.0, DM-094);
  CVE-2014-9390; per-directory feature bits make images unmountable without Unicode support (`fs/survey.md#bcachefs`).
  Every layer (shell globbing, `make`'s file cache, archive tools) must agree with the filesystem's equality, and the
  measured programs compare names as bytes [inference: they are POSIX programs].
- **Cost:** *(estimate)* Unicode case-folding and normalisation tables in the filesystem service, versioned forever.
- **Precedent:** NTFS, HFS+/APFS, ext4/F2FS/bcachefs casefold directories.

### Option C: Strict names, no links, no `..` in the server (Plan 9 / Fuchsia)

- **Description:** Names are validated UTF-8 without control characters, compared byte for byte; the server has no
  symbolic links, no hard links and no `..`; a directory handle confines its holder to its subtree.
- **Evidence for:** Removes DM-039's and DM-040's classes from the server entirely; Fuchsia's directory handles
  confine by construction (DM-039); Plan 9 ran a whole system with lexical `..` and bind instead of links (DM-039,
  GR-004). Matches CP-04 and CP-22 literally. The data shows the toolchain never creates a hard link and creates
  symlinks only to probe for them, with a fallback (`fs/requirements.md#3-links`).
- **Evidence against:** Archives and source trees that contain symlinks (tar, cpio, git checkouts) cannot be
  extracted faithfully; toybox's `cp -s`, `cpio` and `tar -x` symlink sites become errors (`emulate` sites,
  `task-02/callsite-analysis.md#feature-6-other-task-01-design-mistakes-in-the-data`). Fuchsia's client-side
  canonicalisation "works only because it has no symlinks" (DM-039).
- **Cost:** *(estimate)* Smallest server; `libposix` returns errors for `symlink` and `link`.
- **Precedent:** Plan 9 (research; ideas spread), Fuchsia (shipping on Nest Hub, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).

### Option D: Option C plus inert symbolic-link objects that only clients interpret

- **Description:** As C, but the filesystem can store a *link object*: a node whose content is a target string. The
  server never follows it: resolving a path through a link object returns "this component is a link with target T"
  to the client. `libposix` decides whether to follow, resolving T relative to the link's directory within the same
  namespace-table entry (lexical `..`, INV-0002-4), with a hop limit.
- **Evidence for:** Archives with symlinks round-trip (driver 7); POSIX programs see symlinks (`ln -s` succeeds,
  `readlink` works, configure's probe answers "yes"); and none of DM-039's server-side race exists, because the server
  never resolves a link and no privileged deputy resolves paths for others (no set-uid, CP-03). Following stays
  confined to what the client's handles already reach (driver 4).
- **Evidence against:** A second resolver (in `libposix`) whose behaviour must match POSIX's for the cases programs
  use; a client that follows links still has DM-039's check-then-use race *within its own subtree*. The measured
  workload needs none of it (R-7), so it is scope (CP-27) bought for archives.
- **Cost:** *(estimate)* Server: one more node kind, a few hundred lines. `libposix`: link following in its resolver,
  bounded by a hop limit.
- **Precedent:** Windows NT's object-namespace links followed with restrictions for sandboxed processes (GR-003),
  `openat2` `RESOLVE_*` flags (DM-039) as partial analogues `[inference: no system in the record stores links that the
  server never follows]`.

## Recommendation

**Recommendation (not a decision): Option C for encoding, equality, hard links and `..`; Option D's inert link
objects for symbolic links.**

1. **Encoding.** A name component is a well-formed UTF-8 string (no overlong forms, no surrogate code points, no
   code points above U+10FFFF). Validation needs no Unicode tables and no Unicode version.
2. **Forbidden.** The empty name, `.`, `..`, `/`, and every control character: U+0000–U+001F, U+007F and
   U+0080–U+009F (C1 controls, which include the 8-bit terminal control sequence introducer).
3. **Equality.** Byte-for-byte. No case folding, no normalisation. Two names that look alike are different names;
   detecting confusables is a user-interface concern, as XFS's checker treats it (`fs/survey.md#names`).
4. **Limits.** At most 255 bytes per component (Linux's `NAME_MAX`, so `libposix` adds no new limit,
   `fs/survey.md#names`). A request's relative path is at most 4,096 bytes `[assumption: Linux PATH_MAX; bounds the
   service's work per request]`. No limit on depth beyond that.
5. **Tree.** Every object except the volume root has exactly one name (no hard links). Directories form a tree; a
   rename that would move a directory into its own subtree is refused. Creating a second name for an object is not
   possible, which satisfies driver 5 trivially.
6. **No `..` in the server.** The server rejects `.` and `..` components (rule 2). A directory handle confines its
   holder to the subtree below it (ADR-0002, INV-0002-4). The server may keep parent references internally (to
   enforce rule 5) but never exposes them as a name.
7. **Link objects.** A link object stores a target string (UTF-8, at most 4,096 bytes, any content including `/` and
   `..`). Opening a path whose component is a link object without asking for the link itself fails with `IsLink`
   carrying the remaining path and the target; the server never follows. Creating, reading, renaming and removing a
   link object are ordinary atomic requests (ADR-0010).
8. **Personality (`libposix`).**
   - A POSIX name that is not valid UTF-8 or contains a forbidden character fails with `EILSEQ` (POSIX.1-2024
     already encourages `EILSEQ` for newline in new names, DM-095). This answers ADR-0002's open question (X-8) by
     rejection, not mapping; revisit trigger below.
   - `symlink` creates a link object; `readlink` reads it; path resolution follows link objects inside the same
     namespace-table entry, lexically, at most 40 hops `[assumption: Linux's limit]`, and fails with `ELOOP` beyond;
     a target that would leave the entry fails with `ENOENT` `[assumption]`. `O_NOFOLLOW` and `lstat`
     return the link itself.
   - `link`/`linkat` fail with `EPERM`, which is what Linux returns when "the filesystem ... does not support the
     creation of hard links" (`fs/survey.md#sources`, F-38). Autoconf's fallback then uses `cp -pR`
     (`fs/requirements.md#32-symbolic-links`).
   - `rename` of a directory into its own subtree → `EINVAL`.
   - `..` is handled lexically by `libposix` (ADR-0002 rule 5).

**Why this split.** For encoding, equality and the forbidden set, drivers 1–3 decide, and the measured workload never
needed anything else; Option B's cost is permanent Unicode tables for a benefit (Windows/macOS interoperability) no
scope document asks for. For hard links, the data shows no use at all and DM-040 shows the risk. For symbolic links,
the data alone would support Option C, but archive round-tripping (driver 7) is cheap to keep with inert objects, and
inert objects keep the server out of DM-039 entirely; the architect may prefer pure C, which removes one node kind and
the `libposix` follower.

**Principles this recommendation strains.** CP-27: symlink following in `libposix` is not required by any measured
workload. X-8 is resolved toward rejection, at the cost DM-095 records (archives with non-UTF-8 or control-character
names cannot be extracted).

## Consequences

- **Positive.** Name equality is a byte comparison, fixed forever, with no Unicode dependency. Names never contain
  bytes that break terminals or newline-separated tools. The server has no path-race class. `make`, shells and tcc see
  ordinary POSIX names.
- **Negative.** Archives with invalid UTF-8, control characters or hard links cannot be extracted faithfully (`tar`
  and `cpio` hard-link entries become copies or errors). Case-insensitive interoperability (FAT media, Windows shares)
  would need a separate service. `libposix` carries a link follower.
- **Follow-up work.** A `Name` type in the `abi` crate that enforces rules 1, 2 and 4 in its constructor (alongside
  `RelativePath`, INV-0002-1); the `IsLink` reply in the IPC protocol; the `libposix` resolver extensions; tests below.

## Invariants

- **INV-0013-1.** The `abi` `Name` constructor accepts exactly the well-formed UTF-8 strings of 1 to 255 bytes that contain no `/`, no code point in U+0000–U+001F, U+007F or U+0080–U+009F, and are not `.` or `..`. *Check:* unit tests with boundary cases (255/256 bytes, each forbidden code point, overlong encodings, encoded surrogates, U+10FFFF and above) plus a property test against an independent reference predicate.
- **INV-0013-2.** The filesystem service compares names byte for byte: two distinct valid names never refer to the same entry. *Check:* a property test that creates pairs of names differing only in case or normalisation form (for example U+00E9 versus U+0065 U+0301) and asserts both exist independently.
- **INV-0013-3.** The service never follows a link object: resolving any path through one returns `IsLink` and touches no object beyond the link. *Check:* service tests with link objects at every position of a path, asserting the reply and that the target's access counters (test instrumentation) are unchanged.
- **INV-0013-4.** No request can give an existing object a second name, and no rename can make a directory its own descendant. *Check:* a model-based test over random request sequences asserting that the namespace stays a tree with one name per object.
- **INV-0013-5.** No component `.` or `..` and no absolute path is accepted by any service request. *Check:* unit tests on the request decoder, and the `RelativePath`/`Name` types of INV-0002-1 and INV-0013-1.

## Open questions

- Should `libposix` offer an escape mapping for non-UTF-8 names (as Python's surrogate escape does) instead of
  `EILSEQ`? Research what archives in the toolchain's inputs contain (ADR-0002 open question).
- Pure Option C (no link objects) versus D: depends on whether extracting third-party archives with symlinks is in
  scope; no measured workload does it.
- Directory link counts reported by `libposix` (`st_nlink`): 2 plus the number of subdirectories (POSIX convention,
  maintained cheaply by the service) or 1. No measured program was shown to read link counts
  (`fs/requirements.md#5-questions-the-data-cannot-answer`).

## Revisit triggers

- A workload in scope fails because a name was rejected (`EILSEQ`): add a reversible mapping for POSIX names.
- A workload in scope needs hard links (for example a package format or build tool that hard-links outputs): a
  superseding ADR with the DM-040 rule (linking requires authority over the object).
- A requirement to read or write case-insensitive media: a separate FAT/exFAT service, not a change to this one.

## Architect's decision
