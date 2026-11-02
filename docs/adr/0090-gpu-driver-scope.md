# ADR-0090: GPU driver scope

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0015, ADR-0051, ADR-0052, ADR-0053, ADR-0054, ADR-0070, ADR-0091, ADR-0093
- **Evidence:** DM-081, DM-046, DM-047, GR-021, CP-17, alternatives-postmortem.md#genode, alternatives-postmortem.md#minix-3, stage-4/sources.md#drivers, stage-4/sources.md#virtio-and-qemu

## Context
Stage 5 puts pixels on QEMU's display. QEMU `virt` offers `virtio-gpu-device` on the same virtio-mmio transports as the disk (`stage-4/sources.md#virtio-and-qemu`), and ADR-0054 already gives a user process device, interrupt and pin objects. The virtio-gpu 2D command set is small: query the display, create a resource, attach guest memory as its backing, bind it to a scanout, transfer a rectangle to the host, flush it `[outside evidence: virtio 1.2, section 5.7.6]`. 3D (virgl) needs a host renderer and a large command stream. What is measured: nothing on Zoraya. What is assumed: one display, one resource, the sizes below.

## Decision drivers
1. A bug in display code must end a process, not the kernel (DM-047, DM-081, CP-17).
2. The device is untrusted: every response is checked, as ADR-0015 rule 8 asks for blk; Linux's virtio drivers had to be hardened later for trusting the device (GR-021).
3. The compositor holds every pixel it shows, so the driver holds no state worth keeping across a restart (CP-17).
4. Small: the stage's criterion is a terminal window, not acceleration.

## Considered options

### Option A: DRM/KMS in the kernel (Linux)
- **Description:** A kernel driver exposes KMS objects (CRTCs, planes, framebuffers) and buffer management to a user-space compositor through `ioctl`s.
- **Evidence for:** The path every Linux compositor uses; atomic mode setting; buffers shared by handle.
- **Evidence against:** Display and GPU code in the kernel is the class DM-081 records for win32k, with its exploit history, and DM-046 leaves the monolithic choice contested. ADR-0054 already moved drivers out.
- **Cost:** *(estimate)* A kernel subsystem and an `ioctl` surface, against ADR-0052.
- **Precedent:** Linux.

### Option B: virtio-gpu 2D in a user process
- **Description:** `gpud` holds the device page (ADR-0091), sets up the control queue, and serves one client, the compositor, over a display protocol: `Info`, `Attach` (the compositor's memory object becomes the resource's backing), `Flush` (a rectangle). It issues the six 2D commands only.
- **Evidence for:** Drivers 1 to 4. The objects exist (ADR-0054, ADR-0070). MINIX 3 restarted user-space drivers (`alternatives-postmortem.md#minix-3`). Genode runs graphics as a driver component outside the kernel (`alternatives-postmortem.md#genode`).
- **Evidence against:** Every flush crosses two processes. The driver is in the trusted base for DMA, as blk is (`stage-4/sources.md#drivers`).
- **Cost:** *(estimate)* 400 lines of host-tested logic and a 250-line process.
- **Precedent:** MINIX 3, Genode, Fuchsia's user-space drivers (`stage-4/sources.md#drivers`).

### Option C: virtio-gpu with 3D (virgl)
- **Description:** Option B plus contexts and command submission for a host renderer.
- **Evidence for:** Acceleration later without a new driver.
- **Evidence against:** A command stream is a large hostile-input surface in both directions, needs a host renderer QEMU builds only with extra libraries, and nothing at stage 5 draws in 3D (driver 4).
- **Cost:** *(estimate)* Several thousand lines.
- **Precedent:** Linux's virtio-gpu with Mesa.

### Option D: A firmware framebuffer and no driver (`ramfb`)
- **Description:** QEMU's `ramfb` takes a framebuffer address once through `fw_cfg`; the guest writes pixels and the host reads them.
- **Evidence for:** No queues and no commands.
- **Evidence against:** It is configured through `fw_cfg`, which needs DMA writes to a board interface the kernel does not otherwise use, and there is no flush: the host scans the whole buffer, so damage cannot reach it. It is not virtio, so ADR-0054's objects do not cover it.
- **Cost:** *(estimate)* A kernel `fw_cfg` path, or a new device object.
- **Precedent:** Firmware framebuffers in early boot on most systems `[outside evidence: QEMU ramfb documentation]`.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Process.** `gpud`, slot 5 its device (ADR-0091), slot 6 the channel to the compositor; its first argument is the range. It starts with `dma_release_quarantine` (ADR-0070), as `blkd` does.
- **Device.** Device ID 16, virtio-mmio version 2, feature `VERSION_1` only: no `VIRGL`, no `EDID`. Queue 0 (control) only; the cursor queue is not set up, and the compositor draws its own pointer (ADR-0093).
- **Commands.** `GET_DISPLAY_INFO`, `RESOURCE_CREATE_2D` (format `B8G8R8X8_UNORM`, so a pixel is the little-endian word `0x00RRGGBB`), `RESOURCE_ATTACH_BACKING` (the runs `dma_pin` gave), `SET_SCANOUT`, `TRANSFER_TO_HOST_2D`, `RESOURCE_FLUSH`. One command at a time, with its response read and checked: the response type, then nothing else from the device until the next. A response of the wrong type, or a used length shorter than that type's size, is a protocol break: the driver resets the device and exits.
- **Display.** Scanout 0, which must be enabled, with width and height from 1 to 4096 `[assumption]`. Anything else and the driver reports it and exits: there is no display to serve.
- **Display protocol** (`zoraya-proto-display`, ADR-0052). `Info` gives width, height and the pixel format. `Attach` carries one memory object: the driver checks its type, pins it, and checks that its runs cover `width × height × 4` bytes, creates the resource and sets the scanout. A second `Attach` is refused. `Flush { x, y, width, height }` is checked against the resource, then transferred and flushed; a rectangle outside it is `BadRange` and reaches no device. The driver treats every message as hostile (ADR-0054).
- **Death.** The compositor sees `PEER_CLOSED`, and `init` restarts the driver alone (ADR-0097). The new driver gets a new `Attach`, and the compositor sends one full flush: the compositor holds every pixel, so nothing is lost (driver 3).

- **Errors.** A response of the wrong type or length is a protocol break: reset and exit (INV-0090-3). A device *error* response to `Flush` is a status, `DEVICE`, and the driver goes on. An error, or a broken response, after `RESOURCE_CREATE_2D` in an `Attach` leaves resource 1 and perhaps its backing in the device, so it is treated as a protocol break: reset, then unpin, then exit.

**Tensions with accepted ADRs.** *ADR-0054* names virtio-blk as its device; this is a second driver on the same objects, with no change to them. *ADR-0054, Trust boundary:* the same statement holds for `gpud`: it can program the device to read any memory, so it is in the trusted base.

### Effect of the vision
None on the choice. For target 2 the driver is reached only by the compositor, never by an untrusted client, so a sandboxed agent cannot speak to it. The DMA trust statement of ADR-0054 applies unchanged, and so does its cost of change (an SMMU and `virtio-gpu-pci`). Target 3 needs acceleration, which is Option C added to the same process: the 2D commands stay, so moving later costs the 3D path, not a rewrite. Networking and threads are untouched.

### Deferred to stage 6
Nothing here depends on POSIX.

## Consequences
- **Positive.** A display bug ends `gpud`; the compositor redraws after a restart. The driver's state is small enough to test against a hostile fake device.
- **Negative.** No hardware cursor, no acceleration, one display. Every flush is a channel round trip and two device commands.
- **Follow-up.** Multiple scanouts and `EDID` when a person needs them; the cursor queue if the software pointer costs too much.

## Invariants
- **INV-0090-1.** The driver negotiates `VERSION_1` and no other feature. *Check:* host test with a fake device that offers every feature bit, asserting what the driver wrote to `DriverFeatures`.
- **INV-0090-2.** No command reaches the device with a rectangle outside the resource, whatever the client sends. *Check:* host test of the display logic with hostile `Flush` and `Attach` messages against a recording fake device.
- **INV-0090-3.** A response of the wrong type or length, or a used index past what was submitted, is a protocol break, after which the driver resets the device and sends no further command. *Check:* host tests against a hostile fake device, one per kind of lie, that each is reported as a protocol break; the reset and exit that follow are in `gpud`, checked by reading.

## Open questions
- The cost of a flush round trip through `gpud`: measured by this task (frames per second of a full redraw).
- *As built (TASK-14).* The first build missed completions under TCG. The cause, found by the review (R1): virtio-mmio's interrupt is edge-triggered on QEMU `virt`, and a completion that lands while the line is still high from the previous one makes no edge. The driver now takes the device's interrupt down and looks at the queue again before every wait. The wait also looks at the queue every 10 ms, and still gives up after a second, as a guard; the first time it does, the driver says so once on the console.
- Whether `B8G8R8X8_UNORM` is what QEMU's screendump reports byte for byte: checked by the screendump test.

## Revisit triggers
- A client that needs acceleration.
- A measured flush cost that dominates a frame.

## Architect's decision
