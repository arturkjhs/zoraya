# ADR-0021: Physical memory management

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0020
- **Evidence:** CP-14, DM-030, alternatives-postmortem.md#barrelfish, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4

## Context
The kernel needs frames for page tables and the heap now, and for user memory from stage 2. RAM on the test machines is 128 MiB to 2 GiB. Almost every request is one 4 KiB frame; multi-frame contiguous requests are expected only for device rings and for buffers shared with devices [assumption]. Nothing else in the kernel can allocate when this allocator starts, so its metadata needs a home that it does not allocate itself.

## Decision drivers
1. It must be impossible to receive a reserved frame or one already allocated, because every later protection rests on that.
2. Exhaustion must be a return value at the call site, not a kill somewhere else (CP-14, DM-030).
3. Metadata must fit in memory found before any allocator exists.
4. The logic must be host-testable with no hardware (`CLAUDE.md`).
5. Small enough to review. Stage 1 has no performance data to design for.

## Considered options

### Option A: Buddy allocator (Linux baseline)
- **Description:** Free lists of power-of-two blocks, split and merged on demand.
- **Evidence for:** O(log n) contiguous allocation; decades of use at any scale [outside evidence: Linux `mm/page_alloc.c`].
- **Evidence against:** Free-list links live in free frames, so a wild write corrupts the allocator, and detecting a double free needs a separate state map, which is the bitmap of Option B in addition to the lists [assumption: reasoning from the structure].
- **Cost:** Moderate (estimate: 300 to 400 lines plus state tracking). No measured need yet.
- **Precedent:** Linux.

### Option B: Bitmap per region
- **Description:** Two bits per frame (allocated, reserved) in a metadata block outside the managed frames. Next-fit scan for single frames; a run search for contiguous requests.
- **Evidence for:** [assumption: reasoning from the structure] State is outside the frames, so a wild write to a free frame cannot corrupt it. Double free, freeing a reserved frame and freeing outside RAM are all detected by reading two bits. Size is exactly known: 128 KiB of metadata per 2 GiB of RAM.
- **Evidence against:** Contiguous search is O(frames), and allocation cost grows as memory fills, although word-at-a-time skipping bounds it.
- **Cost:** Lowest (estimate: 200 lines).
- **Precedent:** Early allocators of many teaching kernels; it is the structure a bump-plus-bitmap kernel grows out of [assumption].

### Option C: Static or typed memory in the seL4 and Hubris manner
- **Description:** No kernel allocator policy. Hubris fixes memory at build time (alternatives-postmortem.md#hubris). seL4 hands untyped capabilities to user space, which retypes them, so kernel objects are paid for by user-level policy (alternatives-postmortem.md#l4-family-and-sel4).
- **Evidence for:** Exhaustion moves to a defined owner, the property CP-14 wants, and the kernel has no hidden allocation.
- **Evidence against:** Barrelfish found the seL4 model a heavy distraction on dynamic PC-class memory (alternatives-postmortem.md#barrelfish). Static allocation cannot serve a general process model.
- **Cost:** Highest (estimate: it presumes the handle model of stage 3).
- **Precedent:** seL4, Hubris.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision. The interface, not the structure, is what the rest of the kernel depends on, so replacing the bitmap with a buddy allocator later stays local.

- **Interface.** `alloc() -> Result<Frame, OutOfFrames>`, `alloc_contig(count, align) -> Result<Frame, OutOfFrames>`, `free(frame)` and `free_contig(frame, count) -> Result<(), FreeError>`, and `stats()` with total, reserved and free counts. Frames are 4 KiB and are not zeroed; zeroing is the caller's duty.
- **Reserved regions.** RAM minus: the kernel image with its stacks and boot tables; the device tree blob; every `/memreserve/` entry; every `reg` of a `/reserved-memory` child, enabled or not; the allocator's own metadata, carved from the largest remaining free range; and frame 0 if RAM contains it. Partial pages are rounded outward. A device tree with more reservations than the parser records stops the boot, because an unknown reservation could otherwise be handed out. Firmware areas are covered only as far as the device tree names them: under QEMU `virt` with a bare ELF, nothing else is in RAM.
- **Exhaustion.** Returned to the caller, never a panic in the allocator. A bad `free` is a kernel bug: the crate returns the error and the kernel panics on it.

## Consequences
- **Positive.** The allocator's guarantees can be tested exhaustively on the host. Metadata cannot be damaged by a stray write to a free frame.
- **Negative.** Contiguous allocation is O(frames) and can fail under fragmentation with enough total memory free.
- **Follow-up.** Per-CPU frame caches at SMP. A commit-accounting layer when CP-14 is taken up. Revisit when `alloc_contig` becomes frequent.

## Invariants
- **INV-0021-1.** The allocator never returns a frame that is reserved, outside RAM, or already allocated. *Check:* host property tests in `zoraya-pmm` against a model, and an in-QEMU test that drains the allocator and compares every frame with the live device tree.
- **INV-0021-2.** Freeing a frame that is free, reserved or outside RAM is reported as an error and changes no state. *Check:* host tests in `zoraya-pmm`.
- **INV-0021-3.** Exhaustion returns `OutOfFrames` and leaves the allocator usable. *Check:* host test in `zoraya-pmm`, and the in-QEMU drain test.

## Open questions
- Whether `alloc_contig` needs a lower bound on physical address for 32-bit devices. virtio on QEMU has no such limit [assumption].
- When frames must be zeroed on free rather than on allocation, which depends on stage 2.

## Revisit triggers
- `alloc_contig` showing up in a profile, or failing with enough free memory.
- Frame allocation showing up in the cost of the `snapshot_process` measurement that ADR-0005 requires.

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-05-review.md`.
