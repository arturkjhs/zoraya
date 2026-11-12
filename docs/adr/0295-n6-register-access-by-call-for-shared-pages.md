# ADR-0295: N6, a shared virtio-mmio page: register access by call, one device per transport

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0054, ADR-0070, ADR-0091, ADR-0221, ADR-0230
- **Evidence:** DM-009, CP-01, CP-17, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, stage-4/sources.md#virtio-and-qemu

## Context
ADR-0221 item 8 found no page for the network device when the GUI is on: the four pages of `virt` hold the disk (page 3),
the GPU (transport 23, page 2), the keyboard (15, page 1) and the tablet (7, page 0). The architect deferred the choice
and TASK-26 decided: take the recommended option, register access by kernel call, in N6. ADR-0091 offered it as its
Option D and put it aside in favour of one device per page. This ADR is the superseding note ADR-0221 item 8 says that
choice needs. It changes ADR-0091 only for a page with more than one device.

## Decision drivers
1. A driver holds only its own device (ADR-0091 driver 1, CP-01, DM-009): the network driver and the display driver on one page
   must not reach each other's registers.
2. No device knowledge in the kernel beyond ADR-0070's reset (ADR-0054 driver 2): the kernel bounds an offset, it does not
   know a register.
3. A driver dies and restarts alone (CP-17), so a pin, a quarantine and a reset belong to one transport, not to a page.
4. The drivers already use the `Registers` trait: nothing in a driver's logic changes.

## Considered options

### Option A: the call on the page device, with the range as an argument
- **Description.** `device_read(page, range, offset)`. One handle for the page, so every holder reaches every transport.
- **Evidence against.** Fails driver 1: the isolation would rest on what `init` does not say.
- **Precedent.** None kept.

### Option B: one device object per transport, reached by calls only (ADR-0091 Option D)
- **Description.** Below.
- **Evidence for.** Drivers 1 to 4. Zircon gives a driver resources per device (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`); the transports share a page because QEMU fills them eight to a page (`stage-4/sources.md#virtio-and-qemu`).
- **Evidence against.** A system call per register access (a notify per transmit batch, an acknowledgement per
  interrupt); a second access path beside the mapping.
- **Cost.** About 120 lines in the kernel and a `Window` type that is a mapping or a call.

### Option C: virtio over PCIe, or one driver for two devices (ADR-0221 item 8 (b) and (c))
- **Evidence against.** (b) is a host bridge and ECAM in the kernel or `init`; (c) merges `ethd` with `gpud` or the two
  input drivers, against ADR-0092.

## Recommendation
**Recommendation (not a decision): Option B.** Built in N6.

1. **`device_transport(page, index)`** (call 53) gives a handle to the device of transport `index` of a page. It needs
   `MAP` on the page (the holder of a page may split it), and returns a `Device` object made at boot (ADR-0022), one per
   transport, with the rights of a device handle less `MAP` (N6 review, finding 1). Asking a transport device for a
   transport, or for a mapping, is `AccessDenied`; an index with no range is `InvalidArgument`. `init` gives a driver a
   transport with the driver's rights less `MAP`.
2. **`device_read(device, offset, width)`** (call 54, `READ`) and **`device_write(device, offset, value, width)`** (call 55,
   `WRITE`) read and write `width` bytes (1, 2 or 4) of a transport device, one access of that width: the configuration's
   8- and 16-bit fields get the accesses virtio 1.2 4.2.2.2 asks for (N6 review, finding 6; the first cut read a word and
   wrote a byte back as a whole word). The control registers below 0x100 take 32-bit accesses only, as the same section
   says (QEMU answers a narrower read there with 0). The offset must be a multiple of the width and the access inside
   the transport's block (the device tree's size, at most 0x200 bytes); else, or for any other width, `InvalidArgument`. A value wider
   than the access is `InvalidArgument`, not cut. A page device has no register calls (`AccessDenied`). The offset and
   width are `Size`s and the value an `Opaque`, which the ABI's allow list (INV-0045-1) lists by name for these two calls.
   `zoraya-rt` has `device_read` and `device_write` for 32 bits and `device_read_width` and `device_write_width`.
3. **What is per transport.** Interrupt (range 0 only), pin, quarantine and reset are the transport device's own, so a
   driver's death resets its transport and no other. Interrupt lines are taken once, by line, as before.
3a. **Holders (N6 review, finding 1).** A transport device and its page are one device to ADR-0070. A process that
   writes a transport's registers becomes its *holder*: the kernel keeps the hold in the process's address space and ends
   it when the process closes its last handle to the transport that has `WRITE`, or ends. A transport has at most one
   holder, and none while its page is mapped; a second writer, or a writer while the page is mapped, gets `BadState`.
   The page cannot be mapped, nor its quarantine released, while any of its transports has a holder or a live pin; a
   transport's quarantine is released only with no holder, no live pin and its page unmapped. Reading needs no hold (a
   read restarts nothing), so `init` identifies a transport without holding it. A hold whose handle was moved to another
   process (`channel_write`) lasts until the first process ends: the safe direction. Lock order: the page's state, then a
   transport's.
3b. **Blocks do not overlap.** At boot a transport whose block (offset and size, as clamped above) would overlap the one
   before it in its page is left out with a message (N6 review, finding 5), so no transport device reaches a neighbour's
   registers whatever the device tree says.
4. **`init`'s scan.** A page with one device is given whole, mapped, as ADR-0091 says. A page with several is read, and
   each of its transports is split with `device_transport`, identified by `device_read`, and given to the driver for its
   device ID with range 0. A driver opens its device with `Window::open`: it maps the page if it may and uses the calls if
   the kernel answers `AccessDenied`. The driver's code does not change.
5. **`tools/run-qemu.sh`** accepts `--net` with `--gui`: the network device stays on transport 16, which now shares page 2
   with the GPU on transport 23.

**Tensions with accepted ADRs.**
- *ADR-0091 INV-0091-1* ("a device page goes to a driver only if exactly one transport holds a device") holds for the
  page; for a shared page the rule is that no driver gets the page, and each gets a transport device. *INV-0091-2*
  ("each GUI device alone on its page") is not true with `--net`, by design.
- *ADR-0054*: a mapped page is still held by one process at a time. A process that maps the page can reach every
  transport on it, so the mapping and the transports' holders exclude each other (item 3a): ADR-0070's premise, no
  mapping and so no process that can restart the device, holds for a transport as for a page.
- *ADR-0070*: a page device's quarantine release still resets every transport of the page, and is refused while any of
  them has a holder or a live pin; a transport device's release resets that transport only.

### Effect of the vision
Target 2: the drivers are in the trusted base; separating them is fault isolation, as in ADR-0091. A call per register
access is the price of a desktop with a network.

## Consequences
- **Positive.** `--gui --net` works on `virt`; the RTC and any later device get the same way; no driver logic changed.
- **Negative.** A system call per register access on a shared page; two paths to the registers (mapping, call).
- **Follow-up.** The cost of a call is measured in the report (N6). If a page is ever shared by a disk and a driver that
  notifies at a high rate, the call's cost should be measured again.

## Invariants
- **INV-0295-1.** `device_read` and `device_write` reach only the registers of the transport device's own block: an
  access whose width is not 1, 2 or 4, narrower than 4 below the configuration (0x100), whose offset is not a multiple
  of its width, or that is not inside the block, is refused, a page device has no register calls, a transport device has no mapping, no `MAP` and no transports, and no
  two blocks of a page overlap. *Check:* `t_devices` (`transport_devices`) and the kernel test
  `overlapping_transports_are_left_out` in `make test-qemu`.
- **INV-0295-2.** On a shared page `init` gives no driver the page, only its transport. *Check:* `make test-gui` with
  `tests/gui/net.script`: the machine with `--gui --net` boots, `init` reports the shared page, the network has its lease
  and the terminal shows `net status`.

- **INV-0295-3.** A transport device and its page are never driven by two processes at once, and neither's quarantine is
  released while the other could restart the device: a transport has at most one holder and none while its page is
  mapped; a page is not mapped or released while a transport has a holder or a live pin; a transport is not released
  while its page is mapped or it has a holder or a live pin. *Check:* `t_devices`
  (`a_transport_and_its_page_keep_holders_apart`: the review's scenario, a second writer, the hold's end with the last
  handle and with the holder's exit) in `make test-qemu`.

## Open questions
- The cost of a register call under HVF and TCG (report, N6).

## Revisit triggers
- A driver on a shared page that notifies at a high rate (a measured cost).
- PCI transports, which make the question moot (ADR-0091's trigger).

## Architect's decision
(Left empty for the human.)
