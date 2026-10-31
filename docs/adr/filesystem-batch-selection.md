# Decision on the filesystem ADR batch (ADR-0010 to ADR-0015)

- **Date:** 2026-10-31
- **Made by:** an agent acting as architect, on the project owner's explicit instruction to take this decision in the
  owner's place, because the owner does not assess this area.
- **Scope:** the reasons behind accepting ADR-0010 to ADR-0015. It is not an ADR. The decision itself is in each ADR's
  "Architect's decision" section. That instruction covers this batch only; it is not a standing change to ADR-0001
  rule 2. No human specialist reviewed this decision, which is a reason to take the revisit triggers seriously.

## Decision

Accept all six, each with its recommended option.

| ADR | Decision area | Accepted | Why |
|---|---|---|---|
| [ADR-0010](0010-filesystem-client-guarantees.md) | Guarantees to clients | Option C: a crash leaves a prefix of the completion order; `sync` on request; sticky fail-stop | No measured program requests durability, so only a consistent default protects the toolchain. It is also one checkable rule. |
| [ADR-0011](0011-filesystem-crash-consistency-mechanism.md) | Crash consistency | Option B: copy-on-write tree with dual root records and no replay | It gives ADR-0010's prefix rule for data overwrites at no extra cost, and leaves two outcomes per commit to test. |
| [ADR-0012](0012-filesystem-integrity.md) | Integrity | Option B: CRC32C in parent pointers, a total decoder, one service per volume | It detects lost and misdirected writes, and crafted images are contained without relying on checksums. |
| [ADR-0013](0013-filesystem-names-and-namespace.md) | Names | Option C for encoding, equality, hard links and `..`; Option D's inert link objects for symbolic links | UTF-8 with byte equality needs no Unicode tables. Link objects keep the server out of path races and cost little. |
| [ADR-0014](0014-filesystem-metadata.md) | Metadata | Option D: `mtime`, `ctime`, advisory mode; no atime, owners or xattrs; strictly increasing times | It stores what the workloads write, and turns `make`'s assumption about times into a tested guarantee. |
| [ADR-0015](0015-block-device-contract.md) | Block device | Option B: the virtio contract as written, with no write atomic and flush as the only barrier | It is the only option a test device can reproduce exactly, and ADR-0011 needs nothing stronger. |

**Why all six.** They form one design: ADR-0010 defines the contract, ADR-0011 implements it on ADR-0015's device,
ADR-0012 sits in ADR-0011's block pointers, and ADR-0013 and ADR-0014 define what the tree stores. Rejecting one
would reopen the others. The independent review (`docs/research/fs/review-notes.md`) raised 61 findings, and the
drafter records all of them as addressed, none declined.

## The one real choice among the recommendations

ADR-0013 leaves open whether to keep link objects or take pure Option C, which removes one node kind and the
`libposix` link follower. This decision keeps link objects: no measured workload needs them, but they let archives
with symbolic links round-trip, and removing them later is cheaper than adding a node kind to a format in use. If
extracting third-party archives turns out to be out of scope, pure Option C is the better choice.

## Conditions that remain open

None of these blocks acceptance, but each must be settled before or during implementation:

1. **Assumed values.** `MAX_ATOMIC_WRITE` (1 MiB) and `T_COMMIT` (5 s) wait for the IPC ADR and a commit-cost
   measurement. `N_PAIRS`, the 4 KiB block size and the path limits are also marked as assumptions.
2. **Wall clock.** ADR-0014 needs a wall clock. ADR-0023 (proposed on `kernel/stage-1`) gives the kernel only a
   monotonic clock, so the device-tree RTC that ADR-0014 assumes has no implementation yet.
3. **Unmeasured costs.** Write amplification on W1/W4 (ADR-0011) needs the prototype. Its result could change block
   size or leaf size, not the choice of copy-on-write.
4. **Single-copy metadata.** With ADR-0012 as recommended, one bad metadata block makes a volume read-only until
   offline repair. Metadata duplication stays an open question.
