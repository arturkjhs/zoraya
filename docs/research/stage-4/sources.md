# Sources for stage 4 design

New sources for TASK-10. Each URL was fetched on the check date and the note says what it was read for. Sources already in [`../sources.md`](../sources.md) (S-numbers) are cited through `alternatives-postmortem.md` and are not repeated. Fetches that failed or that did not answer are listed at the end, so nothing below rests on them.

## Channels

- [Z1] Fuchsia project, Channel (kernel object), https://fuchsia.dev/fuchsia-src/reference/kernel_objects/channel, primary, checked 2026-11-01. Handles move atomically with a message; ordered queue per endpoint; unread messages are destroyed with the endpoint and their handles closed. The page states no queue capacity.
- [Z2] Fuchsia project, zx_channel_write, https://fuchsia.dev/fuchsia-src/reference/syscalls/channel_write, primary, checked 2026-11-01. 64 handles and 65,536 bytes per message; byte limit "not yet finalized"; every listed handle is gone from the caller on success and on failure; a channel's own handle cannot be sent; handles need `ZX_RIGHT_TRANSFER`.
- [Z3] Fuchsia project, zx_channel_read, https://fuchsia.dev/fuchsia-src/reference/syscalls/channel_read, primary, checked 2026-11-01. `ZX_ERR_BUFFER_TOO_SMALL` reports the needed sizes for a retry; empty channel: `ZX_ERR_SHOULD_WAIT`, or `ZX_ERR_PEER_CLOSED` with the peer gone.

## Memory objects

- [Z4] Fuchsia project, Virtual Memory Object, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_object, primary, checked 2026-11-01. A VMO can be mapped into many address spaces and is the standard way to share memory; pages commit on first access.

## Interface definitions

- [F1] Fuchsia project, FIDL wire format, https://fuchsia.dev/fuchsia-src/reference/fidl/language/wire-format, primary, checked 2026-11-01. Header with txid, flags, magic and a 64-bit ordinal; encoded form has no pointers or handles; handles travel out of band; validation is required when decoding from a peer; depth limit 32; zero padding checked; tables use envelopes so a reader can skip the unknown.
- [C1] Cap'n Proto, Encoding, https://capnproto.org/encoding.html, primary, checked 2026-11-01. Messages hold relative pointers; capabilities are indices into a table; validation is lazy, with a traversal budget and a depth limit.

## Drivers

- [Z5] Fuchsia project, Bus Transaction Initiator, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/bus_transaction_initiator, primary, checked 2026-11-01. Pinning gives device-physical addresses; an un-released pin is quarantined so that DMA cannot reach reallocated memory.
- [Z6] Fuchsia project, Interrupts, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/interrupts, primary, checked 2026-11-01. Interrupt objects for user drivers; bound to a port, an IRQ arrives as a packet and must be acknowledged before the next.
- [G1] Genode Labs, Genode Foundations 24.05, Device drivers, https://genode.org/documentation/genode-foundations/24.05/components/Device_drivers.html, primary, checked 2026-11-01. Without an IOMMU a user-level driver can program any physical address, so drivers and devices must be trusted; with one, each device is limited to its mappings.
- [L1] Linux kernel documentation, VFIO, https://www.kernel.org/doc/html/latest/driver-api/vfio.html, primary, checked 2026-11-01. DMA is the most dangerous part of a device interface; VFIO needs an IOMMU, with an explicit unsafe no-IOMMU mode; UIO has no notion of IOMMU protection.
- [H1] Oxide Computer, Hubris reference, https://hubris.oxide.computer/reference/, primary, checked 2026-11-01. Drivers live in tasks; an interrupt is a notification bit, masked until the driver re-enables it; a task id carries a generation, so a client of a restarted server gets an error and learns of the restart; a reply to a dead task is dropped; leases lend memory for a call.

## Virtio and QEMU

- [V1] OASIS, Virtual I/O Device (VIRTIO) Version 1.2, https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html, primary, checked 2026-11-01. Section 4.2: MagicValue 0x74726976, Version 2, DeviceID, the queue address registers as 64-bit physical addresses, QueueNotify, InterruptStatus, InterruptACK. Section 2: split virtqueue areas, physically contiguous; descriptors hold guest-physical addresses.
- [Q1] QEMU project, 'virt' generic virtual platform, https://www.qemu.org/docs/master/system/arm/virt.html, primary, checked 2026-11-01. `iommu=none` is the default; `iommu=smmuv3` applies to all devices in the PCIe topology; 32 virtio-mmio transports by default.

## Not relied on

- The Zircon page on driver architectures and the QNX 8.0 message-passing page gave 404 on 2026-11-01. QNX claims here rest on `alternatives-postmortem.md#qnx`.
- `cs.vu.nl` did not resolve, so the MINIX 3 paper was not re-read; MINIX claims rest on `alternatives-postmortem.md#minix-3`.
- The FIDL compatibility guide is marked outdated by its maintainers and is not cited.
- The Redox "schemes and resources" page and the Genode Foundations contents page did not answer the questions asked. Redox claims rest on `alternatives-postmortem.md#redox`.
- The definition of `VIRTIO_F_ACCESS_PLATFORM` was not found in the parts of [V1] that were read: `[unverified]`, and ADR-0054 treats it so.
