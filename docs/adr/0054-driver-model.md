# ADR-0054: Driver model

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0006, ADR-0010, ADR-0012, ADR-0015, ADR-0024, ADR-0031, ADR-0034, ADR-0040, ADR-0041, ADR-0045, ADR-0050, ADR-0051, ADR-0053
- **Evidence:** DM-023, DM-046, DM-047, DM-161, alternatives-postmortem.md#minix-3, alternatives-postmortem.md#redox, stage-4/sources.md#drivers, stage-4/sources.md#virtio-and-qemu

## Context
Stage 4 needs a virtio-blk driver. ADR-0015 fixes the device contract, and ADR-0012 puts the filesystem in a process of its own. QEMU's `virt` offers 32 virtio-mmio transports, 0x200 bytes apart, so eight share a 4 KiB page (`stage-4/sources.md#virtio-and-qemu`), and an SMMUv3 only with `iommu=smmuv3`, which covers the PCIe topology (`stage-4/sources.md#virtio-and-qemu`). So a virtio-mmio device is not behind any IOMMU. Virtio queue addresses and descriptors hold physical addresses (`stage-4/sources.md#virtio-and-qemu`), so whoever writes them chooses what the device reads and writes.

## Decision drivers
1. Driver faults are a large cause of crashes, though DM-047 warns that its 85% figure is one unpublished anecdote: a bug should end a process, not the kernel.
2. State honestly what isolation is real without an IOMMU (DM-161).
3. Fit the failure rule: ADR-0010 makes device failure fail-stop and sticky.
4. Small kernel additions, each testable.
5. Replacing physical addresses with translated ones later must not change the driver's interface.

## Considered options

### Option A: In the kernel (Linux)
- **Description:** A kernel module speaks to the device, and the filesystem service calls it.
- **Evidence for:** Fastest path, no new objects. With no IOMMU, a driver can reach all memory anyway (`stage-4/sources.md#drivers`).
- **Evidence against:** A bug is a kernel fault (DM-047, ADR-0024), and hostile device data would be parsed in the kernel (ADR-0015 rule 8).
- **Cost:** *(estimate)* The least code at stage 4, and the wrong direction for later drivers.
- **Precedent:** Linux, with DM-046's contested record.

### Option B: In user space, trusted for DMA
- **Description:** A process with device memory, an interrupt and DMA buffers. It writes physical addresses into queues itself.
- **Evidence for:** MINIX 3 recovered from driver crashes, and its user-mode drivers cost 5-10% against in-kernel ones, unoptimised and self-reported (`alternatives-postmortem.md#minix-3`). Redox, Hubris and Fuchsia run drivers as tasks (`alternatives-postmortem.md#redox`, `stage-4/sources.md#drivers`).
- **Evidence against:** Fault isolation only, not security. Without an IOMMU a malicious driver can program any physical address, as Genode's documentation and Linux's VFIO say plainly (`stage-4/sources.md#drivers`, DM-161).
- **Cost:** *(estimate)* Three kernel objects, 500 lines, and a driver of about 600.
- **Precedent:** MINIX 3, Redox, Fuchsia.

### Option C: In user space, with the kernel writing the queues
- **Description:** The driver names buffers by handle, and a kernel component validates them and writes the descriptors.
- **Evidence for:** Real protection without an IOMMU on this one device type.
- **Evidence against:** The kernel then holds virtio knowledge: most of a driver, per device type. It defeats the point.
- **Cost:** *(estimate)* A kernel virtqueue layer.
- **Precedent:** None in the files.

### Option D: In user space, behind SMMUv3 now
- **Description:** Use `virtio-blk-pci`, `iommu=smmuv3`, and a kernel SMMU driver with per-device translation.
- **Evidence for:** Real DMA isolation (`stage-4/sources.md#drivers`), and the driver gets translated addresses.
- **Evidence against:** A PCIe host bridge, enumeration and an SMMUv3 driver in the kernel, for a virtual machine; DM-161 names virtio-iommu as an alternative, not evaluated here. Unless `VIRTIO_F_ACCESS_PLATFORM` is negotiated, a device has the same access to memory as the driver, and a device only "MAY" refuse to work when a driver declines the feature (`stage-4/sources.md#virtio-and-qemu`), so Option D's isolation depends on the device refusing. What QEMU does was not tested.
- **Cost:** *(estimate)* A stage of its own.
- **Precedent:** Fuchsia BTIs (`stage-4/sources.md#drivers`), Linux VFIO.

## Recommendation
Recommended: **Option B**, with an interface that lets Option D replace it. It is a recommendation, not a decision.

- **Device object.** One per page group of MMIO ranges from the device tree, made by the kernel at boot and given to `init`. Transports that share a page cannot be separated with ADR-0020's granule, so one driver serves them all and sees all their registers. `NotWaitable`. Rights: `MAP`, `INTERRUPT`, `DMA`, `DUPLICATE`, `TRANSFER`. `init` keeps a handle, so a restart has something to hand over.
- **Mapping.** `device_map(device, index, out)` maps a page-rounded window uncached (Device-nGnRnE), never executable, and writes the address and the range's offset inside the window to an output buffer, as ADR-0032 leaves one value to x1. One holder at a time: `BadState` until the last mapping goes.
- **Interrupt object.** `device_interrupt(device, index)` makes one. Waitable, with signal `INTERRUPT`: subscribed through `port_subscribe`, so ADR-0045's lint is enough. It stays masked once asserted, until `interrupt_ack`, as Zircon's and Hubris's do (`stage-4/sources.md#drivers`), so a stuck driver cannot storm the kernel. Closing it masks the line first: a device not yet reset could otherwise raise an interrupt with no handler, which ADR-0024 makes a panic. Rights: `WAIT`, `WRITE` for the ack, `TRANSFER`.
- **DMA.** `dma_pin(memory, device)` needs `DMA` on the device and `WRITE` on the memory object. It pins a memory object's frames and returns *device addresses*. `dma_unpin` ends a pin. A pin is `NotWaitable`, with `TRANSFER`, and holds the memory object (ADR-0051). On `virt` they equal physical addresses, but the driver treats them as opaque, which is what lets a future IOMMU change them (driver 5).
- **Quarantine.** When a pin's holder dies, its frames are not freed, because the device may still write them. They stay quarantined. The mechanism is Zircon's (`stage-4/sources.md#drivers`); the rule that the successor resets the device, sees status 0, and then calls `dma_release_quarantine(device)` is this ADR's.
- **The driver** is a process holding those three, a channel to the filesystem service and a supervisor channel from `init`. It validates every length and index the device returns (ADR-0015 rule 8).
- **Trust boundary, stated.** On QEMU `virt` with virtio-mmio there is no IOMMU. A malicious or corrupted driver can make the device read or overwrite any physical memory: kernel, other processes, secrets. Option B gives isolation against *bugs* that do not reach the descriptors, not against an attacker in the driver. The driver is in the trusted computing base. `iommu=smmuv3` does not change this for virtio-mmio, because it applies to the PCIe topology. A driver with device mappings is also the one program that can cause an external abort, and ADR-0034 keeps SError from EL0 a kernel fault, so such a bug can stop the kernel. INV-0031-2 is false for a driver through DMA; its wording needs a superseding ADR.
- **Block protocol.** Declared with ADR-0052: `info` (sector count, flush support), `read`, `write` (a batch, failing whole, as `BlockDevice::write` does) and `flush`, with data in a memory object attached by the service. The driver treats the service's messages as hostile: a crafted image can compromise the service (ADR-0012 rule 4), and the driver is in the trusted base, so this is the path that makes a total compromise real.
- **Driver death.** It is `PEER_CLOSED` at the filesystem service, seen by the loop whether or not a request is in flight. The service cannot know which writes reached the device, so it calls a new public `Volume::device_lost()`, with its own failure reason, which treats this as ADR-0015 rule 6's failed flush: `Failed`, sticky, cleared only by remount. `zoraya-fs` gains this one method. A device read that fails inside the engine is `Corrupt` and adds to `corrupt_reads`, whatever the cause. So the block adapter records the loss, and from then on fails every read at once without a message. The service replies `StorageFailed` to any request that gave `Corrupt` after the loss, a read in flight included, and reports the corruption count less those reads, so an outage is not counted as corruption. An uncached read after the loss gets `StorageFailed` too. Unlike MINIX 3's retry, nothing is retried.
- **Hangs.** A block request has a deadline, 30 s `[assumption]`, in the service's port wait, and missing it counts as device loss. The service reports the loss on its supervisor channel. A hung driver is otherwise invisible to `init`, and it still holds its mapping, so `init` kills the driver's job on that report, before anything below: a successor would otherwise get `BadState` from `device_map`.
- **Restart order.** Death is not a crash: ADR-0015's model ends all writes, and a dead driver's device goes on. So `init` ends the service first, if it lives. The new driver resets the device, waits for status 0, after which the device touches no queue, and only then accepts a client. Only then does `init` start the service, which remounts to the durable prefix (ADR-0010 rule 3). `Volume::mount` flushes first, which ADR-0011 does not state and this order relies on.
- **Service death with a live driver.** The service's channel closes with writes in flight. The driver answers or abandons them, resets the device as above, and only then accepts a new client on a channel from `init`, sent over its supervisor channel. Otherwise an old write could land on a block the new mount reallocated.
- **What clients see.** One with a request in flight sees `PEER_CLOSED`. After `Failed` and before the service is replaced, mutating requests get `StorageFailed`, and reads are served only from what is cached, a limit on ADR-0010 rule 6. Running clients are not recovered.

**Tensions with accepted ADRs.** *ADR-0010 rule 6:* a new `Failed` trigger, and reads limited to the cache. *ADR-0015 rule 6:* "every request completes" fails for a dead driver, so the adapter invents `IoError`, and "any read error as `Corrupt`-equivalent" is overridden by the service after device loss, above. Both need a superseding ADR-0015. *ADR-0015 rules 3 and 4:* no text on writes outstanding at a reset. This ADR assumes that writes outstanding at a completed reset count as completed for rule 4; virtio does not say so, and a superseding ADR-0015 must. *ADR-0020:* granule, above. *ADR-0024, ADR-0034:* above. *ADR-0006 rule 1:* new types and an `INTERRUPT` signal. *ADR-0031:* above.

### Effect of the vision
This is where the vision matters most. For target 2, a driver in the trusted base is acceptable only if it holds no attacker's code and takes hostile data carefully: the sandboxed agent talks to the filesystem service, not to the driver, so this boundary is not directly exposed. But a compromise of the driver, for instance through a filesystem image, is a total compromise. Target 2 therefore needs Option D, or a design where untrusted code cannot influence a driver, before any claim of strict isolation, and even that is necessary, not sufficient: DM-161 records that an enabled IOMMU is still not enough. Changing later costs a PCIe bridge, an SMMUv3 driver and a move to `virtio-blk-pci`; the block and queue logic stays, and the transport is rewritten. Networking follows the same model with its own device objects.

### Deferred to stage 6
Nothing in this ADR depends on POSIX or `fork`: only the filesystem service speaks to the driver. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** A driver bug that neither corrupts DMA nor raises an external abort ends a process, and recovery is a remount. Three small objects.
- **Negative.** No security gain over Option A against a compromised driver. Quarantined frames stay lost until a new driver runs. Every I/O crosses processes.
- **DMA isolation.** Isolating DMA requires PCIe and SMMUv3. It will be a separate ADR, if the direction of isolating untrusted code is chosen; this ADR does not provide it.
- **Follow-up.** `tools/run-qemu.sh` gains a disk option.

## Invariants
- **INV-0054-1.** A device mapping is never executable and never Normal memory. *Check:* in-QEMU table walk.
- **INV-0054-2.** An unacknowledged interrupt gives no second packet, and closing an interrupt object masks its line. *Check:* in-QEMU test with a test-kernel-only pending write to a spare interrupt line.
- **INV-0054-3.** Frames pinned by a dead driver are not reallocated before `dma_release_quarantine`. *Check:* in-QEMU test killing a holder and comparing frame counts.
- **INV-0054-4.** The driver treats every device-supplied index and length, and every message from the service, as untrusted. *Check:* host tests against a fake device returning out-of-range values, and decoder tests of the block protocol.
- **INV-0054-5.** A driver's death moves the volume to `Failed`, clean or dirty, idle or busy. *Check:* in-QEMU tests killing the driver when idle with a clean volume, idle with a dirty one, during a commit and during a data read.
- **INV-0054-6.** A restart after the death of a driver or a service keeps ADR-0010 rule 3. *Check:* in-QEMU restart test, then `zoraya-fsck` and a compare with the durable prefix.

## Open questions
- QEMU's behaviour when a driver clears `VIRTIO_F_ACCESS_PLATFORM`, and whether virtio-iommu is cheaper than an SMMUv3 driver: test before Option D.
- Cost of a block round trip through two processes.
- Whether the kernel should reset the device itself when a driver dies, which needs device-specific knowledge.

## Revisit triggers
- The architect commits to target 2 of the vision.
- A driver for real hardware.
- A measured I/O path dominated by the driver's process boundary.

## Architect's decision
