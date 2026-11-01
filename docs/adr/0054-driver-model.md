# ADR-0054: Driver model

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0006, ADR-0010, ADR-0015, ADR-0024, ADR-0031, ADR-0050, ADR-0051, ADR-0053
- **Evidence:** DM-046, DM-047, DM-161, alternatives-postmortem.md#minix-3, alternatives-postmortem.md#redox, stage-4/sources.md#drivers, stage-4/sources.md#virtio-and-qemu

## Context
Stage 4 needs a virtio-blk driver. ADR-0015 fixes the device contract, and ADR-0012 puts the filesystem in a process of its own. QEMU's `virt` offers 32 virtio-mmio transports, and an SMMUv3 only with `iommu=smmuv3`, which covers the PCIe topology (`stage-4/sources.md#virtio-and-qemu`). So a virtio-mmio device is not behind any IOMMU. Virtio queue addresses and descriptors hold physical addresses (`stage-4/sources.md#virtio-and-qemu`), so whoever writes them chooses what the device reads and writes.

## Decision drivers
1. Driver faults are a dominant cause of crashes (DM-047): a bug should end a process, not the kernel.
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
- **Evidence for:** MINIX 3 recovered from driver crashes with 5-10% overhead (`alternatives-postmortem.md#minix-3`). Redox, Hubris and Fuchsia run drivers as tasks (`alternatives-postmortem.md#redox`, `stage-4/sources.md#drivers`).
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
- **Evidence against:** A PCIe host bridge, enumeration and an SMMUv3 driver in the kernel, for a virtual machine. Whether virtio devices honour it needs `VIRTIO_F_ACCESS_PLATFORM`, whose text was not verified here `[unverified]`.
- **Cost:** *(estimate)* A stage of its own.
- **Precedent:** Fuchsia BTIs (`stage-4/sources.md#drivers`), Linux VFIO.

## Recommendation
Recommended: **Option B**, with an interface that lets Option D replace it. It is a recommendation, not a decision.

- **Device memory object.** One MMIO range of the device tree, made by the kernel at boot and given to `init`. `device_map` maps it uncached (Device-nGnRnE), never executable. It carries one right, `MAP`, and can be moved, not duplicated.
- **Interrupt object.** One interrupt. Bound to a port by key, it gives a packet per interrupt, masked until `interrupt_ack`, as Zircon's and Hubris's do (`stage-4/sources.md#drivers`). An unacknowledged interrupt stays masked, so a stuck driver cannot storm the kernel.
- **DMA.** `dma_pin(memory, device)` pins a memory object's frames, contiguous when asked, and returns *device addresses*. On `virt` they equal physical addresses, but the driver treats them as opaque, which is what lets a future IOMMU change them (driver 5).
- **Quarantine.** When a pin's holder dies, its frames are not freed, because the device may still write them. They stay quarantined until the driver's successor resets the device and calls `dma_release_quarantine`, as Zircon does (`stage-4/sources.md#drivers`).
- **The driver** is a process holding those three and a channel to the filesystem service. It validates every length and index the device returns (ADR-0015 rule 8).
- **Trust boundary, stated.** On QEMU `virt` with virtio-mmio there is no IOMMU. A malicious or corrupted driver can make the device read or overwrite any physical memory: kernel, other processes, secrets. Option B gives isolation against *bugs* that do not reach the descriptors, not against an attacker in the driver. The driver is in the trusted computing base. `iommu=smmuv3` does not change this for virtio-mmio, because it applies to the PCIe topology.
- **Restart.** A driver's death is `PEER_CLOSED` at the filesystem service. It cannot know which writes reached the device or whether a flush completed, so it treats this as ADR-0015 rule 6's failed flush: the volume enters `Failed`, sticky, and only a remount clears it. Unlike MINIX 3's retry, nothing is retried. `init` restarts the driver, then the service, which remounts to the durable prefix (ADR-0010 rule 3). Clients see `StorageFailed`, then `PEER_CLOSED`.

### Effect of the vision
This is where the vision matters most. For target 2, a driver in the trusted base is acceptable only if it holds no attacker's code and takes hostile data carefully: the sandboxed agent talks to the filesystem service, not to the driver, so this boundary is not directly exposed. But a compromise of the driver, for instance through a filesystem image, is a total compromise. Target 2 therefore needs Option D, or a design where untrusted code cannot influence a driver, before any claim of strict isolation. Changing later costs a PCIe bridge, an SMMUv3 driver and a move to `virtio-blk-pci`; the driver's use of opaque device addresses keeps its code the same. Networking follows the same model with its own device objects.

## Consequences
- **Positive.** A driver bug that does not corrupt DMA ends a process, and recovery is a remount. Three small objects.
- **Negative.** No security gain over Option A against a compromised driver. Quarantined frames stay lost until a new driver runs. Every I/O crosses processes.
- **Follow-up.** `tools/run-qemu.sh` gains a disk option. The architect decides whether Option D belongs to the roadmap.

## Invariants
- **INV-0054-1.** A device mapping is never executable and never Normal memory. *Check:* in-QEMU table walk.
- **INV-0054-2.** An unacknowledged interrupt gives no second packet. *Check:* in-QEMU test with a virtual interrupt, or a timer-backed test line.
- **INV-0054-3.** Frames pinned by a dead driver are not reallocated before `dma_release_quarantine`. *Check:* in-QEMU test killing a holder and comparing frame counts.
- **INV-0054-4.** The driver treats every device-supplied index and length as untrusted. *Check:* host tests against a fake device returning out-of-range values.
- **INV-0054-5.** A driver's death moves the volume to `Failed`. *Check:* in-QEMU test killing the driver in the middle of a commit.

## Open questions
- `VIRTIO_F_ACCESS_PLATFORM`'s exact meaning, and QEMU's behaviour with it: read the specification's reserved-feature section before Option D.
- Cost of a block round trip through two processes.
- Whether the kernel should reset the device itself when a driver dies, which needs device-specific knowledge.

## Revisit triggers
- The architect commits to target 2 of the vision.
- A driver for real hardware.
- A measured I/O path dominated by the driver's process boundary.

## Architect's decision
