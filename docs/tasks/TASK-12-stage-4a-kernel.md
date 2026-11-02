# TASK-12: Stage 4a — handle width, memory objects, channels, protocols, init, device objects

## Precondition

`main` contains stage 3a (tag `stage-3a`) and accepted ADR-0050 to ADR-0055 and ADR-0057. ADR-0056 is `proposed`. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md` (including Known limitations and the rule that no untrusted code runs before ADR-0056), `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-4/plan.md`.

Read **in full**: ADR-0040, ADR-0041, ADR-0044, ADR-0050 to ADR-0055, ADR-0057, ADR-0060 to ADR-0063, and the review notes for stage 4 (`docs/research/stage-4/review-notes.md`), at least the sections of the last two reviews.

Stage 4 is "virtio-blk, filesystem service, `mkfs` on host — files survive reboot". It is split in two tasks. This one builds the kernel objects and the user-space foundations, steps 1 to 5 of `plan.md` plus the handle width change. TASK-13 builds the driver, the filesystem service and the roadmap criterion on top of it.

The accepted ADRs are the specification. Where they are silent, write a proposed ADR (numbers **0070 to 0079** are reserved for this task); never fill a gap silently.

## Step 0: decisions to settle before code

Write proposed ADRs for:
1. **Releasing quarantined DMA frames** (backlog W4). Who may end a quarantine, when, and what proves the device no longer writes the frames: a device reset observed by the kernel, the death of the driver's job followed by a reset, or another mechanism. A running driver must not be able to free frames its device may still write. Give the invariant and how it is tested.
2. **The `invariants.md` total** (backlog): `make adr-check` must verify the stated totals against the rows, so a mismatch fails the build.

Commit them before the code they govern. Implement the recommended option; the architect accepts or changes it when reviewing this branch.

## Step 1: 64-bit handles (ADR-0057)

First, before any new format carries a handle: `Handle` becomes `u64` (slot bits 0–15, generation bits 16–63), no slot retirement, `ZSP2` for the spawn format as ADR-0057 says. Update `zoraya-abi`, the kernel, `zoraya-rt`, `zosh`, the test programs and every test. Check INV-0057-1 as the ADR states. `make test` must pass at the end of this step on its own commit.

## Step 2: what to build

Following `plan.md` steps 1 to 5 and the accepted ADRs:

- **Memory objects (ADR-0051):** create with eager commit and zeroing, optional contiguous allocation, map with kernel-chosen address, unmap with break-before-make and TLB maintenance, rights, mapping limit, lifetime held by handles, mappings and pins.
- **Channels (ADR-0050):** create, write (all or nothing, handles moved or duplicated with the rights rules), read (with `BufferTooSmall` reporting sizes and leaving the message), limits, signals `READABLE`, `WRITABLE`, `PEER_CLOSED`, the no-cycle rule, iterative teardown. The `Out` parameter kind and the lint changes INV-0051-5 names.
- **Protocols (ADR-0052):** the `protocol!` macro, the wire format with its checked padding, total decoders under `forbid(unsafe_code)`, generic error replies for unknown ordinals, one crate per protocol. Declare the block protocol (ADR-0054) and the file protocol (ADR-0055) now, with decoder tests, even though their servers come in TASK-13.
- **`init` (ADR-0053):** the normal kernel starts only `init` with its slots (console, own job, ramdisk directory, `POWER` at slot 4, `Device` handles from slot 5). At this stage `init`'s compiled table starts `zosh` in a sub-job with only the handles `zosh` needs, and powers off when `zosh` exits. ADR-0063 is then replaced: record that in ADR-0063 and ADR-0053 as ADR-0001 allows. `zosh` must keep working exactly as before; its tests stay green.
- **Device objects (ADR-0054):** device objects from the device tree, `device_map` (uncached, never executable, one holder at a time), interrupt objects with `INTERRUPT` signal and `interrupt_ack`, `dma_pin` / `dma_unpin` with device addresses written to an output buffer, and quarantine with the release rule from Step 0. The new rights `MAP`, `INTERRUPT`, `DMA`.

If an accepted ADR turns out to be impossible or contradictory once implemented, stop that part, write a proposed superseding ADR, and continue with the rest.

## Step 3: tests

- `plan.md` "done when" criteria for steps 1, 2, 4 and 5 as in-QEMU tests: a process shares a page with a child through a memory object; every INV-0050 check passes; normal boot starts `init`, which starts `zosh`; a user program maps a virtio-mmio device, reads its magic value and takes an interrupt (use a virtio-blk device attached for the test only, without a driver).
- **Every invariant of ADR-0050 to ADR-0055 and ADR-0057 that this task makes checkable gets its check now**, with status `checked: <command>` in `invariants.md`. The rest keep `not yet applicable` with the stage and reason.
- Hostile inputs: malformed messages, too many handles, an endpoint sent through itself or its peer, a memory object mapped beyond its size, unmapping what is in use by a pin, a pin closed while the device is active (quarantine holds), a decoder fed random bytes and every truncation of valid messages. None panics the kernel.
- Leaks: a loop creating and destroying channels with handles in flight, memory objects with mappings and pins; kernel heap and frame counts return to their starting values.
- Measurements `plan.md` asks for, under HVF and TCG: channel round trip; inline copy cost per KiB against a mapped memory object; `port_wait` with many channels. Report numbers, do not optimise.

`make test` passes from a clean clone in every configuration; CI stays green.

## Documentation

`docs/design.md` "as built" sections and repository map; `index.md`, `invariants.md`, `dependencies.md`, `backlog.md` (close W4 and the totals item when done). Do not change Vision or the roadmap.

## Process

1. Read the context. 2. Step 0 ADRs, commit. 3. Step 1 (handle width), commit with `make test` green. 4. Memory objects. 5. Channels. 6. Protocols. 7. `init`. 8. Device objects and quarantine. Commit after each. 9. Tests, invariants, measurements, documentation.
10. Independent review by a subagent that did not write the code: `make test` from a fresh clone; every new invariant against its check, using deliberate mutations to confirm each check fails when the property is broken; kernel object lifetime (use-after-free, leaks, quarantine); the TLB and break-before-make sequence for unmapping against the Arm ARM; all `unsafe` and `SAFETY` comments; decoders for out-of-bounds reads and unbounded loops; consistency with each accepted ADR. Findings and dispositions in `docs/tasks/TASK-12-review.md`; fix them. Then a second, short check by another subagent of the fixes only (changed lines since the review).
11. Final report: what was built, `make test` summary, measurements, invariant delta, proposed ADRs with one-line recommendations, open questions.

## Git

Branch `kernel/stage-4a` from the latest `main`. Configured git identity only. Commit after each step. **Never merge, never push.**

## Definition of done

- Handles are 64-bit everywhere, on its own green commit.
- Memory objects, channels, protocols with total decoders, `init`, device, interrupt and DMA objects with quarantine work as the accepted ADRs specify; `zosh` works under `init`.
- The listed checks and measurements exist; `make test` passes from a clean clone.
- Independent review and the check of its fixes done and addressed.

## Out of scope

- The virtio-blk driver, the filesystem service, disk images in `run-qemu.sh`, the roadmap criterion (TASK-13).
- ADR-0056 quotas (still `proposed`), POSIX, `snapshot_process`, SMP.
