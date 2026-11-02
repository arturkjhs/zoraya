# TASK-13: Stage 4b — virtio-blk driver, filesystem service, files in the shell

## Precondition

TASK-12 is merged into `main` (64-bit handles, memory objects, channels, protocols, `init`, device objects), and its proposed ADRs are accepted. If not, stop and report.

## Context

Read first: `CLAUDE.md`, `docs/design.md`, `docs/adr/index.md`, `docs/backlog.md`, `docs/research/stage-4/plan.md`, `docs/tasks/TASK-12-review.md`.

Read **in full**: ADR-0010 to ADR-0016 (filesystem), ADR-0050 to ADR-0055, ADR-0060 to ADR-0062, and TASK-12's accepted ADRs.

This task finishes stage 4, steps 6 to 10 of `plan.md`, and adds file commands to `zosh`. The roadmap criterion is: **files survive reboot**.

ADR numbers **0080 to 0089** are reserved for this task. Where an accepted ADR is silent, write a proposed ADR; never fill a gap silently.

## What to build

- **virtio-blk driver (ADR-0054, ADR-0015)** as a user process: device reset and negotiation, a virtqueue in a contiguous memory object pinned for DMA, read, write and flush, interrupts through a port, validation of every length and index the device returns. It serves the block protocol (ADR-0052 declaration from TASK-12) to exactly one client. Host tests against a fake device implementing the virtio-mmio register and queue behaviour, including hostile device responses.
- **Disk in QEMU:** `tools/run-qemu.sh --disk FILE`, the only place that attaches a `virtio-blk-device`. `make run` builds a default image with `zoraya-mkfs` from a directory in the repository (for example `rootfs/`, with a README and a few small files) unless an image already exists, and attaches it. The image is not committed.
- **Filesystem service (ADR-0055)** in a thin `fs-service` crate around `zoraya_fs::Volume`, over a `BlockDevice` that speaks the block protocol: one process per volume, one thread, one port, one request at a time per connection, replies and `WouldBlock` handling, `TooManyOpen`, `MAX_INLINE` and attached memory objects, `readdir` with cookies, failure reports on the supervisor channel, `device_lost`. The loop runs on the host behind a transport trait, so most of the protocol is host-tested.
- **`init`:** starts the driver with its device handles, the filesystem service with a channel to the driver, and `zosh` with a directory channel to the root of the volume. Supervision as ADR-0053 says. Restart policy as the ADRs say, or the simplest one with a proposed ADR if they leave it open.
- **`zosh` file commands:** `ls [dir]` (the volume; the ramdisk listing moves to `ls-ramdisk` or similar, say which), `cat`, `write <file> <text...>` (create or replace), `append`, `mkdir`, `rm`, `mv`, `stat` (size, `mtime`, `ctime`, advisory mode), `sync`, `df` if the protocol offers it. Paths are relative to the shell's current directory channel; add `cd` and `pwd` if the protocol allows it cleanly, else explain why not. Errors print the protocol status by name.

Data path: inline messages for small transfers, memory objects above `MAX_INLINE`, as ADR-0055 says.

## Tests

- **Roadmap criterion as an automated test:** boot with a fresh image, write files and a directory through the shell, `sync`, power off; boot again with the same image; the files are there with the same content. Also without `sync`: what must and may survive follows ADR-0010, and the test checks exactly that.
- **Crash during writes:** stop QEMU at random points while the shell writes (repeat enough times to state a number); after each, the next boot mounts, `zoraya-fsck` on the image reports clean, and every file made durable before the crash is intact (ADR-0010, ADR-0011).
- **Failure paths (`plan.md` step 9):** the driver is killed; a device error is injected (through the host fake, and in QEMU if a method exists, otherwise state it); the service is killed. `Failed` is observed by clients as the ADRs specify, `init` reacts as specified, and a remount recovers the durable prefix.
- **Hostile clients:** malformed file-protocol messages, too many open files (`TooManyOpen`), a client that never reads replies (the service stays responsive to others), paths with `..`, absolute paths and control characters (refused as ADR-0013 says).
- **Shell:** a scripted session of every file command, including errors.
- Every invariant of ADR-0010 to ADR-0016 and ADR-0050 to ADR-0055 that becomes checkable in a running system gets its check, with status in `invariants.md`. Report which "partly checked" rows from TASK-07 are now fully checked.
- Measurements under HVF and TCG: write and read throughput for 4 KiB and 1 MiB files, inline and through memory objects; time for `sync`; boot to prompt with a mounted volume.

`make test` passes from a clean clone in every configuration; CI stays green. If an image-based test needs `zoraya-mkfs`, the Makefile builds it.

## Documentation

`docs/design.md`: "as built" sections, repository map, and the "Using the shell" section with the file commands. In the roadmap table mark stage 4 as done when the criterion test passes: the architect instructs you to do this. `index.md`, `invariants.md`, `backlog.md`.

## Process

1. Read the context. 2. Proposed ADRs for gaps, commit. 3. Driver with host tests. 4. Disk in `run-qemu.sh`, image build. 5. Filesystem service with host tests. 6. `init` wiring. 7. Shell commands. 8. Tests, invariants, measurements, documentation. Commit after each step.
9. Independent review by a subagent that did not write the code: `make test` from a fresh clone; the driver against the virtio specification for queue handling, barriers and validation of device-written data; the service loop for blocking, starvation and lost failures; every new invariant against its check with deliberate mutations; the crash test's coverage. Findings and dispositions in `docs/tasks/TASK-13-review.md`; fix them. Then a short check by another subagent of the fixes only.
10. Final report: what was built, `make test` summary, crash-test numbers, measurements, invariant delta, proposed ADRs, open questions, and the exact commands to try it by hand.

## Git

Branch `stage-4b` from the latest `main`. Configured git identity only. Commit after each step. **Never merge, never push.**

## Definition of done

- `make run` boots to `zosh` with a mounted volume; files written in one boot are read in the next.
- The roadmap criterion, crash, failure-path, hostile-client and shell tests pass; `make test` passes from a clean clone.
- Independent review and the check of its fixes done and addressed.

## Out of scope

- ADR-0056 quotas: every process is trusted at stage 4, as `design.md` states.
- POSIX, `fork`, pipes between programs, a terminal service, GUI, SMP.
- Spawning programs from the volume (`plan.md` step 11) unless it falls out cheaply; if done, through a proposed ADR.
