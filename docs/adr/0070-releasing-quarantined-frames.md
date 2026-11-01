# ADR-0070: Releasing quarantined DMA frames

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0021, ADR-0024, ADR-0051, ADR-0053, ADR-0054
- **Evidence:** DM-161, stage-4/sources.md#drivers, stage-4/sources.md#virtio-and-qemu

## Context
ADR-0054 keeps a pin's frames quarantined when its last handle goes without `dma_unpin`, and has "the successor" reset the device and then call `dma_release_quarantine`. Nothing proves the reset. The call only trusts its caller, and INV-0054-7 and backlog item W4 ask who may end a quarantine while a driver runs. With no IOMMU (ADR-0054) a device writes wherever its queues say, so frames freed too early are memory corruption anywhere in the kernel or another process. The kernel cannot see what a driver does to a mapped register.

## Decision drivers
1. A running driver must not be able to free frames its device may still write (DM-161 has why DMA is the dangerous path).
2. What ends a quarantine must be something the kernel observed, not a claim.
3. Small kernel knowledge: ADR-0054 keeps device logic out of the kernel.
4. A restart must be possible with no reboot (ADR-0054).

## Considered options

### Option A: Trust the caller (Zircon)
- **Description:** A call with the device's right ends the quarantine. `zx_bti_release_quarantine` has this shape, and its page does not say who calls it (`stage-4/sources.md#drivers`).
- **Evidence for:** No device knowledge in the kernel. It is ADR-0054's text.
- **Evidence against:** A running driver with the right can release frames while its device writes them. Against drivers 1 and 2.
- **Cost:** *(estimate)* 20 lines.
- **Precedent:** Fuchsia.

### Option B: Never release until reboot
- **Description:** Quarantined frames are lost for this boot.
- **Evidence for:** Safe, and needs nothing.
- **Evidence against:** Each restart leaks its queues and buffers, against driver 4.
- **Cost:** A leak per driver death.
- **Precedent:** None in the files.

### Option C: An IOMMU unmaps the device (Linux VFIO)
- **Description:** Translation is removed, and the device can then reach nothing (`stage-4/sources.md#drivers`).
- **Evidence for:** The real answer, with no trust in anyone.
- **Evidence against:** `virt` puts virtio-mmio behind no IOMMU (`stage-4/sources.md#virtio-and-qemu`), and ADR-0054 defers Option D.
- **Cost:** *(estimate)* A stage of its own.
- **Precedent:** Linux.

### Option D: The kernel resets the device and observes it
- **Description:** The release call needs the device to have no mapping, so no process can restart it, and then the kernel itself writes 0 to each virtio-mmio transport's Status register and reads back 0. The virtio specification says that after a reset the device touches no queue (`stage-4/sources.md#virtio-and-qemu`).
- **Evidence for:** Drivers 1, 2 and 4. Only two registers of device knowledge, so driver 3 holds.
- **Evidence against:** The kernel knows one bus's reset. A device that ignores reset still writes: virtio's own contract is what is trusted, as ADR-0015 rule 8 and ADR-0054 already do for a device's behaviour otherwise. Another device type needs its own reset before it can be released.
- **Cost:** *(estimate)* 60 lines.
- **Precedent:** ADR-0054's open question names it.

## Recommendation
Recommended: **Option D**. It is a recommendation, not a decision.

- **The call.** `dma_release_quarantine(device)` needs `DMA` on the device. Any holder may call it, a running driver too, because what it asks for is safe whoever asks.
- **Refused while unsafe.** While the device has a mapping, or a live pin, the call is `BadState` and nothing changes. A mapping could restart the device after the reset, and a live pin belongs to a driver that is still using its device.
- **The reset.** With neither, the kernel, holding the device's lock so that no mapping can begin, writes 0 to Status at offset 0x70 of every transport in the device's window whose MagicValue and Version say virtio-mmio, and polls for 0, up to 100 ms `[assumption]`. A transport that does not read back 0 is `DeadlineExceeded`, and the frames stay quarantined.
- **The result.** Only then do the quarantined frames return, and the memory objects they held go. A device with no virtio transport stays quarantined: `BadState`.
- **Order at a restart.** The successor calls the release before `device_map`. This replaces the order in ADR-0054's Quarantine bullet, where the successor resets the device itself first.

**Tensions with accepted ADRs.** *ADR-0054 Quarantine:* the order above, and the kernel gains a reset, which its open question left open. If this ADR is accepted, that bullet is partly replaced. *ADR-0053:* the restart order gains this call. *ADR-0024:* a device that faults on a kernel access is a kernel fault; the window is one the kernel mapped from the device tree, so this is the same risk as the UART's.

### Effect of the vision
For target 2 this keeps a driver from corrupting memory through the release path, but a driver still reaches all memory through its queues (ADR-0054), so this is a defence against bugs and restarts, not against a malicious driver. Moving to an IOMMU later replaces the reset with an unmap, behind the same call.

### Deferred to stage 6
Nothing here depends on POSIX. ADR-0055 holds the one requirement on stage 4.

## Consequences
- **Positive.** No process, however buggy, can free frames a live device may write through this call.
- **Negative.** The kernel holds a little virtio knowledge. A release resets every transport in a window, so a driver sharing a window with another device's driver cannot release alone; ADR-0054 already has one driver per window.
- **Follow-up.** A reset for each new device type.

## Invariants
- **INV-0070-1.** `dma_release_quarantine` frees nothing while the device has a mapping or a live pin. *Check:* in-QEMU test of each refusal, comparing frame counts.
- **INV-0070-2.** Quarantined frames return only after the kernel has read Status 0 from every transport in the window. *Check:* in-QEMU test that sets a transport's Status, releases, and reads 0 and the frame count.
- **INV-0070-3.** A device with no virtio transport keeps its quarantine. *Check:* in-QEMU test with a device object that has no transport.

## Open questions
- Whether 100 ms is enough for every virtio device: nothing measured.

## Revisit triggers
- A device type with no observable reset.
- An IOMMU path (ADR-0054 Option D).

## Architect's decision
