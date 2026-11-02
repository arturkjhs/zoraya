# ADR-0091: Identifying virtio devices and giving each driver only its own

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0020, ADR-0053, ADR-0054, ADR-0070, ADR-0080
- **Evidence:** DM-009, CP-01, CP-17, alternatives-postmortem.md#genode, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, stage-4/sources.md#virtio-and-qemu, stage-4/sources.md#drivers

## Context
ADR-0054 makes one `Device` object per 4 KiB page of virtio-mmio transports, because eight transports share a page and the granule cannot separate them (`stage-4/sources.md#virtio-and-qemu`). Whoever maps the page sees all eight register blocks. Stage 4 had one device. Stage 5 adds a GPU, a keyboard and a tablet. QEMU fills transports from the highest address down (transport 31 first, measured with `info qtree` on QEMU 11.1.2), so four devices given in the default order sit in one page, and one `Device` handle would give a single driver all four. `init` also needs to know which device is which: the kernel names none of them (ADR-0053). Measured: with `bus=virtio-mmio-bus.N` on each `-device`, QEMU places the device at transport N, at `0x0a000000 + N × 0x200` (`info qtree`, QEMU 11.1.2).

## Decision drivers
1. A driver holds only its own device (CP-01, DM-009): a bug in the keyboard driver must not reach the disk's registers.
2. No device knowledge in the kernel beyond ADR-0070's reset (ADR-0054).
3. A driver dies alone and restarts alone (CP-17), which needs the device's mapping to have one holder (ADR-0054's `BadState`).
4. Small.

## Considered options

### Option A: Bus enumeration and driver binding in the kernel (Linux)
- **Description:** The kernel probes each transport, reads its device ID and binds a kernel driver by table.
- **Evidence for:** Automatic, with no placement rules.
- **Evidence against:** The drivers are in the kernel, against ADR-0054, and binding becomes kernel policy.
- **Cost:** *(estimate)* A driver core in the kernel.
- **Precedent:** Linux's device model.

### Option B: One driver per page
- **Description:** Keep ADR-0054 as written: one process serves every device on a page.
- **Evidence for:** No change anywhere.
- **Evidence against:** With QEMU's default placement that one process is disk, display and input together, against drivers 1 and 3: a keyboard bug ends the disk.
- **Cost:** None now; a merged driver later.
- **Precedent:** ADR-0054's own statement for shared pages.

### Option C: One device per page, placed on the command line; `init` identifies
- **Description:** `tools/run-qemu.sh` puts each GUI device on a transport of its own page with `bus=`. `init` maps each device page once, reads every transport's MagicValue, Version and DeviceID, and gives a driver a page only if exactly one transport in it answers with a device. It hands that page's `Device` handle with `MAP`, `INTERRUPT` and `DMA`, and the transport's range index, to the driver for that device ID.
- **Evidence for:** Drivers 1 to 4, with no kernel change. The parent routes device resources, as Genode's parent tree does (`alternatives-postmortem.md#genode`).
- **Evidence against:** Isolation depends on the command line. A board that packs devices into one page gets Option B's merged driver or no driver at all: `init` refuses such a page.
- **Cost:** *(estimate)* 60 lines in a host-tested function and three `bus=` options.
- **Precedent:** Genode's platform driver hands each driver its own device resources `[outside evidence: Genode Foundations, platform driver]`.

### Option D: Register access by system call for shared pages
- **Description:** A `Device` object per transport, with `device_read` and `device_write` calls that check the offset against the transport's 0x200 bytes. No mapping.
- **Evidence for:** Real separation on any placement. Zircon gives drivers resources per device (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, `stage-4/sources.md#drivers`).
- **Evidence against:** A system call per register access, and every notify. New kernel calls and a second access path beside ADR-0054's mapping.
- **Cost:** *(estimate)* 150 lines in the kernel and a second `Registers` implementation.
- **Precedent:** Port I/O permission bitmaps on x86 `[outside evidence]`.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Placement** (ADR-0097). The disk stays where QEMU puts it, transport 31 (page 3). The GPU is on transport 23 (page 2), the keyboard on 15 (page 1), the tablet on 7 (page 0).
- **Identification.** A pure function over a page's eight register reads, in a host-tested crate: for each transport, `MagicValue` must be `0x74726976` and `Version` 2; `DeviceID` 0 is an empty transport. It gives the page's one device as `(range, device ID)`, or nothing if the page has none or more than one. Known IDs: 2 block, 16 GPU, 18 input. `init` reads each page with one mapping, which it ends before handing the handle on.
- **Hand-over.** The driver gets a duplicate with `MAP | INTERRUPT | DMA`, never `DUPLICATE` or `TRANSFER`, and its range as an argument. `init` keeps its own handle for a restart (ADR-0053).
- **Refusal.** A page with two devices is reported on the console and gets no driver. The disk follows the same rule, which changes nothing for `make run`, where it is alone.

**Tensions with accepted ADRs.** *ADR-0054, Device object:* "one driver serves them all" stays true of the object; this ADR makes the shared page a configuration `init` refuses rather than one it serves.

### Effect of the vision
None on the choice for targets 1 and 3. For target 2, where drivers are part of the trusted base, separating them by page keeps one device's bug away from the others, which matters more there; but it is only fault isolation, as ADR-0054 says of DMA. Moving to Option D later costs two kernel calls and a `Registers` implementation, and nothing in a driver's logic changes, because drivers already use the `Registers` trait. Networking adds a device ID and a page.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** Each driver holds one device. `init`'s choice is a tested function.
- **Negative.** The command line carries an isolation property. A user who adds a device by hand without `bus=` gets a refusal, not a merged driver.
- **Follow-up.** The test kernel gives test programs every device, as before; a test that wants one device finds it with the same function.

## Invariants
- **INV-0091-1.** `init` gives a device page to a driver only if exactly one transport in it holds a device, and only to the driver for that device ID. *Check:* host tests of the identification function with empty, single, double and malformed pages.
- **INV-0091-2.** `tools/run-qemu.sh --gui` places each GUI device alone on its page. *Check:* `make test-gui` boots and `init` reports each device on its own page; a page with two devices would be refused and the test would fail.

## Open questions
- Whether a board with packed transports ever matters: QEMU `virt` is the only target.

## Revisit triggers
- A device that cannot be placed alone on a page.
- PCI transports, which have their own BARs and make this moot.

## Architect's decision
