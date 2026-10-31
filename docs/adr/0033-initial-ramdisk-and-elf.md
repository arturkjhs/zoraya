# ADR-0033: Initial ramdisk and ELF loading

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0020, ADR-0031
- **Evidence:** CP-16, CP-26, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris

## Context
User programs need a way into the machine before any filesystem exists (stage 4). Measured here: QEMU 11.1.2 ignores `-initrd` for this kernel. With an ELF `-kernel`, `/chosen` in the dumped device tree has no `linux,initrd-start`, because QEMU treats an ELF as a bare-metal image.

## Decision drivers
1. Works with `-kernel` and an ELF, as the kernel boots today.
2. Parsers of external bytes are total: they cannot panic or read out of bounds (ADR-0012's discipline, applied here).
3. W^X also holds for loaded programs (CP-16).
4. Small: the ramdisk is a stop-gap until the filesystem.

## Considered options

### Option A: `-initrd` with a Linux boot image
- **Description:** Boot as a Linux `Image` so that QEMU loads an initrd and names it in `/chosen`, with a cpio archive.
- **Evidence for:** The conventional path, independent of the kernel build [outside evidence: Linux `Documentation/arch/arm64/booting.rst`].
- **Evidence against:** Changes the boot path that stages 0 and 1 tested, and needs a flat image with a Linux header. Does not meet driver 1.
- **Cost:** Moderate.
- **Precedent:** Linux.

### Option B: Embed the ramdisk in the kernel image
- **Description:** The build packs user programs into one file and `include_bytes!` puts it in `.rodata`. A small own format: a header, then entries of name, offset and length.
- **Evidence for:** Needs no boot change. The bytes are mapped read-only and never aliased, as all of the image is (ADR-0020). Close to Hubris's image, built with its tasks (alternatives-postmortem.md#hubris).
- **Evidence against:** The kernel must be rebuilt to change a program, and the build gains an order: user programs, then the kernel. An own format has no outside tools.
- **Cost:** Low (estimate: 100 lines plus a host tool).
- **Precedent:** Zircon's ZBI is also an own boot format, though loaded separately (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon ZBI docs].

### Option C: `-device loader` at a fixed address
- **Description:** QEMU's generic loader places the file at an address the kernel knows.
- **Evidence for:** Keeps the kernel and programs separate, with no kernel rebuild.
- **Evidence against:** A fixed physical address is a hardware fact that does not come from the device tree, against `CLAUDE.md`, and the address must avoid RAM the allocator hands out.
- **Cost:** Low.
- **Precedent:** None in our evidence.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Ramdisk.** `zoraya-ramdisk`: a total reader with no `unsafe`, and a writer for `tools/mkramdisk`. If no ramdisk has been built, the kernel builds with an empty one, so `cargo build -p zoraya-kernel` still works.
- **ELF.** `zoraya-elf`: a total parser with no `unsafe`, for little-endian AArch64 static `ET_EXEC` only. It rejects anything else, a segment that is not readable, a segment both writable and executable (driver 3), segments that overlap or lie outside the user range of ADR-0031, and `memsz < filesz`.
- **Loading.** Frames are allocated, zeroed and mapped with the segment's permissions, then the file bytes are copied in through the direct map. The tables are not in use yet, so no page is writable while it can be executed. Executable pages then get instruction cache maintenance.
- **Authenticity.** None: the ramdisk is part of the kernel image. CP-26's verified image is a later matter.

## Consequences
- **Positive.** The boot path does not change. W^X extends to user programs at load time.
- **Negative.** Rebuilding programs rebuilds the kernel. The image grows with its programs.
- **Follow-up.** Replace with the filesystem at stage 4; dynamic linking and `ET_DYN` are outside this ADR.

## Invariants
- **INV-0033-1.** The ELF and ramdisk parsers never panic and never read out of bounds on any input. *Check:* `#![forbid(unsafe_code)]`, and host tests with truncations and random mutations.
- **INV-0033-2.** A program with a writable and executable segment, or a segment outside the user range, is refused. *Check:* host tests in `zoraya-elf`, and an in-QEMU test.

## Open questions
- Whether stage 4's first filesystem image replaces the ramdisk or is loaded by it.

## Revisit triggers
- A need to boot through firmware or a Linux-style loader.

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-08-review.md`.
