# Sources for stage 4 design

New sources for TASK-10. Each URL was fetched on the check date and the note says what it was read for. Sources already in [`../sources.md`](../sources.md) (S-numbers) are cited through `alternatives-postmortem.md` and are not repeated. Fetches that failed or that did not answer are listed at the end, so nothing below rests on them.

## Channels

- [Z1] Fuchsia project, Channel (kernel object), https://fuchsia.dev/fuchsia-src/reference/kernel_objects/channel, primary, checked 2026-11-01. Handles move atomically with a message; ordered queue per endpoint; unread messages are destroyed with the endpoint and their handles closed. The page states no queue capacity; [Z2] warns that undrained channels use excessive kernel memory.
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
- [G1] Genode Labs, Genode Foundations 24.05, Device drivers, https://genode.org/documentation/genode-foundations/24.05/components/Device_drivers.html, primary, checked 2026-11-01. Without an IOMMU a user-level driver can program any physical address, so drivers and devices should ideally be trustworthy; with one, each device is limited to its mappings.
- [L1] Linux kernel documentation, VFIO, https://www.kernel.org/doc/html/latest/driver-api/vfio.html, primary, checked 2026-11-01. DMA is the most dangerous part of a device interface; VFIO needs an IOMMU, with an explicit unsafe no-IOMMU mode; UIO has no notion of IOMMU protection.
- [H1] Oxide Computer, Hubris reference, https://hubris.oxide.computer/reference/, primary, checked 2026-11-01. Drivers live in tasks; an interrupt is a notification bit, masked until the driver re-enables it; a task id carries a generation, so a client of a restarted server gets an error and learns of the restart; a reply to a dead task is dropped; leases lend memory for a call.

## Virtio and QEMU

- [V1] OASIS, Virtual I/O Device (VIRTIO) Version 1.2, https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html, primary, checked 2026-11-01. Section 4.2: MagicValue 0x74726976, Version 2, DeviceID, the queue address registers as 64-bit physical addresses, QueueNotify, InterruptStatus, InterruptACK. Section 2: split virtqueue areas, physically contiguous; descriptors hold guest-physical addresses. Section 6, read by the independent review in the v1.2 cs01 text: without `VIRTIO_F_ACCESS_PLATFORM` the device has the same access to memory as the driver, and a device MAY fail to operate if the feature is not accepted.
- [Q1] QEMU project, 'virt' generic virtual platform, https://www.qemu.org/docs/master/system/arm/virt.html, primary, checked 2026-11-01. `iommu=none` is the default; `iommu=smmuv3` applies to all devices in the PCIe topology; 32 virtio-mmio transports by default.
- [Q2] QEMU project, hw/arm/virt.c, https://gitlab.com/qemu-project/qemu/-/blob/master/hw/arm/virt.c, primary, read by the independent review 2026-11-01. `[VIRT_MMIO] = { 0x0a000000, 0x00000200 }`: transports are 0x200 bytes apart.

## Services

- [Z7] Fuchsia project, fuchsia.io, https://fuchsia.dev/reference/fidl/fuchsia.io, primary, read by the independent review 2026-11-01. Each open object is a channel; `GetToken` and `Rename` with a destination token valid while the connection stays open; the seek offset is per connection, and clones do not share it.

## Quotas

- [K1] Linux kernel documentation, Control Group v2, https://docs.kernel.org/admin-guide/cgroup-v2.html, primary, checked 2026-11-01. `memory.max` is the hard limit, and if usage cannot be reduced the OOM killer runs inside the cgroup; `memory.high` throttles and never invokes it; restrictions closer to the root cannot be overridden further down; kernel data structures, stacks and page tables are tracked. Only the first 100,000 characters of the page were read.
- [Z8] Fuchsia project, Job, https://fuchsia.dev/fuchsia-src/reference/kernel_objects/job, primary, checked 2026-11-01. Jobs form a tree and are said to track and limit basic resource consumption; the page does not say how.
- [Z9] Fuchsia project, zx_job_set_policy, https://fuchsia.dev/fuchsia-src/reference/syscalls/job_set_policy, primary, checked 2026-11-01. Policies allow, deny or kill on conditions such as `ZX_POL_NEW_VMO`, and combine with the parent's. The page gives no memory quota.
- [G2] Genode Labs, Genode Foundations 24.05, Resource trading, https://genode.org/documentation/genode-foundations/24.05/architecture/Resource_trading.html, primary, checked 2026-11-01. A parent moves part of its quota to each child's account; clients pay for server-side allocations through session quota, returned on close.

## Not relied on

- The Zircon page on driver architectures and the QNX 8.0 message-passing page gave 404 on 2026-11-01. QNX claims here rest on `alternatives-postmortem.md#qnx`.
- `cs.vu.nl` did not resolve, so the MINIX 3 paper was not re-read; MINIX claims rest on `alternatives-postmortem.md#minix-3`.
- The FIDL compatibility guide is marked outdated by its maintainers and is not cited.
- The seL4 manual page holds only links to PDFs and did not answer; seL4 claims rest on `alternatives-postmortem.md#l4-family-and-sel4`.
- The Redox "schemes and resources" page and the Genode Foundations contents page did not answer the questions asked. Redox claims rest on `alternatives-postmortem.md#redox`.
