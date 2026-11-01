# ADR-0061: Listing a directory handle

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0032, ADR-0040, ADR-0043, ADR-0044, ADR-0045
- **Evidence:** CP-01, CP-02, DM-053

## Context
`directory_lookup` finds a name but cannot say what names exist. A shell's `ls` needs that, and stage 4's filesystem service plans a `read_directory` with an opaque cookie. Listing should have one shape. Nothing here is measured.

## Decision drivers
1. Authority comes from the handle (CP-01, CP-02): `LOOKUP`, as for names.
2. Typed, not an untyped buffer of unknown layout (DM-053).
3. The same bytes must be offerable by a service.
4. ADR-0032's registers and INV-0045-1's allow-list hold.

## Considered options

### Option A: `getdents` (Linux)
- **Description:** Fill a buffer with as many variable records as fit, with a position held in the open file.
- **Evidence for:** Fast for large directories, and POSIX's `readdir` sits on it.
- **Evidence against:** Hidden state in the handle, so two readers disturb each other, and a stage 3 `Directory` has no state.
- **Cost:** *(estimate)* 60 lines and a position.
- **Precedent:** Unix.

### Option B: One entry per call, with a cookie
- **Description:** `directory_entry(directory, cookie, entry)` writes one record: the next cookie and the name.
- **Evidence for:** No state in the handle. Drivers 1 to 4. A service can batch later by repeating the record.
- **Evidence against:** One system call per name.
- **Cost:** *(estimate)* 40 lines.
- **Precedent:** `fuchsia.io`'s dirent records in a buffer, with one record per call as a limit case `[outside evidence: fuchsia.dev fuchsia.io]`.

### Option C: A pseudo-file holding all names
- **Description:** Read one image holding the listing, as Plan 9 reads directories `[outside evidence]`.
- **Evidence for:** No new call.
- **Evidence against:** The ramdisk has no such file, and the layout becomes an untyped format (DM-053).
- **Cost:** *(estimate)* Small, with a format to keep.
- **Precedent:** Plan 9.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Call.** `directory_entry(directory, cookie, entry)` needs `LOOKUP`. `cookie` is a parameter of a new kind `Cookie`, opaque to the caller; 0 means the start. INV-0061-1 replaces INV-0045-1 to allow it by name.
- **Record.** 8 bytes of next cookie, little-endian, 4 of name length, 4 of zero, then the name, in `zoraya-abi` as `DirectoryEntry` with a total decoder. The call returns the bytes written, or 0 at the end. A buffer too small is `InvalidArgument`, and nothing moves.
- **Stage 4.** A service can return the same record, several in one reply, from its own cookie. Any cookie it issued stays meaningful only to it.

**Tensions with accepted ADRs.** *ADR-0045:* its allow-list needs `Cookie`, above.

## Consequences
- **Positive.** Listing is stateless, and the format is shared.
- **Negative.** A new `Param` kind. One call per name.
- **Follow-up.** The file protocol of stage 4 takes this record.

## Invariants
- **INV-0061-1.** Every object a native syscall acts on is designated by a `Handle` parameter; every path parameter has the `abi` type `RelativePath`, whose constructor rejects a leading `/`; the only integer payloads are `detail` fields, `ExitRecord.code`, port subscription keys, user packet data and directory cookies, each documented as opaque to the kernel or to its caller. *Check:* the lint of INV-0045-1, with `Cookie` allowed only for `directory_entry`, plus a unit test that `RelativePath::new("/x")` fails.
- **INV-0061-2.** Starting at cookie 0 and following next cookies gives each name once, then 0. *Check:* in-QEMU test against the ramdisk, and a `zoraya-abi` round-trip test.
- **INV-0061-3.** `directory_entry` without `LOOKUP` is `AccessDenied`. *Check:* in-QEMU test.
- **INV-0061-4.** `DirectoryEntry::decode` is total. *Check:* host test with truncations and mutations.

## Open questions
- Whether a batched form earns its place at stage 4.

## Revisit triggers
- A directory large enough for one call per name to show in a measurement.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
