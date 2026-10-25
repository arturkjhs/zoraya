# TASK-06: Filesystem design (research and proposed ADRs)

## Where this runs

This task runs in a **cloud session** (Linux container), in parallel with kernel work done elsewhere. It needs only the repository and web access. It does not need QEMU, the Mac, or any VM.

## Context

Zoraya OS: Rust `no_std` kernel, aarch64, QEMU `virt`. Native interfaces are our own design; POSIX is a compatibility personality (`libposix`). Accepted decisions are in `docs/adr/` (ADR-0001 to ADR-0006). If `CLAUDE.md` exists on `main`, follow it.

Read first:
- `docs/adr/index.md`; ADR-0001 in full; ADR-0002 (namespace table, temporary directory service, no uid/gid natively) and ADR-0005 (POSIX scope) in full; the Recommendation sections of ADR-0003, ADR-0004, ADR-0006.
- `docs/research/design-mistakes.md` and `held-up-well.md`: every entry in category 6 (filesystems, namespaces, storage semantics), category 16 (text, encoding, locale), and any entry they reference.
- `docs/research/candidate-principles.md` and `alternatives-postmortem.md` (Fuchsia, Redox, Plan 9, QNX sections at least).
- `docs/research/task-02/libc-surface.csv` and `syscall-surface.csv`, and `tools/posix-surface/results/` path analyses.

The filesystem will be a userspace service (ADR-0002). This task designs **what it stores, what it guarantees and how it stays consistent**. It does not design the IPC interface between clients and the service in detail; that depends on ADRs about channels and messages that do not exist yet. Where the design depends on them, state the assumption.

This task produces evidence and **proposed** ADRs only. No implementation. The architect decides; implementation is a separate task after acceptance.

## Part 1: What the filesystem must support (measured)

From the TASK-02/02b data in the repository, produce `docs/research/fs/requirements.md`:
- every filesystem-related libc function and syscall used in tiers T0–T2, with counts and which programs use them (e.g. `rename`, `link`, `symlink`, `readlink`, `unlink`, `mkdir`, `utimensat`, `chmod`, `umask`, `fstat`, `fsync`, `ftruncate`, `openat` flags, `O_EXCL`, `O_APPEND`, `O_TRUNC`, `mmap` of files);
- which of these the measured toolchain depends on for correctness (e.g. atomic `rename` for build outputs, `O_EXCL` for temporary files, timestamps for make);
- whether the measured workloads ever create or follow symbolic links or hard links, and where;
- what file metadata is read (size, mode bits, timestamps, inode numbers, link counts) and what for. make depends on modification times: record their required resolution and ordering behaviour.

Every number comes from the existing CSVs or results, with the file and query. If the data does not answer a question, say so; do not run new measurements.

## Part 2: Research

Produce `docs/research/fs/survey.md`. For each design below: crash-consistency mechanism, integrity (checksums), metadata structure, how it handles the failure modes in the TASK-01 catalog, implementation size and maturity, and what went wrong in practice. Same evidence rules as TASK-01: primary sources preferred, every URL fetched, never invent a citation, paraphrase.

At least: ext4 (journaling modes), XFS, btrfs and ZFS (copy-on-write), F2FS and NILFS (log-structured), Fuchsia Fxfs and Minfs, RedoxFS, littlefs, FAT, Plan 9's fossil/venti or cwfs, and anything you find relevant (for example WAFL, bcachefs).

Also research and summarise:
- **crash consistency as applications experience it**: what applications assume about ordering and atomicity, and what filesystems actually guarantee (the literature on application-level crash vulnerabilities, the PostgreSQL fsync discussion, rename-over-write data loss);
- **testing crash consistency**: record-and-replay of block writes, enumerating reorderings within flush boundaries, model-based testing against a reference implementation, fuzzing of on-disk images;
- **names**: byte strings vs UTF-8, normalization, case sensitivity, forbidden characters, maximum lengths, and the security and portability problems each choice caused.

## Part 3: Proposed ADRs

ADR numbers **0010 to 0019 are reserved for this task** so they do not collide with ADRs written in parallel. Use the template in `docs/adr/0000-template.md` exactly, status `proposed`, "Architect's decision" empty. Every option list includes what Linux/POSIX filesystems do as a baseline and at least one non-Unix precedent.

Write one ADR per decision below (merge or split if the evidence argues for it, and say why):

1. **Guarantees to clients.** What is atomic (rename, create, single writes?), what ordering clients can rely on, what "durable" means and how a client requests it, and how a write-back error is reported so it is never silently lost (DM entries on fsync semantics). This is the most important ADR of the batch: everything else must satisfy it.
2. **Crash-consistency mechanism.** Journaling, copy-on-write, log-structured, or another design. Must state the expected implementation size and how each option would be tested.
3. **Integrity.** Checksums on metadata and/or data, algorithm, what happens on mismatch, whether the filesystem must survive a maliciously crafted image (assume yes for removable or downloaded images and say what that requires).
4. **Names and namespace.** Encoding, normalization, case, limits; whether symbolic links and hard links exist natively; how `..` behaves given handle-relative access (ADR-0002, the DM entries on path races); what the POSIX personality must emulate if native support is omitted, using the Part 1 data.
5. **Metadata.** Which attributes exist natively (sizes, timestamps with resolution and range, no uid/gid per ADR-0002), how POSIX mode bits and `umask` are represented for the personality, extended attributes or not, sparse files or not.
6. **Block device contract.** What the filesystem assumes about the device: block size, atomicity of a single block write, reordering until a flush, flush and FUA semantics, how errors are reported. Must match virtio-blk semantics; state where you rely on the virtio specification and cite it.

Each ADR lists **invariants** that a later implementation can check (by tests in a host-tested crate), and the tests that would check them.

Also produce `docs/research/fs/implementation-outline.md`: a non-binding outline of the crates the accepted design would need (on-disk format, core engine behind a block-device trait, `mkfs`, `fsck`), the test strategy (model-based tests, crash injection, image fuzzing), and an estimate of size and order of work. Mark everything as depending on the ADRs' acceptance.

## Independent review

A subagent that did not write the ADRs checks: citations support claims; options are not strawmanned; recommendations follow from Part 1 data and Part 2 evidence; invariants are actually testable; the six ADRs are consistent with each other and with ADR-0002 to ADR-0006. Record findings in `docs/research/fs/review-notes.md` and fix them, with a disposition table as in TASK-03.

## Git

- Branch: `fs/design`, from the latest `main`.
- Use the configured git identity. If none is configured, stop and ask. Never pass `-c user.name` / `-c user.email` or invent an identity.
- Commit after each part. You may **push this branch only** (`fs/design`), so the architect can read it. Never merge, never push other branches, never force-push.
- Touch only `docs/research/fs/` and `docs/adr/0010-*` to `docs/adr/0019-*`. Do not edit other ADRs, `index.md`, `invariants.md`, other research files, or code. List the index and invariant rows you would add in your final report instead; the architect merges them.

## Definition of done

- `requirements.md` answers every Part 1 question from existing data, or says which ones it cannot.
- `survey.md` covers every listed design with sources.
- ADRs for all six decisions, `proposed`, template-exact, with invariants and tests.
- `implementation-outline.md` exists.
- Independent review done and addressed.
- Branch `fs/design` pushed; final report lists the ADRs, the recommended option of each in one line, and open questions for the architect.

## Out of scope

- Any code.
- The client–service IPC protocol (depends on future channel/message ADRs).
- Accepting or changing accepted ADRs.
