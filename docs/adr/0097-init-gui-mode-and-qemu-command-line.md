# ADR-0097: `init` in GUI mode, and the QEMU command line

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0053, ADR-0054, ADR-0070, ADR-0080, ADR-0090, ADR-0091, ADR-0092, ADR-0093, ADR-0096
- **Evidence:** CP-17, CP-01, alternatives-postmortem.md#genode, alternatives-postmortem.md#hubris, alternatives-postmortem.md#minix-3

## Context
ADR-0053 makes `init` the one process the kernel starts, with a compiled table of what it starts and supervises; ADR-0080 sets its restart policy for the storage stack. Stage 5 adds two drivers (ADR-0090, ADR-0092), a compositor (ADR-0093), a terminal (ADR-0096) and a second way to run the shell. `make run` is the serial shell and must stay so. Tests must see the screen without a window on the host. QEMU offers a QMP socket with `screendump`, `send-key` and `input-send-event` `[outside evidence: QEMU QMP reference]`. Measured: nothing yet.

## Decision drivers
1. `make run` and every stage 4 test behave as before.
2. Each process gets only what it needs (CP-01): the compositor's manager channel stays with `init`.
3. A dead driver costs as little as possible: the compositor holds every pixel, so a GPU or input driver can restart alone (CP-17).
4. The GUI is testable headless, from the host, without image files in the repository.
5. Small: one more table in `init`, not a session manager.

## Considered options

### Option A: A display manager and session started by the service manager, devices bound by hotplug (Linux)
- **Description:** systemd starts a display manager, which starts a compositor session for a user; udev binds input and DRM devices as they appear.
- **Evidence for:** Flexible: sessions, users, devices that come and go.
- **Evidence against:** A session manager, a device manager and a configuration format, for one fixed machine (driver 5).
- **Cost:** *(estimate)* Several programs.
- **Precedent:** Linux desktops.

### Option B: `init`'s table, with a GUI mode chosen by the devices present
- **Description:** If `init` finds a GPU and an input device (ADR-0091), it starts the GUI drivers, the compositor, the terminal, and the shell with a terminal; otherwise exactly what stage 4 starts. It supervises each part.
- **Evidence for:** Drivers 1 to 5. Restart of a driver alone is MINIX 3's reincarnation (`alternatives-postmortem.md#minix-3`), and parent-routed handles are Genode's (`alternatives-postmortem.md#genode`).
- **Evidence against:** `init` grows, and is still a single point of failure (ADR-0053).
- **Cost:** *(estimate)* 250 lines in `init` and a shared module.
- **Precedent:** ADR-0053's table, extended.

### Option C: A separate session launcher under `init`
- **Description:** `init` starts the storage stack and a `session` process, which owns the GUI table and its restarts, recursively, as Genode's init does (`alternatives-postmortem.md#genode`).
- **Evidence for:** `init` stays small; a session could be restarted whole.
- **Evidence against:** A second supervisor and a channel between them for no gain at stage 5: the session holds the same handles `init` would.
- **Cost:** *(estimate)* Option B plus a process and its protocol.
- **Precedent:** Genode.

### Option D: A static image (Hubris)
- **Description:** All processes and connections fixed at build time, restarted by a supervisor task (`alternatives-postmortem.md#hubris`).
- **Evidence for:** Nothing decided at run time.
- **Evidence against:** `make run` and `make run-gui` would be two images, and a device's presence could not choose the mode.
- **Cost:** *(estimate)* A build step per configuration.
- **Precedent:** Hubris.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **GUI mode** if `init` finds a GPU page and at least one input page (ADR-0091). `init` reads only the device ID, so it does not tell a keyboard from a tablet; each `inputd` finds out what it drives (ADR-0092). Otherwise `init` is stage 4's, line for line in behaviour.
- **What `init` starts in GUI mode**, each in a sub-job, after the storage stack: `gpud` (the GPU page), one `inputd` per input page, `compd` with the manager channel and the drivers' channels, `term` with a compositor client channel and the shell's terminal end, and `zosh` with the volume at slot 5 and the terminal at slot 6. `zosh` gets the console with `WRITE` only. The compositor's manager channel stays in `init`.
- **Restarts.** A dead `gpud` or `inputd` is restarted alone, at most three times each in a boot `[assumption, as ADR-0080]`, and its new channel goes to the compositor with `AttachDisplay` or `AttachInput`. A driver whose client goes resets its device and exits. If `compd` or `term` dies, `init` ends the GUI processes and the shell and starts them again, at most three times; a storage restart (ADR-0080) restarts the terminal and the shell with it. When the shell ends on its own, `init` syncs and powers off, as before.
- **QEMU** (`tools/run-qemu.sh`). `--gui` adds `virtio-gpu-device` (1024 × 768) on `virtio-mmio-bus.23`, `virtio-keyboard-device` on bus 15 and `virtio-tablet-device` on bus 7 (ADR-0091), with the modern transport (ADR-0080). Interactive, it opens QEMU's default display; with `--test` the display is `none`. `--qmp PATH` adds a QMP socket at `PATH`.
- **Make.** `make run` stays serial. `make run-gui` boots with `--gui`, 256 MiB and the disk, and opens a window on the Mac; the serial line stays on the terminal for `init`'s reports. `make test-gui` boots headless, drives QMP from the test runner, reads screendumps (PPM) and types with `send-key`.

**Tensions with accepted ADRs.** *ADR-0080* restarts the shell on any storage death; in GUI mode the terminal goes with it, so that the new shell gets a new terminal.

### Effect of the vision
None on the choice. For target 2 a launcher of sandboxes would be a program like this table, holding the compositor's manager channel and handing each sandbox one client channel; that is a later program, not a change here. Target 3 needs sessions and hotplug, which Option C's launcher would hold, at the cost of moving the GUI part of this table into it.

### Deferred to stage 6
The POSIX broker joins the session as another process `init` starts; the terminal's shell end then goes to the broker (ADR-0096).

## Consequences
- **Positive.** One image runs both modes. Driver restarts are invisible except for a redraw.
- **Negative.** `init` holds more state. Children of the shell print to the serial line in GUI mode (ADR-0096).
- **Follow-up.** A session launcher if more than one session is ever wanted.

## Invariants
- **INV-0097-1.** Without GUI devices, `init` starts what stage 4 starts and the serial tests pass unchanged. *Check:* `make test-boot` and `make test-files`.
- **INV-0097-2.** A dead GUI driver is restarted alone, and the compositor shows the screen again after it. *Check:* an in-QEMU test that kills the GPU driver and an input driver, restarts them through the same code `init` uses, and sees a commit acknowledged and input delivered after each.
- **INV-0097-3.** In GUI mode `zosh` holds no console `READ` right and its input comes only from its terminal. *Check:* `make test-gui` types into the window with the serial line closed for input.

## Open questions
- Whether three restarts per driver is right: nothing measured.

## Revisit triggers
- A second session or user.
- Devices that appear after boot.

## Architect's decision
