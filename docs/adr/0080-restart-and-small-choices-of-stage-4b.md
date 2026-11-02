# ADR-0080: Restarting the storage stack, and small choices of stage 4b

- **Status:** proposed
- **Date:** 2026-11-02
- **Depends on:** ADR-0014, ADR-0015, ADR-0050, ADR-0052, ADR-0053, ADR-0054, ADR-0055, ADR-0070
- **Evidence:** CP-12, DM-046, alternatives-postmortem.md#minix-3, stage-4/sources.md#virtio-and-qemu

## Context
ADR-0053 makes restart `init`'s policy, and ADR-0054 gives an order for it, with two separate paths: a driver that dies, and a service that dies while its driver lives, where the driver takes a new client. It says running clients are not recovered, and leaves open what `init` then does with them. Building TASK-13 also met small points the accepted ADRs leave open. Nothing here is measured.

## Decision drivers
1. Whatever happens, a remount sees a stopped device (ADR-0054, ADR-0015).
2. One restart path is fewer paths to test (DM-046 records drivers as a large source of bugs).
3. A person at the shell should get a working volume back.
4. Failure is a value a supervisor sees (CP-12).

## Considered options

### Option A: Reboot on any storage failure (Linux with a dead root disk, in effect)
- **Description:** `init` powers off.
- **Evidence for:** Nothing to get wrong.
- **Evidence against:** ADR-0054 asks for a restart, against driver 3.
- **Cost:** None.
- **Precedent:** A Linux system whose root device goes.

### Option B: Two paths, as ADR-0054 lists
- **Description:** A driver that outlives its service resets the device and takes a new channel on a supervisor channel, and only a dead driver is restarted.
- **Evidence for:** MINIX 3 restarts only what died (`alternatives-postmortem.md#minix-3`).
- **Evidence against:** Two paths, and a driver with a protocol for reattachment that serves no other purpose.
- **Cost:** *(estimate)* 100 more lines in the driver and `init`.
- **Precedent:** MINIX 3.

### Option C: One path: end both and start both
- **Description:** On any death or report, `init` kills what is left of the driver and the service, and starts both, then a new shell. A driver that sees its client go resets the device and exits.
- **Evidence for:** Drivers 1 to 4. A new driver always starts from `dma_release_quarantine` and a reset (ADR-0070), so the order of ADR-0054 is met whichever died.
- **Evidence against:** The live state of a working driver is thrown away, which costs a device negotiation.
- **Cost:** *(estimate)* 60 lines in `init`.
- **Precedent:** None in the files.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. Since it replaces ADR-0054's "Service death with a live driver", accepting it marks that bullet as replaced.

- **Clients.** `init` ends the shell's job too, and starts a new shell with a new root connection, so a person gets a prompt back, with its job history gone. Running clients are not recovered, as ADR-0054 says.
- **At most three restarts** in a boot `[assumption]`. After that `init` leaves the shell running with no volume.

Accepting this ADR also records these, as built:

- **Modern virtio-mmio.** `tools/run-qemu.sh` passes `virtio-mmio.force-legacy=false`, because QEMU defaults to the legacy transport and ADR-0054 and ADR-0015 follow version 2 (`stage-4/sources.md#virtio-and-qemu`).
- **A heap in user programs.** `zoraya-fs` needs `alloc`, so `zoraya-user` provides `zoraya-heap` over memory objects.
- **Tokens.** ADR-0055's event object is `event_create`, with no signal yet, `NotWaitable`, and `object_same(a, b)`.
- **Supervisor messages.** The service reports on its supervisor channel with one message in `zoraya-proto-file`: `Failure { reason }`.
- **Cookies.** A `read_directory` cookie is valid for the connection's last reply: the service keeps one name per connection, because the engine continues after a name.
- **`clone` of an object.** `Volume::reopen` gives a second engine handle, so a clone has a record of its own (ADR-0055).
- **No `df`.** The file protocol has no free-space request, and the engine reports none: `zosh` has no `df`.
- **`cd`.** `zosh` holds a root connection and a current-directory connection. `..` is resolved in the shell from its own path string, since ADR-0013 keeps it out of the server.
- **Wall clock.** The virtual timer gives no date, so the service's wall clock is seconds since boot, added to the volume's latest time by ADR-0014's rule.

**Tensions with accepted ADRs.** *ADR-0054, Service death with a live driver:* replaced, above. *ADR-0014 rule 3:* a clock with no date is one that "may stand still or step backwards", which the engine already copes with.

### Effect of the vision
For target 2 a shell is not the client, and a launcher would restart sandboxes the same way. Option B can be added later for a device that is costly to renegotiate. Moving later costs the reattach protocol and its tests, 100 lines `[estimate]`.

### Deferred to stage 6
Nothing here depends on POSIX. ADR-0055 holds the one requirement on stage 4.

## Consequences
- **Positive.** One restart path, tested end to end.
- **Negative.** A failure ends the shell's jobs.
- **Follow-up.** A real clock, when a device offers one.

## Invariants
- **INV-0080-1.** After any death in the storage stack, a remount happens only after the device has been reset. *Check:* in-QEMU tests killing the driver and the service, then `zoraya-fsck` on the image and a read through a new connection.
- **INV-0080-2.** `init` restarts at most three times in a boot, and the shell still answers after that. *Check:* a scripted test killing the stack four times.

## Open questions
- Whether three restarts is the right number: nothing measured.

## Revisit triggers
- A device whose negotiation is costly.

## Architect's decision
