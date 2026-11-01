# Stage 4 outline

Non-binding, part of TASK-10. It orders the work; the ADRs decide. Numbers refer to proposed ADRs in `docs/adr/`.

| Step | Work | Depends on | Done when |
|---|---|---|---|
| 1 | Memory objects: create, map, unmap, rights, eager commit | ADR-0050 (handles in messages use the same table), ADR-0051 | a process shares a page with a child |
| 2 | Channels in the kernel, with the no-cycle rule | ADR-0050 | every INV-0050 check passes |
| 3 | `protocol!` macro, the block and file protocols as declarations, decoder tests | ADR-0052, ADR-0054, ADR-0055 | fuzz-style host tests of every decoder |
| 4 | `init` as the kernel's only process in the normal kernel; supervisor channel | ADR-0053 | normal boot starts `init`, and it starts a service |
| 5 | Device memory, interrupt and DMA objects; test with a loopback of virtio-mmio registers under QEMU | ADR-0051, ADR-0054 | a user program reads the virtio magic value and takes an interrupt |
| 6 | `virtio-blk` driver in user space, implementing ADR-0015's trait, with a host test against a fake device | ADR-0015, ADR-0054 | the host fake reads, writes and flushes; real-device test in step 7 |
| 7 | `tools/run-qemu.sh --disk FILE`, the one place that adds the `virtio-blk-device` | CLAUDE.md, ADR-0054 | a `zoraya-mkfs` image is read and written through the driver |
| 8 | Filesystem service around `zoraya-fs`, one per volume | ADR-0010 to ADR-0016, ADR-0055 | a client creates, reads and renames a file |
| 9 | Failure paths: driver killed, device error, service killed | ADR-0010, ADR-0054, ADR-0055 | `Failed` is observed and a remount recovers the durable prefix |
| 10 | Roadmap criterion: a file written, reboot, file read, in `make test` with a disk image | all | files survive reboot |
| 11 | Spawn from a filesystem image: image handle from a file | ADR-0043 open question, ADR-0055 | optional for the stage |

Order of risk: steps 2 and 5 touch the kernel's hardest code. Step 9 is the one the design is most likely to change.

Measurements to take on the way: round-trip time of a channel call under HVF and TCG; cost per KiB of inline against shared-memory transfer; `port_wait` with many client channels; whether 64 messages per queue is enough under one client.
