# ADR-0053: Service discovery and the first process

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0040, ADR-0041, ADR-0043, ADR-0046, ADR-0050, ADR-0052, ADR-0054, ADR-0055, ADR-0056
- **Evidence:** DM-023, CP-01, DM-009, DM-021, GR-004, GR-005, alternatives-postmortem.md#genode, alternatives-postmortem.md#hubris, alternatives-postmortem.md#plan-9--inferno

## Context
ADR-0002 rule 5 has a launcher build a per-process namespace table and forbids a global name space. Nothing yet says who the launcher is. At stage 3 the kernel itself starts programs, with the console, a job and the ramdisk directory (ADR-0043). A driver and a service now need starting, with handles to each other, and clients need to reach them.

## Decision drivers
1. No ambient authority: a process reaches a service only through a handle (CP-01, DM-009).
2. The kernel knows no service by name.
3. Failure of a service is seen by something that can act.
4. Small at stage 4: three processes, not a framework.

## Considered options

### Option A: A global path namespace and `init` scripts (Linux)
- **Description:** Services listen at well-known names any process can open.
- **Evidence for:** Simple and known.
- **Evidence against:** Ambient authority, DM-009, and against ADR-0002.
- **Cost:** Permanent.
- **Precedent:** Unix.

### Option B: A global registry service (D-Bus, Mach bootstrap)
- **Description:** Every process gets a channel to a registry and looks up names there.
- **Evidence for:** Late binding, and easy restarts.
- **Evidence against:** One channel that names everything is a global namespace with a policy problem attached. DM-021 records D-Bus's daemon hops and growing credentials API, and the rejection of kernel buses.
- **Cost:** *(estimate)* A service and its policy.
- **Precedent:** D-Bus (DM-021); Mach's bootstrap port and launchd `[outside evidence]`.

### Option C: Parent-routed handles; `init` as the root launcher
- **Description:** The kernel starts one process with the root handles. It starts services and passes each what it needs at `spawn`. A client gets channels in its handle list or namespace table, from whoever spawned it.
- **Evidence for:** ADR-0003's explicit handle list, with Genode's parent-routed sessions as a working recursive form (`alternatives-postmortem.md#genode`) and Fuchsia's launcher (GR-005). Plan 9's per-process name spaces are the table's ancestor (GR-004, `alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** Everything must be wired by a parent, and a late-started service needs a path through parents. Genode's configuration format migration was its "most delicate challenge" of 2025 (`alternatives-postmortem.md#genode`).
- **Cost:** *(estimate)* A few hundred lines in `init`.
- **Precedent:** Genode, Fuchsia.

### Option D: A static system image (Hubris, seL4 Microkit)
- **Description:** All processes and connections are fixed at build time.
- **Evidence for:** Nothing to discover (`alternatives-postmortem.md#hubris`).
- **Evidence against:** A general-purpose system starts things at run time.
- **Cost:** Low now, and a redesign later.
- **Precedent:** Hubris.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **First process.** The normal kernel starts exactly one program, `init`, from the ramdisk. It is an ordinary process with no special status in the kernel, only special handles. The test kernel keeps starting test programs its own way.
- **What `init` receives.** ADR-0043's console, a job of its own under the root, and ramdisk directory, plus `Device` handles at further slots (ADR-0054). Nothing else in the system can make those. `init` keeps them, to give a restarted driver a duplicate.
- **Starting services.** At stage 4 a compiled table in `init`: start the driver with its device handles in a sub-job, start the filesystem service with a channel to the driver in a sub-job, and start each client in a sub-job with a directory channel. All of them are trusted at stage 4; limits on jobs belong to ADR-0056. The service gets its root endpoint at spawn. `init` keeps the root client end, and `clone`s it (ADR-0055) with narrowed rights for each client. A later configuration format is not needed for three processes.
- **Discovery.** There is none as such. A process has what it was handed.
- **Supervision.** `init` holds a process handle and a supervisor channel for each service, on one port. Exit is `TERMINATED`; a volume's `Failed` state is a message on the supervisor channel, which settles ADR-0010's assumption. Restart is `init`'s policy, and the order is in ADR-0054. Running clients are not recovered: they lose their handles, and only their spawner could give them new ones.
- **If `init` ends,** the kernel reports it and powers off. Nothing else can supervise, as with `init` on Linux.

### Effect of the vision
Target 2 fits this best: an agent's sandbox is a sub-job with a small namespace table, and it holds nothing else. A launcher for such sandboxes is an ordinary program holding a job handle. Target 3 needs a configuration format and perhaps a broker that hands out services by name to the processes allowed to ask: that is a program, not a kernel change, so the later cost is small.

### Deferred to stage 6
The namespace table of ADR-0002 is user-space data, built by the launcher and passed at spawn. It arrives with `libposix`, which moves ADR-0002's follow-up, a format in the `abi` crate, to stage 6. The broker is started as services are. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** Authority is visible in `init`'s table. The kernel stays ignorant of services.
- **Negative.** A new service reachable by an old client needs a new path, and that cost will grow. `init` is a single point of failure.

## Invariants
- **INV-0053-1.** The normal kernel starts exactly one process. *Check:* `make test-boot` output shows `init` as the only process the kernel created.
- **INV-0053-2.** Device-resource handles exist only in `init` and in what it hands out. *Check:* in a boot test, `init`'s outgoing handle lists are enumerated, and a child given a console and a job cannot obtain a `Device`.
- **INV-0053-3.** A service's death is seen by its supervisor. *Check:* a `make test-boot` run in which `init` kills a service and reports `TERMINATED` from its port.

## Open questions
- How a user session is built at stage 5, and who is its launcher.
- Whether `init` should reboot on service failure, or keep what still runs.

## Revisit triggers
- A service that must be reachable by clients that cannot be wired in advance.
- More than a handful of services.

## Architect's decision
