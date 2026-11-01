# TASK-10: Design for stage 4 — IPC, driver model, filesystem service (proposed ADRs)

## Where this runs

A **cloud session**, in parallel with stage 3 kernel work (TASK-09) on the architect's Mac. It needs only the repository and web access. No code, no QEMU.

## Precondition

`main` must contain stage 2 and the TASK-07 review fixes. If `docs/adr/` has no ADR-0030 to ADR-0036, stop and report.

## Context

Read first: `CLAUDE.md` (it binds you), `docs/design.md`, `docs/adr/index.md`. Read in full: ADR-0002, ADR-0003, ADR-0004, ADR-0006, ADR-0010 to ADR-0016, ADR-0031, ADR-0032. Read the Recommendation sections of the others. Read `docs/research/design-mistakes.md` and `held-up-well.md` entries in categories 3 (IPC), 7 (kernel structure, drivers, fault isolation) and 8 (ABI design), `candidate-principles.md`, and the Fuchsia, QNX, L4/seL4, MINIX 3, Redox, Hubris and Genode sections of `alternatives-postmortem.md`.

Stage 4 is "virtio-blk, filesystem service, `mkfs` on host — files survive reboot". Building it needs decisions that do not exist yet. This task writes them as **proposed** ADRs. The architect decides; stage 4 is built after acceptance.

### The architect's vision is not final

Zoraya may become (1) a documented experiment in building an OS with AI agents, (2) a niche system for running untrusted code such as AI agents in strict isolation, which would later need networking, threads and common language runtimes, and (3) a general-purpose OS. None is chosen yet. Therefore every ADR in this task must:
- recommend an option that works for all three where one exists;
- state explicitly, in a short subsection "Effect of the vision", whether the recommendation changes if Zoraya targets (2), and what it would cost to change later;
- never preclude threads or networking, even though ADR-0005 keeps them out of the POSIX scope.

## Decisions to write

**ADR numbers 0050 to 0059 are reserved for this task.** Template-exact, status `proposed`, "Architect's decision" empty, at least three options each with a Linux baseline and a non-Unix precedent, steelmanned. Short and concrete.

1. **IPC channels.** The native message-passing object: message vs byte stream (or both, and how they relate to ADR-0004's `PeerClosed` byte streams and ADR-0006 readiness), message size limits, handle transfer in messages with rights, ordering, backpressure, how a full or closed peer is reported, and the signals a channel exposes to ports.
2. **Interface definitions.** How services describe their protocols (an IDL with a code generator, Rust types as the schema, or hand-written encoding); wire format and its versioning; validation of untrusted messages as total decoders like ADR-0012's. Must address the research catalog's entries on `ioctl` and untyped interfaces.
3. **Service discovery and the first process.** How a process gets a channel to a service without a global name namespace (ADR-0002's launcher-built namespace table); what the first user process (init/launcher) is, what handles it receives from the kernel, and how it starts services.
4. **Driver model.** Where the virtio-blk driver lives (kernel or user process) and why; if in user space: kernel objects for device memory (MMIO) and interrupts, how DMA buffers are allocated and pinned, and the trust boundary given that QEMU `virt` has no IOMMU by default (state what a malicious or buggy driver could do and whether `virt`'s SMMUv3 option changes that). Restart of a failed driver: what the filesystem service observes, related to ADR-0010's fail-stop.
5. **Filesystem service protocol.** How ADR-0010's guarantees (completion order, atomic requests, durability on `sync`, sticky fail-stop) map onto messages; how `zoraya-fs` (which knows nothing about IPC) is wrapped; one service per volume (ADR-0012); how clients get file and directory handles and what rights they carry; how data moves (copy through messages, shared memory, or both) and the cost of each; and how `libposix` will later sit on top for ADR-0005 T1 (open, read, write, rename, stat, readdir — from `docs/research/fs/requirements.md`).
6. **Shared memory objects** (if 1, 4 or 5 need them): a memory object type, mapping into address spaces (ADR-0031), rights, and lifetime.

Merge or split decisions if the evidence argues for it, and say why. One decision per ADR (lesson from ADR-0016).

For each ADR give the invariants a later implementation can check, and how.

Also write `docs/research/stage-4/plan.md`: a non-binding outline of stage 4 work in order (kernel objects, driver, service, `mkfs` image as a virtio-blk disk via `tools/run-qemu.sh`, tests), with what depends on which ADR.

## Research rules

Same as TASK-01: primary sources preferred (papers, specifications such as the virtio specification, design documents of Fuchsia/Zircon FIDL, seL4, QNX, Genode, MINIX 3), every URL fetched before citing, never invent citations, paraphrase. Put new sources in `docs/research/stage-4/sources.md` with check dates.

## Independent review

A subagent that did not write the ADRs checks: citations support claims; options are not strawmanned; recommendations are consistent with every accepted ADR (list any tension explicitly); the six ADRs are consistent with each other; invariants are testable; each "Effect of the vision" section is honest. Findings and dispositions in `docs/research/stage-4/review-notes.md`; fix them.

## Git

- Branch `adr/stage-4-design` from the latest `main`.
- Configured identity only. If none, stop and ask. Never pass `-c user.name` / `-c user.email`.
- Commit after each ADR. **You may push this one branch and nothing else.** Never push any other branch, never create other branches on the remote, never merge, never force-push. Earlier cloud sessions pushed branches outside their task; that is a violation.
- Touch only `docs/adr/0050-*` to `docs/adr/0059-*` and `docs/research/stage-4/`. Do not edit `index.md`, `invariants.md`, other ADRs, `design.md` or code. List the index and invariant rows to add in your final report.

## Definition of done

- Proposed ADRs for decisions 1–5 (and 6 if needed), each with "Effect of the vision" and invariants.
- `plan.md` and `sources.md` exist.
- Independent review done and addressed.
- Branch `adr/stage-4-design` pushed; final report lists each ADR with its recommendation in one line, tensions with accepted ADRs, and questions for the architect.

## Out of scope

- Code of any kind.
- Networking and thread design (only: do not preclude them).
- Changing or accepting any ADR.
