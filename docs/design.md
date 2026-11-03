# Zoraya design summary

What a new agent needs in order to understand the system in ten minutes. This page states what has been decided and
what exists. The decisions themselves, with their evidence and rejected options, are the ADRs in [`adr/`](adr/index.md).
The rules for working here are in [`../CLAUDE.md`](../CLAUDE.md).

## Vision (architect)

Zoraya is an experiment in building an operating system from scratch with AI agents, where every design decision is grounded in measured evidence and recorded as an ADR. Its core idea is that a process can reach only what it was explicitly given: no ambient authority, no global namespace, no fork, no asynchronous signals in the native world; POSIX lives in an isolated personality. Near term, Zoraya is a general-purpose base that runs a self-hosting C toolchain and a GUI terminal under QEMU. A possible later direction is a system for running untrusted code, such as AI agents, in strict isolation; that choice is made after stage 4 and must not be precluded before it. The process itself — successes, failures and numbers — is published as part of the project.

*Note, 2026-11-02 (the architect's direction, recorded by an agent from TASK-14):* after stage 4 the architect chose
to continue with stages 5 and 6, towards the general-purpose base. The isolation direction stays open and must not be
precluded; every ADR of stage 5 states the effect of the vision on its choice.

## Platform

- **Architecture:** aarch64 (ARMv8-A).
- **Machine:** QEMU `virt`. Under HVF on an Apple Silicon Mac (the development host, an M4) the CPU is `host`. Under
  TCG elsewhere it is `cortex-a72`. Real hardware is out of scope.
- **Language:** Rust, `no_std`, target `aarch64-unknown-none`. Stable toolchain 1.99.1, pinned in
  `rust-toolchain.toml`. No nightly features are used.
- **External crates:** none in the workspace ([`dependencies.md`](dependencies.md)). The kernel uses `core` and
  `alloc`. The separate `fuzz/` project (nightly, not built by `make test`) uses `libfuzzer-sys`.
- **Native interfaces are Zoraya's own design.** POSIX exists only as a compatibility personality on top of them
  (ADR-0002).

## Architecture as decided

ADR-0001 to ADR-0006 were accepted as recommended on 2026-10-25, and the filesystem ADRs below on 2026-10-31. [ADR-0001](adr/0001-decision-process.md) sets the process:
- agents write ADRs as `proposed`, and only the architect accepts them;
- accepted ADRs bind implementation;
- every invariant gets an ID and becomes a check (`make adr-check` covers ADR-0001's own invariants;
  [`adr/invariants.md`](adr/invariants.md) gives the status of each).

**Native ABI and POSIX personality boundary** ([ADR-0002](adr/0002-native-posix-boundary.md)). POSIX is split three
ways:
- **`libposix`** is a per-process library for the data path: files, pipes, memory, stdio, time, environment, cwd,
  `$PATH`, `posix_spawn` and readiness waits, all on native handles.
- **A per-session broker** holds pids, process groups, sessions and the controlling terminal.
- **A per-session temporary directory service** provides `/tmp` and FIFO nodes.

`/` is a per-process namespace table of (prefix → handle) built by the launcher. The native ABI has no uid/gid; a
handle's rights are the authority. The only POSIX-specific kernel facility is a copy-on-write process snapshot for
`fork`, gated by a job's `posix` policy bit. Native syscalls name objects only by handle, and paths are relative.

**Process creation** ([ADR-0003](adr/0003-process-creation.md)). The native primitive is `spawn`. It takes:
- a job handle and an image handle;
- argv and environment vectors;
- an explicit handle list (slot, handle, rights, move or duplicate);
- a process-group handle;
- `start_suspended`;
- an inherited-attribute block.

Nothing that is not listed is inherited. `start_suspended` plus `resume` replace child-side setup between `fork` and
`exec`. `fork` exists only for `posix` jobs, as `snapshot_process`. `Port` handles are not duplicated into the child.
`exec` is `spawn` plus exit, with the broker moving the pid. Every process lives in a job.

**Asynchronous notification without Unix signals** ([ADR-0004](adr/0004-async-notification.md)). No native call runs
code asynchronously on an existing thread. Instead:
- **Exit records.** A process object carries `ExitRecord { code, reason, detail, request_kind }`.
- **Request signals.** These are `Interrupt`, `Terminate`, `Hangup`, `Quit` and `Custom(n)`. Their per-process
  dispositions (`Terminate`, `Ignore`, `Deliver`) are held by the kernel.
- **Job-wide delivery.** `job_signal` reaches a whole job tree atomically, and `job_kill` is the uncatchable kill.
- **Faults.** A faulting thread is suspended and the fault goes to an exception channel.
- **Closed peers.** A write to a byte stream whose peer has closed returns `PeerClosed`.

`libposix` delivers POSIX handlers at wait points, and maps "killed by signal N" through the exit record.

**Scope of POSIX compatibility** ([ADR-0005](adr/0005-posix-scope.md)). Compatibility comes in two acceptance tiers:
- **T1:** dash, GNU make (serial and `-j`, both jobserver styles), TinyCC, and the toybox/busybox applets measured in
  TASK-02. This is 248 libc symbols (minus the set-uid rows, plus `writev`) and 49 syscalls.
- **T2:** adds autoconf `configure` (W4), for 291 symbols and 59 syscalls. T2 is accepted only after the cost of
  `snapshot_process` is measured on W4.

The surface is a list of measured CSV rows, and only an ADR can grow it. Out of scope, with written reasons: threads,
`ptrace`, System V IPC, sockets, general `poll`/`epoll`, fault handlers, record locks, `dlopen`, set-uid, and T3
(clang/lld).

**Event and readiness mechanism** ([ADR-0006](adr/0006-event-mechanism.md)). Every kernel object type defines a signal
set or is explicitly not waitable.
- **Ports.** A port holds persistent, handle-keyed subscriptions, which are level- or edge-triggered.
- **Waiting.** `port_wait` takes a monotonic deadline and costs O(packets returned). Every blocking call takes a
  `Deadline`.
- **Cancellation.** A user packet or closing the port are the only things that end a wait early. There is no
  `Interrupted` status.
- **Data transfer.** Readiness plus non-blocking transfer is the whole contract.
- **Completions.** A completion packet kind is reserved on the same port for later.

`pselect` with SIGCHLD and both jobserver styles are mapped onto this mechanism for every measured scenario.

**Filesystem** ([ADR-0010](adr/0010-filesystem-client-guarantees.md) to
[ADR-0015](adr/0015-block-device-contract.md)). Accepted; the reasons are in
[`adr/filesystem-batch-selection.md`](adr/filesystem-batch-selection.md). The storage engine is built as a
host-tested library (TASK-07, "Filesystem engine (as built)" below); the service around it is built (TASK-13, "Storage stack (as built)"). Choices the
ADRs leave open are in [ADR-0016](adr/0016-filesystem-implementation-choices.md). Until a later ADR declares the
format stable, compatibility between format versions is not guaranteed: volumes are rebuilt with `zoraya-mkfs`.
- **Guarantees.** After a crash a volume holds a prefix of the completion order. Requests are atomic, durability comes
  from `sync`, and a storage error stops the volume's writes for good.
- **Mechanism.** A copy-on-write tree with dual root records. Mount picks the newest valid root, with no replay.
- **Integrity.** CRC32C of every block in its parent pointer, a total decoder, and one service per volume.
- **Names.** UTF-8 compared byte for byte, no hard links, no `..` in the server, symbolic links as inert objects.
- **Metadata.** `mtime`, `ctime` and an advisory mode; times strictly increase on a volume.
- **Device.** The virtio contract as written: no write is atomic, and flush is the only barrier.

**GUI** ([ADR-0090](adr/0090-gpu-driver-scope.md) to
[ADR-0097](adr/0097-init-gui-mode-and-qemu-command-line.md), proposed in TASK-14 and built as recommended; the
architect accepts or changes them at review). A virtio-gpu 2D driver and virtio-input drivers in user processes, each
on a device page of its own; a compositor that copies committed damage from clients' memory objects into its own
copies and owns input and focus; a bitmap font of Zoraya's own; a terminal that speaks a VT subset to the shell over a
channel; and `init`'s GUI mode. "GUI (as built)" below describes the code.

**Kernel foundations** ([ADR-0020](adr/0020-kernel-address-space.md) to
[ADR-0024](adr/0024-kernel-fault-policy.md)). Address space, physical memory, heap, interrupts and time, and kernel
fault policy, accepted on 2026-10-31. The sections below describe the code that follows them.

## Boot (as built)

How the kernel starts. The code is in `kernel/src/arch/aarch64/boot.rs`, `kernel/src/dtb.rs` and `kernel/linker.ld`.

- **Loading.**
  - `tools/run-qemu.sh` passes the kernel ELF with `-kernel`. QEMU treats an ELF as a bare-metal image: it loads the
    segments at their physical addresses and starts CPU 0 at the ELF entry.
  - The image is linked at `0xFFFF_FFFF_8000_0000` and loaded at `0x40200000`, which is RAM base plus 2 MiB. The ELF
    entry is the physical address of `_start`. QEMU puts the device tree blob at the start of RAM (`0x40000000`; 1 MiB
    on QEMU 11.1.2), but only if it fits below the image. Otherwise QEMU silently omits the blob and the kernel can
    only report its absence through the fallback UART. `linker.ld` asserts the 2 MiB gap.
- **Device tree address.** The kernel accepts an address in `x0` if it lies in RAM below the image (Linux boot
  protocol style). Otherwise it reads the blob at the start of RAM.
  - Under QEMU 11.1.2, `x0` is `0` for an ELF, under both HVF and TCG. The blob is therefore always found at the
    start of RAM. Every boot banner prints this.
  - The blob is validated by `zoraya-dtb` before use.
- **Exception level.** The kernel starts at EL1, or at EL2 with `-machine virt,virtualization=on`, which works under both TCG and, on the M4 with QEMU 11.1.2, HVF (not on an M1 or M2: Known limitations, "HVF limits"). At EL2 it
  writes a minimal configuration and `eret`s to EL1h; the configuration is documented in the header of `boot.rs`:
  - EL1 is AArch64 with no stage-2 translation. SVE and SME stay trapped;
  - `MDCR_EL2` is set to `HPMN = PMCR_EL0.N` and nothing else, so EL1 owns every PMU counter and no debug or PMU
    access traps to EL2, whatever the reset value was;
  - `SCTLR_EL1` is set before the `eret`;
  - EL1 may use the physical timer and counter;
  - FP/SIMD is not trapped;
  - the GIC system-register interface is enabled for EL1 if present.

  The tests check that the kernel runs at EL1 after boot in both cases.
- **Secondary CPUs.** With PSCI emulated, QEMU starts only CPU 0 and keeps the others powered off until `CPU_ON`. As
  a safeguard, boot code also parks (`wfe`) any CPU whose MPIDR affinity is not 0.
- **Before Rust.** Rust never runs with the MMU off, because pointers stored in its data are link addresses. The
  assembly stub:
  - sets `SCTLR_EL1` to a known value and enables FP/SIMD (the target lets the compiler use NEON);
  - checks for the 4 KiB granule and parks if it is missing (no console exists yet);
  - builds five boot tables: an identity map of the first 4 GiB (1 GiB blocks, the one holding the image Normal, the
    others Device) and the image at its link address (2 MiB blocks, writable and executable, for this window only);
  - enables the MMU and caches and jumps to the link address;
  - installs the vectors, sets the stack and its limit, and clears `.bss`.
- **In Rust, in order** (`kernel_main`): locate and parse the device tree through the identity map; start the
  console; build the final tables and replace the boot tables; parse the device tree again through the direct map, so
  that nothing keeps a pointer into the identity map; start the frame allocator; start the GIC and timer; unmask
  interrupts; print the banner.
- **Console.** The PL011 is the node named by `/chosen/stdout-path`, or failing that the first enabled `arm,pl011`
  node. `0x09000000` is used only to print a device tree error before halting.
- **Banner.** It shows the version (`Cargo.toml`) and git commit (embedded by `kernel/build.rs`), the entry and
  current exception level, the boot CPU, the CPU count, the interrupt controller, GIC version and UART interrupt, the
  timer's frequency and interrupt, PSCI, memory statistics and the memory map.
- **Shutdown.** PSCI `SYSTEM_OFF`, through the conduit (`hvc` or `smc`) named by the device tree's PSCI node.
- **Panic.** The handler prints `ZORAYA-PANIC at file:line:col: message` on one line, then halts.

**Threads and user programs** ([ADR-0030](adr/0030-threads-and-scheduling.md) to
[ADR-0034](adr/0034-user-faults.md)). Stage 2, accepted on 2026-10-31. The sections below describe the code that
follows them.

## Exceptions (as built)

The code is in `kernel/src/arch/aarch64/exception.rs` (ADR-0024).

- **Vectors.** The kernel runs on `SP_EL1`. Each entry of the "current EL with SP_ELx" group compares SP with a limit
  in `SP_EL0`, with `TPIDR_EL1` as a scratch register, before touching memory.
- **Frame.** x0 to x30, SP, `ELR`, `SPSR`, `ESR`, `FAR`, `FPCR`, `FPSR` and q0 to q31: 816 bytes.
- **Stack overflow.** With no room for a frame, the entry moves to a 32 KiB emergency stack, reports and panics. Bit 0
  of the limit marks that stack; a second overflow halts silently.
- **Policy.** Any synchronous exception, SError or FIQ from EL1 prints a `ZORAYA-FAULT` report and panics. The report
  has `ELR_EL1`, `SPSR_EL1` and the registers. Exceptions that set a syndrome add the exception class by number and
  name, `ESR_EL1`, and `FAR_EL1` when valid; an interrupt's report says it has none. An IRQ with no handler is disabled and ended, reported, and panics. Spurious IRQs are counted.
- **From EL0.** The vector saves the user's `SP_EL0` in the frame and computes the stack limit from SP, since thread
  stacks have a fixed size and SP_EL1 is at the top of one on entry. `svc` is a system call. Any other synchronous
  exception ends the process (ADR-0034). An SError from EL0 is a kernel fault.
- **User copies.** A data abort with the PC in `arch/aarch64/usercopy.rs` and `FAR_EL1` in the user range resumes at
  the routine's error exit. It is the only kernel fault that resumes.
- **Masks.** SError is unmasked once `kernel_main` starts, IRQ and FIQ after the GIC is up. Debug stays masked.
  Handlers run with IRQ masked and do not nest. A panic masks IRQ and FIQ.

## Memory (as built)

| Virtual address | Contents |
|---|---|
| `0xFFFF_0000_0000_0000`, 128 TiB | Direct map, RW, never executable |
| `0xFFFF_8000_0000_0000`, 1 TiB | Device window, Device-nGnRnE, never executable |
| `0xFFFF_8100_0000_0000`, 1 TiB | Heap window |
| `0xFFFF_8200_0000_0000`, 1 TiB | Reserved for thread stacks; empty |
| `0xFFFF_FFFF_8000_0000`, 2 GiB | Kernel image |

TTBR0 is disabled (`TCR_EL1.EPD0`) and holds 0. The layout is stated only in `kernel/src/memory/mod.rs`.

- **Granule.** 4 KiB, 48-bit VA in both halves. `zoraya-pagetable` encodes the tables and refuses W+X and executable
  device attributes. `MAIR_EL1` index 0 is Device-nGnRnE, index 1 Normal write-back.
- **Image.** Text is read-execute, rodata read-only, data and bss read-write. The emergency stack and the boot stack
  are read-write. The boot tables and one guard page below the 128 KiB boot stack are not mapped. Inline stack probes
  (from the target) mean a large frame cannot step over the guard.
- **Direct map.** RAM minus the image, minus every device tree reservation. The device tree blob is mapped read-only.
  So every frame of the image has exactly one mapping. RAM above 128 TiB stops the boot.
- **Hardware W^X.** `SCTLR_EL1.WXN` is set with the final tables.
- **Tables.** The first 64 tables come from a pool in `.bss`, the rest from frames.
- **Frames.** `zoraya-pmm`: two bits per frame, in metadata carved from the largest free range (128 KiB per 2 GiB).
  Reserved: the image with stacks and boot tables, the blob, `/memreserve/` entries, `/reserved-memory` children,
  the metadata, and frame 0. A device tree with more reservations than `zoraya-dtb` records stops the boot.
  Exhaustion is a return value. A bad free panics in the kernel (the crate returns an error).
- **Heap.** `zoraya-heap` behind `GlobalAlloc`: power-of-two classes from 16 bytes to 2 KiB, carved from 64 KiB
  chunks, and a coalescing span list above that. It grows by at least 1 MiB, mapping one frame per page in the heap
  window, and never shrinks. `Box::new` and `Vec::push` panic on exhaustion, so they are allowed only at boot and at
  bounded call sites. `memory::heap::try_box` and `Vec::try_reserve` return the failure.
- **Lock.** `sync::SpinLock` masks IRQ and FIQ while held. On one CPU a taken lock can only be a bug, so it panics
  rather than spin. `sync.rs` says what changes for SMP. Lock order: scheduler, then the deadline queue and heap; a process's exit record, then scheduler; stack slots, then heap; heap, then kernel tables, then frames.

## Interrupts and time (as built)

The code is in `kernel/src/gic.rs`, `kernel/src/irq.rs`, `kernel/src/timer.rs` and `kernel/src/arch/aarch64/timer.rs`
(ADR-0023).

- **GIC.** v2 or v3, from the device tree; anything else stops the boot. v3 uses system registers, affinity routing
  and group 1. v2 stays in group 0, the only group QEMU's GIC without security extensions has. Interrupt numbers come
  from the device tree. All interrupts have one priority.
- **Tested.** GICv3 under HVF and TCG; GICv2 under TCG only, since HVF cannot start with it.
- **Timer.** The EL1 virtual timer (`CNTV_*`, interrupt 27 on QEMU `virt`). `CNTFRQ_EL0` is 24 MHz under HVF on the
  M4 and 62.5 MHz under TCG.
- **Clock.** `timer::now()` returns a `zoraya_time::Instant`: `u64` nanoseconds since the clock started, rounded
  down, never decreasing on one CPU.
- **Deadlines.** `timer::arm(instant, callback, arg)` queues up to 32 deadlines and sets the comparator for the
  earliest, rounded up, so nothing fires early. An empty queue turns the timer off. There is no periodic tick;
  periodic work arms its own next deadline. Callbacks run in interrupt context.

## Threads and processes (as built)

The code is in `kernel/src/sched.rs`, `process.rs`, `syscall.rs` and `kernel/src/memory/{stack,user}.rs`.

- **Scheduler.** One run queue, round robin, on one CPU. The kernel is not preempted: a slice ends at a return to
  EL0, and kernel threads run until they yield or block. A 10 ms slice is armed only when another thread is ready.
- **Thread stacks.** 64 KiB with an unmapped guard page, in the stack window.
- **Context switch.** Callee-saved registers, `FPCR`, `FPSR`, `TPIDR_EL0` and the stack limit. All else is in the
  exception frame. A thread ended is freed by a later one, since none can free its own stack.
- **Address spaces.** One table per process, with an ASID of 8 bits, so 255 live processes at most. User pages are
  not global. With no current process, `EPD0` is set and `TTBR0` is 0.
- **User layout.** Programs from 4 MiB up; a 64 KiB stack mapped in full, ending one page below 2^47, with an unmapped
  page below it.
- **Processes.** One thread each. All memory is mapped at creation, so there is no demand paging. A process ends on
  its own thread: anyone else sets the exit it wants and wakes it, and it acts on that at its next return to EL0, or
  before its first instruction. At exit it releases its memory, ASID and handles, and stays as a zombie holding only
  its `ExitRecord` until its last handle goes.
- **Handles (`crates/zoraya-handle`, `kernel/src/object.rs`).** A `u64` of 16 bits of slot and 48 of generation (ADR-0057), 1024 slots, a rights
  mask that only decreases. Port handles have no `DUPLICATE`, so closing one closes the port.
- **Jobs (`kernel/src/job.rs`).** A tree, at most 32 deep, with one lock over membership and walks. `job_signal` and
  `job_kill` cover the tree; a killed tree refuses new processes. `TERMINATED` means no live process in the tree.
  The `posix` bit gates `snapshot_process`; `job_suspend` and `job_resume` came with stage 6a (below).
- **`spawn`.** One flat buffer, copied once and decoded by `zoraya_abi::spawn::SpawnRequest`: job, image, group, handle
  list, dispositions, argv and environment. A child holds what was listed and nothing else, and reads its vectors from a
  read-only block at `STARTUP_BLOCK`. Registers start at zero. A process the kernel starts has the console, a job and
  the ramdisk directory at slots 1 to 3, and gets images with `directory_lookup`.
- **Requests (ADR-0004).** Dispositions per kind, four fixed and 32 custom; a fixed queue of 64 per process, with
  `LimitReached` when full. `Deliver` asserts `REQUEST_PENDING`; no code runs asynchronously.
- **Ports (`crates/zoraya-events`, `kernel/src/port.rs`).** All signals, ready lists and subscriptions in one structure
  under one lock, linked by index. Level and edge. `port_wait` costs the packets it returns, not the subscriptions.
  1024 user packets per port.
- **System calls.** `svc #0`, number in x8, status in x0, value in x1. One `syscalls!` declaration in `zoraya-abi` makes
  the numbers, the table the lints read, the trait `kernel/src/syscall.rs` implements and the stubs `zoraya-rt` wraps.
- **Not built yet.** `object_wait_one`, timers and exception channels. Byte streams, `object_get_signals`,
  `job_suspend`, `job_resume`, `image_create` and `snapshot_process` came with stage 6a (below), channels and event
  objects with stage 4.
- **Ramdisk.** QEMU ignores `-initrd` for an ELF kernel, so `tools/mkramdisk` packs `user/` programs and the kernel
  embeds them in `.rodata`. A kernel built without the Makefile has an empty one.
- **User memory.** Reached only through `syscall::UserSlice`, with `ldtrb` and `sttrb`, which check permissions as EL0
  does.

## Filesystem engine (as built)

The code is in `crates/zoraya-fs` (`no_std` + `alloc`, `#![forbid(unsafe_code)]`); it knows nothing of IPC. The
service wraps `Volume` and forwards its failure report. Blocks stay on the heap (`new_block`): the engine recurses
over the tree on a 64 KiB process stack, and a block passed by value through that recursion overflowed it.

- **Device.** `BlockDevice` (ADR-0015): size in 512-byte sectors, whole 4 KiB block reads, a batch write whose
  members complete in any order, flush. A device without flush mounts read-only unless the mount says unsafe.
- **Format.** Explicit little-endian layouts in `format.rs`. Blocks 0–7 are the root slots (both copies of four
  pairs); everything else is copy-on-write. Every pointer holds the child's address, CRC32C and generation; every
  metadata block's header holds its kind, level, volume id, own address and generation. One B+tree keyed by
  `(object, kind, offset, name)` holds attribute records (exactly ADR-0014 rule 1's fields), directory entries,
  parent links of directories, one extent per data block, and the orphan list. A one-level bitmap allocator caps a
  volume at about 20.9 GiB (ADR-0016).
- **Commits.** Requests change an in-memory copy-on-write tree; a request that fails, including for space, is
  rolled back from a snapshot. A commit writes every new block, flushes, writes the root record into both slots of
  pair `g mod 4`, and flushes again; freed blocks become reusable only then. Commits happen on `sync`, after
  `T_COMMIT` (5 s, driven by `tick`), at 1,024 dirty blocks, before handing out an id at the durable id limit, and to
  release space for a request refused for it.
- **Mount.** Picks the highest generation with a valid root record copy; refuses an invalid tree root of that
  generation; mounts read-only and reports corruption if the generations in the ring are not consecutive; frees the
  orphan list (and commits) on a read-write mount.
- **Requests.** Open (with exclusive create and truncate), mkdir, symlink (inert link objects), unlink, rmdir,
  rename (replace, no-replace, exchange), set attributes, set size, write (at most 1 MiB), read, stat, readdir,
  readlink, unnamed create and publish, close, sync. Paths are relative, components validated (ADR-0013); a link
  object in a path stops resolution with `IsLink`. Times are `max(wall clock, last + 1 ns)`.
- **Failure.** Any device write or flush error, metadata read error or invalid metadata puts the volume in `Failed`:
  every later mutating request and `sync` gets `StorageFailed`, nothing more is written, reads go on from memory, and
  `take_failure_report` returns the reason once. A bad data block is `Corrupt` for that read only.
- **Checker.** `zoraya_fs::check` (used by `zoraya-fsck` and the tests) verifies every invariant the ADRs define;
  `repair` rebuilds the allocator and fixes namespace damage, and refuses when the tree itself is damaged.
- **Tools.** `zoraya-mkfs` creates an image and copies a host directory into it; `zoraya-fsck` checks an image
  read-only and repairs only with `--repair`.

## Storage stack (as built)

Two processes, each in a sub-job that `init` makes, and `init` over them, on [ADR-0053](adr/0053-service-discovery-and-first-process.md),
[ADR-0054](adr/0054-driver-model.md), [ADR-0055](adr/0055-filesystem-service-protocol.md) and
[ADR-0080](adr/0080-restart-and-small-choices-of-stage-4b.md).

- **`blkd`** owns the virtio-mmio device: split queues on transport version 2, in `crates/zoraya-virtio`, which holds
  the logic and treats what the device writes as untrusted. It speaks `zoraya-proto-block` over a channel to the
  service, and resets the device when the service goes.
- **`fsd`** runs the loop of `crates/zoraya-fs-service` over a `Transport`, with the engine under it. One request at a
  time, and its only blocking wait is on the driver, with a deadline of 30 s. Clients speak `zoraya-proto-file`.
- **`init`** finds the disk, starts both (`user/src/stack.rs`, which `t_storage` also uses), and gives the shell a
  connection to the root. On a report from the service or the death of either, it ends all three and starts them
  again, at most three times. After that the shell runs with no volume. When the shell ends, it syncs and powers off.

A request waiting on a driver that dies reports what it lost: `DEVICE_WRITE`, `DEVICE_FLUSH` or `METADATA_READ`. An idle
service reports `DEVICE_LOST`. `init` does the same for all of them.

Measured on an M4 under QEMU 11.1.2, `make run`'s image, `fsbench`, the last of three runs in one session. The
review's runs (TASK-13 review, R-44), on a busier host, gave about half for the 1 MiB write through a memory object
and for TCG's 4 KiB writes, and the rest within a fifth: read these as orders of magnitude. `write` is into the cache of
dirty blocks, and `read` is after a `sync`, from the device. `sync` is on the host's page cache: QEMU's flush is an
`fsync` of a file. Boot to prompt is `uptime` at the first prompt.

| | HVF | TCG |
|---|---|---|
| Inline, 4 KiB: write, read (MB/s) | 152, 58 | 5.1, 4.0 |
| Inline, 1 MiB: write, read | 273, 76 | 27, 8.1 |
| Memory object, 4 KiB: write, read | 243, 78 | 8.3, 4.7 |
| Memory object, 1 MiB: write, read | 4061, 108 | 502, 11.7 |
| `sync` after 4 KiB, after 1 MiB | 0.30, 3.3 ms | 2.7, 10.3 ms |
| Boot to prompt | 0.105 s | 0.211 s |

## GUI (as built)

Stage 5 (TASK-14), on the proposed [ADR-0090](adr/0090-gpu-driver-scope.md) to
[ADR-0097](adr/0097-init-gui-mode-and-qemu-command-line.md). No kernel change: everything is a user process on the
objects of stage 4. Logic lives in host-tested crates; the processes are thin.

- **Devices** (ADR-0091). `tools/run-qemu.sh --gui` puts `virtio-gpu-device` (1024 × 768) on transport 23,
  `virtio-keyboard-device` on 15 and `virtio-tablet-device` on 7, so each is alone on its 4 KiB page; the disk stays on
  31. `init` maps each device page a range at a time and gives it to a driver only if exactly one transport in it holds
  a device (`zoraya_virtio::queue::identify`): device ID 2 to `blkd`, 16 to `gpud`, 18 to `inputd`. A page with two
  devices is reported and used by nobody.
- **Shared virtio code.** `zoraya-virtio` gains a generic split queue, the transport's set-up and reset, and a
  device-side fake for host tests (feature `fake`). `blkd` keeps its own queue, which is older and mutation-tested.
- **`gpud`** (`zoraya-virtio-gpu`, ADR-0090). `VERSION_1` only, the control queue only, one command at a time with
  its response type and length checked. It serves the compositor `zoraya-proto-display`: `Info`, `Attach` (the
  compositor's framebuffer object, pinned, becomes the resource's backing) and `Flush` (transfer and flush, refused
  outside the resource). Before each wait it takes the device's interrupt down and looks at the queue again:
  virtio-mmio is edge-triggered on QEMU `virt`. A wait also looks at the queue every 10 ms, as a guard, and gives up
  after a second.
- **`inputd`** (`zoraya-virtio-input`, ADR-0092). One process per device; it reads the configuration to learn whether
  it drives a keyboard or an absolute pointer, keeps 64 event buffers offered, checks every event, and sends one
  `zoraya-proto-input` frame per `SYN_REPORT`, with axes scaled to 0..65535. A frame that does not fit is dropped and
  counted, and the count goes with the next one.
- **`compd`** (`zoraya-compositor`, ADR-0093, ADR-0094). The loop runs over a `Platform` trait, so it is tested on
  the host. Slot 5 is the manager channel from `init`, 6 the display driver, 7 to 10 input drivers. A client
  (`zoraya-proto-gui`) asks the display size, creates one surface with a memory object, which the compositor maps
  read-only, and commits damage. Each round reads at most one message per connection, copies committed rows once into
  the compositor's own copy, recomposes the changed rectangle (background, windows bottom to top with a one-pixel frame,
  the pointer), and replies to the round's commits: the reply is the frame callback. Flushes do not block (ADR-0093,
  Decision): one is in flight, what is composed meanwhile goes out as one rectangle when the driver answers, and a
  driver that misses 2 s is dropped and reported to `init` (`DisplayHung`), which restarts it. Windows are
  placed by the compositor (16, 16, then cascaded by 32). Keys go to the focused surface, translated by the US layout of
  `zoraya-keymap`; pointer events to the surface under the pointer. A button press focuses and raises; F12 moves focus
  to the next window and reaches no client. Events that do not fit a client's queue are dropped and counted for it.
  At most 8 clients. With no display driver it goes on; a new one gets the same framebuffer and one full flush.
- **`term`** (`zoraya-term`, `zoraya-font`, ADR-0095, ADR-0096). A compositor client with an 80 × 24 grid of 12 × 20
  cells (968 × 488 pixels) and 500 lines of scrollback (Shift with Page Up or Page Down). It parses the VT subset of
  ADR-0096 with ADR-0154's faint (SGR 2, ended by 22: the colour at half intensity), answers no query, and sends the shell the bytes keys make (`zoraya-proto-term`). It draws only the rows
  that changed, and commits again only after the last commit's reply.
- **`zosh`** uses a channel at slot 6 as its terminal: output as `Output` messages (waiting while the terminal's queue
  is full), input from `Input` messages, and its width from `Size`. Without one it uses the console.
- **`init`** (ADR-0097). With a GPU page and an input page it starts the storage stack, `gpud`, an `inputd` per input
  page, `compd`, `term`, and `zosh` with the volume at slot 5, the terminal at slot 6 and the console to write only. A
  dead driver is restarted alone, at most three times each, and handed to the compositor over the manager channel. A
  dead compositor or terminal ends the GUI and the shell and starts them again, at most three times; then the shell
  runs on the console. A storage restart starts a new terminal with the new shell.

Measured on the M4 under QEMU 11.1.2, the last run of `make test` (`t_gui` in the test kernel at 128 MiB, the criterion
script at 256 MiB). A full redraw is a client committing its whole 1024 × 768 surface and waiting for the reply, which
no longer waits for the driver; "compose" is the compositor's time per frame for the copy and composition; "flush" is a
flush's round trip through the driver, which the compositor does not wait for. Key to
screendump is from QMP's key event to the first screendump that shows the character; reading one screendump (PPM,
1024 × 768) in the runner takes about as long as the HVF figure, which is therefore an upper bound.

| | HVF | TCG |
|---|---|---|
| Full redraw | 759 frames/s | 157 frames/s |
| Compositor CPU per full frame (compose); driver flush round trip | 0.72 ms; 1.21 ms | 2.89 ms; 1.69 ms |
| Key to screendump, median of 10 | 35 ms (one screendump read: 35 ms) | 75 ms (one screendump read: 34 ms) |

Under HVF the flush that no longer blocks (ADR-0093, Decision) lowered the full-redraw rate from about 1017 frames/s
and raised compose time from 0.48 ms, between runs of the same tests; under TCG nothing moved. A guess, not measured:
the compositor now composes while QEMU's device still reads the framebuffer for the previous flush.

## Cross toolchain and ports (as built)

Stage 6b (TASK-17), on the accepted [ADR-0100](adr/0100-libposix-construction.md) and
[ADR-0109](adr/0109-building-the-toolchain.md) and ADR-0140 to ADR-0144. Everything here runs on the host. Since
stage 6c `libc.a` holds the real `zoraya-posix` ("`libposix` core (as built)" below), and C programs built against
it run in Zoraya; the ported programs need 6d's process functions. `ports/README.md` has the details.

- **`make fetch-ports`** downloads the archives of `ports/sources.lock` (TASK-02's pins and hashes, musl 1.2.5 added,
  tcc as GitHub's archive of the pinned commit) into `build/ports/src/` and refuses a wrong SHA-256. Each archive
  may have mirrors after its own URL (buildroot's for musl, dash and busybox, GNU's for make), tried in turn with
  retries on any error, since CI once lost musl.libc.org to a reset connection. It is the only
  step with network. `make ports` builds from that cache only and stops with a message naming `make fetch-ports` when
  an archive is missing.
- **`tools/zcc`** is the C compiler for `aarch64-unknown-zoraya`: the host's clang with
  `--target=aarch64-unknown-none-elf -D__zoraya__ -nostdinc -isystem build/sysroot/usr/include -fno-pic
  -fno-stack-protector`, linking with the pinned toolchain's `rust-lld` as a static `ET_EXEC` from 4 MiB
  (`crt1.o crti.o ... -lc crtn.o`, `-z norelro`, no build id). It refuses `-D__linux__`, `-shared` and `-pie`. Neither
  `__linux__` nor `__unix__` is defined (ADR-0143).
- **`tools/zoraya-ar`** writes deterministic GNU `ar` archives with a symbol index (also as `ranlib`).
- **`libc.a`** is 737 objects of musl's retained directories, compiled with the system-call shim, and the 520 members
  of `zoraya-posix`'s static library (stage 6c; in 6b it was ADR-0140's generated stub, in which every function failed
  with `ENOSYS` or trapped), which also bring `core`, `alloc` and `compiler_builtins`. The shim routes eleven calls; 34 files it refuses
  (33 that make other calls, and `ioctl.c`, which needs musl's Linux constants) are not compiled; 748 names outside the tier list are renamed `__zoraya_*` in every object, so that the
  archive exports exactly the 243 names of `docs/adr/posix-surface-T1.txt` (ADR-0141) and the five names ADR-0142 and
  ADR-0144 add in their files' `posix-surface-added` blocks. `<zoraya/posix.h>` (`ports/include/`) declares Zoraya's
  additions (ADR-0142). The sysroot also has
  `crt1.o` (`ports/crt/crt1.S` and musl's `_start_c`), musl's `crti.o` and `crtn.o`, musl's empty libraries, and
  tcc's `libtcc1.a` and headers in `usr/lib/tcc`.
- **The ports.** All build with `zcc`, and none with `__linux__`: dash (TASK-02's M4 patch and ADR-0142's two
  job-control patches: the fork group hint, and `vforkexec` through `posix_spawn` with `SETPGROUP` and
  `POSIX_SPAWN_TCSETPGRP`), GNU make (one line of `config.sub`), busybox (`tr`, `diff`, unpatched), toybox (`cat head
  ls mv rm sleep touch true uname sort wc yes`; one patch gives `lib/portability.[ch]` a Zoraya branch at the seven
  places that stop an unknown OS with `#error`, and the link needs ADR-0144's `atoll` and `getdelim`) and tcc (a
  `Zoraya` target and a fix for `SHF_EXCLUDE` sections). toybox's scripts need GNU sed. `build/rootfs-posix` has the
  programs in `/bin` with a symbolic link per applet and the sysroot as `/usr`. A clean `make -j4 ports` takes about
  31 s on the M4, and two clean builds give the same bytes for all 18 outputs compared.
- **`make check-ports`** (the end of `make ports`): the tier list is the CSV's (INV-0100-1); `posix-surface-check`
  checks `libc.a` (exports, the exclusion table, missing names, unresolved references, the symbol index, `svc`) and
  fails on five broken copies of it; every excluded musl file is refused by the shim; `zoraya-elf-check` runs the
  loader's parser over every program and over the `a.out` that a host build of the same tcc links against the
  sysroot (INV-0109-1, ADR-0143); `ports/compare-predefs.sh` requires `zcc` and tcc to predefine the same identity
  (`__zoraya__`, no Unix or Linux name, the same target and ABI-type macros; ADR-0143). `make test` runs only the fast part: the tools'
  host tests and the fetch step's test (`make check-fetch-ports`).

## Tests

`make test` prints one line per step and keeps each step's output in `build/logs/<step>.log`: a failed step gives its
log's path and first error lines, so that the log is read only when something failed. `make test V=1` prints
everything, and `KEEP_GOING=1` runs the steps after a failure. It runs:
- `cargo fmt --check`;
- clippy with warnings as errors (host crates, kernel, test kernel);
- the host tests;
- `make adr-check`;
- `make check-user-access` and `make check-host-paths`: the unprivileged load and store forms only in `usercopy.rs`
  (INV-0031-4), and no home-directory paths or email addresses in tracked files outside `docs/research/` and
  `tools/posix-surface/results/`;
- `make check-fetch-ports`: the ports' fetch step against a local file, offline (INV-0109-3, INV-0109-4);
- the in-QEMU tests in seven machine configurations, with `build/test-disk.img` attached and `zoraya-fsck` after them;
- `make test-libposix`, the stage 6c criterion (below, under "How the in-QEMU tests run");
- `make test-gui`, the stage 5 criterion: the normal kernel with `--gui`, headless, under HVF (on a Mac) and TCG.
  The runner (`tools/test-runner/src/gui.rs`) types over QMP and reads QEMU screendumps back as text by matching every
  cell of the terminal's grid against every glyph of `zoraya-font`, exactly: the font draws each pixel in the text
  colour or the background and PPM is lossless, so the tolerance is zero. It also checks the background and the
  focused frame pixel by pixel. `tests/gui/terminal.script` waits for the prompt, types `write a.txt hi` and
  `cat a.txt`, sees `hi`, presses F12, runs `ls`, measures key latency, and powers off. The serial line's input is
  closed, so keys reach `zosh` only through the window;
- `make test-boot`, which builds the normal kernel and boots it under every machine configuration. The test runner
  plays `tests/shell/console.script` on the serial line and checks the output, through to `poweroff` and QEMU's status
  0; `tests/shell/leak.script` runs more programs than a handle table has slots, and `tests/shell/exit.script` ends the
  shell.

| Label | Accelerator | Machine | Checks beyond the test list |
|---|---|---|---|
| auto-default | HVF on a Mac, else TCG | default | entry at EL1 |
| auto-smp4-2g | HVF on a Mac, else TCG | 4 CPUs, 2 GiB | `cpus: 4`, 2048 MiB region |
| auto-el2 | HVF on a Mac, else TCG | `virtualization=on` | entry at EL2; skipped where HVF has no EL2 (Known limitations) |
| tcg-default | TCG | default | GICv2 |
| tcg-smp4-gicv2 | TCG | 4 CPUs | GICv2 with a non-zero `GICD_ITARGETSR` |
| tcg-el2 | TCG | `virtualization=on` | entry at EL2, PSCI via `smc` |
| tcg-smp4-gicv3 | TCG | 4 CPUs, 2 GiB, `gic-version=3` | `cpus: 4`, GICv3 |

On a Mac the three `auto` lines run under HVF, so they cover GICv3 only. GICv2 under HVF is not run, because QEMU
cannot start it.

How the in-QEMU tests run and report:
- The test kernel (feature `qemu-test`) prints the `ZORAYA-TEST` protocol, described in `kernel/src/testing.rs`. A
  `run <name>` line precedes each test, so a test that hangs is named in the timeout report.
- A panicking test is reported as that test's failure. Recovery goes through `eret` to the next test, on a fresh
  stack and out of any exception context.
- `make test-libposix` is stage 6c's criterion: it builds `libc.a` (which needs `make fetch-ports` once), runs the
  export check on it, compiles `tests/posix-core/t_libposix.c` with `zcc`, puts it on an image, and plays
  `tests/posix-core/libposix.script` under the host's accelerator and TCG, with `zoraya-fsck` after each.
- `make test-files` is the roadmap's criterion: one image over two boots, with every file command and its errors, and
  `zoraya-fsck` after each. Then a disk that cannot be mounted (`init` stops after three restarts) and a write failed in
  QEMU's block layer with `blkdebug` (`--disk-write-error`: `init` restarts, and durable files are there).
- `make test-crash` kills QEMU at a random point of `crashwriter`, 10 rounds by default, and runs `zoraya-fsck` and
  `crashcheck` after each: every file reported durable whole, and what exists a prefix of the writer's order. One more
  round writes, never syncs, waits 8 s and kills: ADR-0010 rule 5. The seed is printed, and `--seed` repeats the delays.
- `t_gui`, run by the test kernel in `auto-default` and `tcg-default`, which attach the GUI devices: it starts the real
  drivers and compositor with `init`'s code, plays clients and a fake input driver against them (damage outside the
  surface, absurd sizes, a memory object smaller than declared, a buffer a child rewrites during composition, a client
  that never reads, a flood of commits, input and manager ordinals on a client channel, F12 and focus), kills and
  restarts the GPU and input drivers, swaps in a display driver that never answers a flush (`guibench hang-display`)
  and sees keys and frame callbacks go on and the driver reported hung, and measures a full redraw. Without a GPU it passes having done nothing.
- `t_streams`, `t_image`, `t_suspend` and `t_snapshot` check ADR-0101, ADR-0108, ADR-0105 item 8 and ADR-0102 from
  a process; the test kernel runs each twice and compares frame and heap counts. `t_snapshot` runs itself in a
  `posix` job: both halves of a snapshot run with their own memory and moved handles, a register dump compares
  the halves, the child's table is compared slot by slot, and a sweep gives back one frame at a time from an empty
  allocator until the call succeeds, checking that each refusal returned every frame and byte of heap. Test calls
  (`syscall::test_calls`) hold frames and read a process's table for these.
- `t_storage`, run by the test kernel, kills the driver idle clean, idle dirty, in a commit and in a read, then the
  service, and checks the durable file after each remount. It sends malformed messages and hostile paths, fills the
  open table, and never reads its replies.
- Mutation testing is outside `make test`. `make mutants` applies the hand-written mutations of `tools/mutants/*.toml`
  in at most two parallel worktrees of `HEAD` (`build/mutants/w<i>`, kept between runs, about 400 MB each), runs only
  the tests each list names, and writes the survivors to `build/mutants/report.md`. `make mutants-host CRATE=name`
  runs cargo-mutants on one host crate, two jobs at a time. Both run under `tools/mutants/watchdog.sh`, which kills a
  process past a memory limit (its physical footprint on macOS, `tools/footprint`), and every host test caps its own heap at 512 MiB (`crates/zoraya-test-alloc`), so a
  mutant that grows without bound fails instead of taking the host (`docs/backlog.md`). The `mutant-triage` and
  `test-runner` subagents (`.claude/agents/`) read and run these; CLAUDE.md says which work goes to them.
- Tests inspect a fault with `arch::catch`, which runs a function and returns the fault that ended it. That path
  exists only in the test kernel: the normal kernel cannot resume a fault.
- Nine tests fail on purpose (`--expect-panic`): the harness self-tests, and the fault reports for `brk`, an unmapped
  access, an unexpected interrupt, an EL0 exception with no process, and a stack overflow on the boot stack and on a thread
  stack. `--expect-line` checks what the reports say.
- `tools/test-runner` applies a timeout, and fails on a missing `begin` or `end` line, a count mismatch, a panic
  outside a test, a kernel that does not power off, or a non-zero QEMU status. The guest's exit status is not used as
  a signal.

The timer test allows a shortfall of 20 % of the ticks over 200 ms, because the host can deschedule the vCPU (HVF) or
the TCG thread, and ticks then arrive late. Too many ticks always fails: no deadline fires early.

Host tests: `zoraya-pmm`, `zoraya-heap`, `zoraya-pagetable` and `zoraya-time` each have a model test with a seeded
generator, so there is no property-testing dependency. `zoraya-pagetable` decodes descriptors bit by bit with a walker
that shares no code with the crate.

`zoraya-fs` has one test file per invariant family (`crates/zoraya-fs/tests/`), all deterministic and run by
`make test-unit`; `docs/adr/invariants.md` names the file that checks each invariant:
- `model.rs`: random request sequences against the reference model in `zoraya-fs-testkit`, with clean remounts and
  four clock behaviours;
- `crash.rs`: every crash image a recording device allows within a stated bound (all subsets of a window's writes
  when it has at most 12, else every prefix and 64 random subsets, plus 16 torn per-sector variants), each checked,
  mounted and compared with the model's allowed prefixes; the ordering of writes and flushes is checked against the
  commit protocol;
- `faults.rs`: every write, flush and read of request sequences failed in turn, and metadata corrupted on read;
- `integrity.rs`, `units.rs`, `space_time.rs`, `workload.rs`, `basic.rs`;
- `decoder_fuzz.rs`: field-level (eight transformations, checksums recomputed) and random image mutation, with an
  allocation-counting allocator for the decoder's memory bound.

Coverage-guided fuzz targets for the same decoder are in `fuzz/` (nightly, cargo-fuzz; see `fuzz/README.md`).

`zoraya-dtb` is tested against device trees dumped from QEMU (`crates/zoraya-dtb/tests/fixtures/`, with the exact
commands) and against malformed input: every truncation, bad header fields, structure and string errors, and 30,000
deterministic random mutations.

## Channels, memory and devices

- **Memory objects (`kernel/src/memory/object.rs`, ADR-0051).** Frames committed and zeroed at creation, contiguous on
  request, held by handles, mappings and pins. The kernel chooses addresses in a window of the user range, with an
  unmapped page between mappings. `memory_unmap` ends one whole mapping: the range must be exactly that mapping's.
- **Channels (`crates/zoraya-channel`, `kernel/src/channel.rs`, ADR-0050).** Two ends with bounded queues, all or
  nothing on write, a message left in place on any refused read. An end has one handle. Closing runs a work list
  through the messages, so a deep chain takes no stack.
- **Protocols (`crates/zoraya-protocol`, `zoraya-proto-block`, `zoraya-proto-file`, ADR-0052).** The `protocol!` macro
  and a flat wire format with total decoders. `blkd` and `fsd` serve them (TASK-13).
- **`init` (`user/src/bin/init.rs`, ADR-0053).** The one process the kernel starts, with the console, its own job, the
  ramdisk directory, `Power` at slot 4 and `Device` handles from slot 5. It starts `zosh` in a sub-job with only what
  the shell needs, and turns the machine off when the shell ends.
- **Devices (`kernel/src/device.rs`, ADR-0054, ADR-0070).** One `Device` per page of `virtio,mmio` transports. A window
  has one holder at a time. An interrupt object masks its line when it fires, until `interrupt_ack`. `dma_pin` writes
  the object's physical runs as device addresses, and `dma_unpin` ends the pin and its handle. A pin that ends any
  other way leaves its frames quarantined. `dma_release_quarantine` is refused while the device has a mapping or a live
  pin, and otherwise the kernel resets each transport, sees Status 0, and only then frees the frames.

## Stage 6 kernel objects (as built)

TASK-16, on the accepted [ADR-0101](adr/0101-byte-streams.md), [ADR-0102](adr/0102-snapshot-process.md),
[ADR-0105](adr/0105-terminal-and-job-control.md) item 8, [ADR-0108](adr/0108-programs-from-the-filesystem.md) and
[ADR-0109](adr/0109-building-the-toolchain.md) item 5, with the choices they leave open in
[ADR-0130](adr/0130-kernel-object-choices-of-stage-6a.md) (proposed). Eight calls, numbers 42 to 49, all in the one
`syscalls!` declaration.

- **Byte streams (`crates/zoraya-stream`, `kernel/src/stream.rs`).** `stream_create` gives a read end and a write
  end, two object types, over one 64 KiB ring in frames committed at creation (a memory object nobody can name). The
  ring's rules are host-tested data: a write of at most 4096 bytes is all or nothing and needs 4096 free; a longer one
  takes what fits; a read takes what is there; nothing blocks. Each handle to an end holds that end's `Arc`, in a
  table, a channel message or a snapshot's child, so `PEER_CLOSED` comes from the last one dropping. The user copy
  goes straight between user memory and the ring's frames, so a transfer allocates nothing.
- **`snapshot_process` (ADR-0102).** Refused with `AccessDenied` outside a `posix` job; the only
  `#[personality_only]` call. One flat buffer (`ZSN1`, `zoraya_abi::snapshot`), read once. Every check comes first,
  then the child's table: every slot at the parent's generation, duplicable types (`OBJECT_TYPES` marks each)
  duplicated where the handle has `DUPLICATE`, listed handles moved to their slots, the self slot a `WAIT` handle to
  the child. Then the eager copy (`AddressSpace::snapshot`): a fresh frame for every page the process owns, a fresh
  memory object for every mapped one, data cache cleaned to the point of unification for executable pages and the
  instruction cache invalidated once. A device mapping or a pin refuses it. The child's thread starts from a copy of
  the caller's exception frame with x0 `Ok` and x1 0, and its `TPIDR_EL0`; dispositions are copied, the request queue
  is empty. Only after the child exists do the moved handles leave the caller.
- **`image_create` (ADR-0108).** `zoraya-elf` parses an ELF in a memory object in two steps (header, then table)
  with the same rules as the ramdisk; the image keeps the checked headers, a copy of the header table and the object.
  `spawn` copies each segment's bytes from the object then. Rewriting the object after the call harms only the
  children spawned from it afterwards.
- **Startup block (ADR-0109 item 5).** `argc`, `envc`, a header size of 16 and the number of program headers, the
  strings, zeros to a multiple of 8, then the ELF's whole program header table, for `AT_PHDR`.
- **`job_suspend`, `job_resume`.** Need `KILL`. A flag on every job of the tree and `SUSPENDED` on each; every
  process is marked stopped, and its thread parks on its next way to EL0, so none runs a user instruction after the
  call returns (one CPU). A process or job that joins a suspended tree starts stopped. Suspends do not nest.
- **`object_get_signals`.** The bits asserted now, with `WAIT`: `WNOHANG` without a port.

Measured on the M4 under QEMU 11.1.2, `make test`'s test kernel (128 MiB; `auto-*` under HVF, `tcg-*` under TCG),
from `t_streams`, `t_suspend` and `t_snapshot`, in the final run of TASK-16 (a clean clone at `15c554a`), each
program twice per configuration. The "1 MiB" and "8 MiB" processes are the test program with a mapped object of that
size; the program's own pages are about 1.2 MiB, most of it its user heap, so the copies are 2.2 and 9.2 MiB.

| | HVF | TCG |
|---|---|---|
| Stream transfer, 64 KiB writes and reads in one process | 1.0 to 1.8 GB/s | 265 to 269 MB/s |
| `job_suspend` of a job of 16 running processes: mean of 20; worst | 6.1 to 6.5 µs; 8 µs | 82 to 86 µs; 118 µs |
| `snapshot_process` of the "1 MiB" process, worst of 5 masked; pages copied; frames charged | 0.40 to 0.42 ms; 563; 590 | 4.6 to 4.8 ms; 563; 590 |
| `snapshot_process` of the "8 MiB" process, the same | 1.39 to 1.44 ms; 2355; 2385 | 14.0 to 16.5 ms; 2355; 2385 |

The frames charged are the pages copied plus the child's tables, kernel stack and object records. The copy runs
with interrupts masked for the whole call, at about 6.5 GB/s under HVF and 550 to 650 MB/s under TCG. Earlier runs,
while the copies of mapped objects were still zeroed first (TASK-16 review, R4), were about a fifth slower, and under
TCG on a busy host the 8 MiB case reached 49 ms. **Does it need breaking up?** Not for stage 6: dash at about 1 MiB
`[assumption, ADR-0102]` stalls about 0.4 ms under HVF and 5 ms under TCG, within the stalls the kernel already has
(`memory_create` of all memory, 9 ms; a device reset, up to 100 ms). An 8 MiB process stalls TCG for one and a half
10 ms slices; nothing in T1 forks one. It must be bounded with the others before untrusted code runs (ADR-0056), and
copy-on-write (ADR-0102's revisit) removes it.

## `libposix` core (as built)

Stage 6c (TASK-20), on the accepted [ADR-0100](adr/0100-libposix-construction.md),
[ADR-0103](adr/0103-file-descriptors.md), [ADR-0107](adr/0107-namespace-table-and-temp-service.md) and
[ADR-0153](adr/0153-what-a-program-gets.md) rule 11, with the choices they leave open in
[ADR-0180](adr/0180-libposix-core-choices-of-stage-6c.md) (proposed).

- **The open description (ADR-0103).** `fsd`'s record of an open object holds a position and the append flag.
  `zoraya-proto-file` gains `share` (another connection to the same description, `clone`'s rules), `seek`,
  `set_flags`, `get_flags`, reads and writes at the position (inline and through the buffer) that move it inside the
  request, `open_with_mode` (the POSIX mode of a create, ADR-0014 rule 6, answered with the object's kind, mode, ids and
  times and whether this open made it), `stat` (the volume's id, the object's id, exact times) and a listing with each
  entry's id (`read_directory_ids`), ordinals 23 to 33.
  No stage 4 request changed.
- **The bootstrap record (ADR-0107).** `zoraya_abi::posix`: `ZPB1` with the process's facts, `umask`, cwd as an entry
  and a path, each fd's kind and flags, and the namespace entries longest first with their handles in the message;
  the fixed slots `BOOTSTRAP`, `BROKER`, `PROCESS_JOB`, `SELF` and `REPORT` at 1000 to 1004; a second message brings
  `SELF`; `ZPR1` is a refused-path report (ADR-0153 rule 11). Both decoders are total.
- **`crates/zoraya-posix`.** The Rust half of `libc.a`. Its logic has no `unsafe` and is tested on the host, the file
  functions against the real `zoraya-fs-service` loop through a fake kernel (`tests/kernel`): the resolver (longest
  prefix on component boundaries, `..` lexically, in both of ADR-0153's forms, links followed to 40), the fd table
  (`dup`, `dup2`, per-fd `FD_CLOEXEC`), the `errno` table, `struct stat` from the service's metadata, and the file,
  directory, descriptor and pipe functions. `ffi.rs` is the C boundary: start-up from the startup block and the
  record, `malloc` and Rust's allocator on one `zoraya-heap` over memory objects, private anonymous `mmap`, the shim's
  targets, and `ENOSYS` for the process functions of 6d. A path outside every entry is `ENOENT` and is reported
  once on `REPORT`, at most 16 a process; a missing path inside an entry is never reported.
- **`posixrun`** (`user/src/bin/posixrun.rs`) runs a C program from the volume through `image_create`, with a table
  from its options, fds 0 to 2 (`/dev/null` and a stream it copies to the console, or a file shared into fd 1), a
  `BROKER` channel nobody serves whose messages it counts, and the `REPORT` channel, whose reports it prints. It is
  6c's launcher until the broker.

Measured on the M4 under QEMU 11.1.2, `make test-libposix` (the normal kernel, 128 MiB, a 32 MiB image), one run:
`posixrun -s` for the round trips from a native client (mean of 200), `t_libposix bench` for the rest, through
`libposix` from C (an existing file opened and closed 200 times, `lseek(fd, 0, SEEK_CUR)` 500 times, 8 MiB written and
read in 4 KiB and in 1 MiB calls), and the library's own count of the resolver's time (`ZORAYA_POSIX_STATS`).

| | HVF | TCG |
|---|---|---|
| `seek` round trip; `share` round trip (native) | 11 µs; 18 µs | 295 µs; 467 µs |
| `lseek` from C; `open` + `close` from C | 7.1 µs; 15.9 µs | 217 µs; 545 µs |
| The resolver, per path (mean over the run) | 0.32 to 0.46 µs | 11 to 25 µs |
| 4 KiB calls: write, read | 102 MB/s, 54 MB/s | 9.2 MB/s, 5.8 MB/s |
| 1 MiB calls: write, read | 399 MB/s, 152 MB/s | 107 MB/s, 21 MB/s |

An `lseek` is one round trip, and `open` one plus a channel and a close; the resolver is a few per cent of an `open`.
Writes are into `fsd`'s cache of dirty blocks; reads come from it.

## Console input and the first process

- **Console.** Raw bytes from the UART's receive interrupt, in a fixed ring (`crates/zoraya-console`), with a
  `READABLE` signal and `console_read`, which never blocks. The kernel has no line discipline (ADR-0060).
- **First process.** `init` (ADR-0053). The kernel reports it and powers off if it ends.

## Using the GUI

`make run-gui` boots with a GPU, keyboard and tablet and opens QEMU's window; the serial line stays in the terminal for
`init`'s reports, and Ctrl-A X there quits QEMU. The window shows a terminal running `zosh`, with the same commands as
`make run` (below). Click the window to focus it; F12 moves focus to the next window. Shift with Page Up or Page Down
scrolls back. Programs started from the shell print to the serial line, not the window (Known limitations). `exit` closes
the window and `init` opens a new one with a new shell; `poweroff` turns the machine off.

## Using the shell

`make run` boots to `zosh`, the native shell (TASK-18; ADR-0150 to ADR-0158), on the serial console: `Type help to
begin, or help tour.` and the prompt `~> `. Ctrl-A X leaves QEMU. The language is described by `help language` and
the tour (`docs/research/shell/tour.md`); this is what exists after TASK-18 step 6.

- `make run` attaches `build/disk.img`, made from `rootfs/` on first use; delete it to start again. The shell starts
  in `/home` (`shell.home`), shown as `~`; `/config` holds `settings`, `start.zosh` and `history` (ADR-0157). Without
  a disk the shell starts in `/` and the file commands fail.
- **Language.** Words split only at spaces; `'...'` is literal, `"..."` expands `$name` and `$(...)`. Variables are
  lists (`set name 'my notes.txt'`, `$name` is one argument), never split or globbed; `$name[i]`, `count $name`.
  `*`, `?` and `**` match names, a pattern that matches nothing is an error, and a match starting with `-` comes as
  `./-name`. `if`/`else if`/`else`, `for NAME in WORDS`, `while`, `fn NAME PARAMS`, `try`/`catch`, `&&`, `||`, `!`.
  A failure nothing checks stops the rest of the line (`(the rest of the line was not run)`), block, function or
  script (`zosh: FILE line N: NAME: WHAT`). `$status` is the exit code or a word (`interrupted`, `fault`, ...);
  `status` describes the last record. `>`, `>>`, `2>`, `&>` redirect builtins and functions; `export NAME VALUE`
  and `NAME=value command` are the only environment a program gets.
- **Commands.** Every command has a description in `user/describe/` (and the language's own in
  `crates/zoraya-shell/describe/`): `NAME --help`, `help NAME` (paged), `help --search WORD`, Tab completion and
  argument parsing all come from it. Builtins: `cd`, `pwd`, `set`, `unset`, `export`, `count`, `echo`, `test`,
  `math`, `source`, `status`, `history`, `help`, `grants`, `jobs`, `fg`, `bg`, `wait`, `kill`, `settings`, `slots`,
  `uptime`, `exit`, `poweroff`, `reboot`. Until step 7 the file commands `ls`, `cat`, `mkdir`, `rm`, `mv`, `stat` and
  `sync` are built in too; `ls /zoraya/bin` lists the ramdisk's programs. Errors read `NAME: SUBJECT: PROBLEM`
  (`cat: missing.txt: not found`), with a `hint:` when one is known; unknown commands get the nearest names or, for
  `grep`, `wc`, `man`, `less`, `more` and `which`, what to type on Zoraya.
- **Programs** from the ramdisk run by name in a job of their own, with only the console (and, for the test programs
  that need them, a job, the ramdisk or the volume). A trailing `&` runs one in the background (`[1] sleep 30`);
  `jobs`, `wait [%N]`, `kill [--interrupt|--terminate|--hangup|--quit|--kill] [%N]` and `fg [%N]` act on job
  numbers, the smallest free from 1. A job's end is reported once, before a prompt or by `wait` and `kill`.
- `posixrun [-m PREFIX=PATH[:ro]]... [-C CWD] [-o FILE] [-e NAME=VALUE]... PROGRAM [ARG]...` runs a C program from
  the volume through `libposix`, with `/` the volume's root unless `-m` gives a table; its output comes to the console
  (stage 6c; `user/src/bin/posixrun.rs` has the rest). It is a test program: it has no description, so its
  options are its own.
- **Keys** (`help keys`): Left, Right, Home, End and the Ctrl forms, Ctrl-W, Ctrl-U, Ctrl-K, Ctrl-L; Up and Down walk
  the lines that start with what is typed; Ctrl-R searches; Tab completes; Right at the end takes the faint
  suggestion from history; the command word is red while it names nothing. Enter with an open quote or block
  continues on `...> `. Ctrl-C drops the line, or interrupts the job in front (the third one kills it); Ctrl-\
  sends Quit; Ctrl-D only says `type exit to end the shell`.
- **Ending.** `exit` ends this shell and `init` starts a new one, on the console or in a new terminal window; with
  jobs running the first `exit` only warns. `poweroff` syncs and turns the machine off. `reboot` says it is not
  available until the kernel has a reset call (proposed ADR-0159).
- **Settings** (`help settings`): `settings`, `settings --all`, `settings NAME [VALUE]`, `settings --reset NAME`,
  `settings --check`; `shell.prompt`, `shell.home`, `shell.path`, `shell.history`, `shell.history-size`,
  `shell.suggestions`, `shell.colour`, `console.columns`.

**Pending (TASK-18 Part 2).** Step 7: grants (`+r`, `+w`, designated operands, `grants` as ADR-0153 words it) and the
file commands as programs (`cp`, `head`, `find`, `search` arrive then); `grants` today says what a program gets.
Steps 8 and 9 after stage 6a: byte streams, pipelines (`|` is refused with a message), redirection of programs, `$(...)`
of a program, Ctrl-Z, `bg`. Step 10 after 6d: POSIX programs. Step 11: the tour as a test on both terminals.
Step 12: `reboot` (ADR-0159).

## Known limitations

- **Masked interrupts.** System calls run with interrupts masked, and some scale with what they touch. Under HVF on
  128 MiB: `memory_create` of all memory about 9 ms, and closing that object about 4 ms; closing a chain of 20,000
  channel ends about 11 ms. `memory_map`, `dma_pin`, an address space's teardown and a device reset, up to 100 ms a
  transport, are the same kind. ADR-0056 must bound them before untrusted code runs.
- **Signal table.** `zoraya-events` keeps its peak size. A burst of channels leaves that memory in the kernel heap.
- **Handles in messages.** A protocol decoder checks how many handles a message has. A server must then check their
  types with `handle_type` (ADR-0072).
- **Snapshot stall.** `snapshot_process` copies with interrupts masked: 0.4 ms for a 2.2 MiB process under HVF,
  about 16 ms for 9.2 MiB under TCG, more on a busy host ("Stage 6 kernel objects (as built)"). Suspends do not nest, and a resume of a
  sub-job runs it while its parent stays suspended (ADR-0130 item 7). A stopped thread blocked in the kernel finishes
  its call before it parks. The stream ring is 64 KiB per stream, committed, with no quota (ADR-0056).
- **No untrusted code.** Nothing limits what a process may cost the system: no job has a quota. So no untrusted code
  runs in Zoraya until [ADR-0056](adr/0056-job-memory-accounting.md) is accepted and implemented. At stage 4 every
  process is trusted.
- **Console input.** Input is a fixed 256-byte ring (ADR-0060). A burst overruns it while the shell waits for the CPU,
  and the shell reports how many bytes went. Any holder of the console's `READ` right can take input from the shell;
  children get `WRITE` only.

- **Shell (TASK-18 Part 2, steps 1 to 6).** Programs print to the console, not through `zosh`: until step 8 there
  are no streams, so `|`, a program's redirection and `$(...)` of a program are refused with a message, and keys
  typed while a program runs reach nobody but Ctrl-C and Ctrl-\\. The file commands are builtins with the shell's
  own access until step 7, and `grants` says so. A line wider than the terminal is drawn again on a new row when
  edited away from its end (ADR-0154's subset has no cursor-up; its open questions). `settings` rewrites
  `/config/settings` in place, not by publishing a new file (ADR-0157's open questions), and `/config/history` is
  rewritten after every line. Descriptions are compiled into `zosh` as well as kept in each program and the
  ramdisk: no call reads a ramdisk entry (ADR-0156's open questions). The third Ctrl-C kills a job, as before
  ADR-0158 (step 9). `reboot` is not available (ADR-0159, proposed).
- **GUI.** Programs started from `zosh` in the window print to the serial console: they have no byte stream to inherit
  until stage 6 (ADR-0096). The font is ASCII; everything else draws a box. One keyboard layout (US), one display, no
  resizing, no window decorations beyond a frame, no clipboard. F12 is reserved by the compositor. The compositor keeps
  a copy of every surface (up to 8 clients, each up to the display's size), bounded only by those limits until
  ADR-0056. Attaching a new display driver still waits for it, up to 2 s per call; `init` blocks up to 6 s while it
  restarts a driver (backlog: to fix before stage 6). NUL cannot be typed in the window. A QMP socket's path must be under 104 bytes
  on macOS, so the tests keep it in `build/`. The runner's key latency figure under HVF is bounded by the cost of one
  screendump.

- **QEMU `virt` assumptions.** The RAM base `0x40000000` (where the device tree and image are found), the fallback
  UART address, and the boot identity map's shape (first 4 GiB, image below 4 GiB) are QEMU `virt` facts. The load
  address is fixed by the ELF; KASLR is not implemented.
- **Early faults.** Before the vectors are installed at the link address, a fault cannot be reported. A CPU without
  the 4 KiB granule parks silently.
- **UART set-up.** The PL011 baud rate is not programmed; QEMU ignores it. Two prints can interleave: the console has
  no lock, so that the panic path can always print.
- **Boot image mapping.** The boot tables map the image writable and executable until the final tables replace them.
- **Direct map.** A kernel write primitive still reaches all ordinary RAM through it (ADR-0020).
- **Memory.** Page tables left empty by an unmap are not freed; later maps go below them, so a block cannot be used
  where an empty table remains. The heap never shrinks, and memory in a size class
  stays there. Frames are not zeroed by the allocator, except for table frames. No double-free detection in the heap.
- **Threads.** No priorities, and kernel paths are not preempted. One thread per process. `sleep_until(Infinite)`
  never returns, and nothing can kill a process yet. Sleepers share a deadline queue of 32, with one slot kept for the slice, so a full
  queue fails `sleep_until` with `NoMemory`. Thread and process objects use `Arc::new`, which panics when memory runs
  out, so stage 3's `spawn` needs a fallible form first (ADR-0022).
- **User memory.** Copies are a byte at a time. At most 255 live processes. A process's memory is all mapped up front.
- **ABI table.** `SYSCALLS` is written by hand and not connected to the dispatcher, so the lints over it are weak.
  Stage 3 must build the dispatcher from it, or test that they match (ADR-0032).
- **Console.** `console_write` is a stand-in until the terminal service (ADR-0032), and any process can use it. A
  call takes up to 4 KiB and holds off interrupts while it prints, about 60 ms under QEMU.
- **PMU under HVF.** `PMUSERENR_EL0` is 0, and TCG traps EL0's PMU accesses, but the review saw them work at EL0 under
  HVF. It looks like QEMU's HVF register emulation, which a guest cannot change, so a process can read a cycle counter
  there.
- **Tests.** The suite needs more memory than the 32 MiB of stage 1: `running_out_of_asids_is_an_error` needs room for
  255 live processes.
- **PAN** is not used. A stray kernel access to user memory would not fault.
- **Interrupts.** One priority, no nesting. FIQ and SError reports are untested, since nothing raises them.
  Interrupt handlers must not panic in a test that expects recovery: the interrupt would stay active.
- **Emergency stack.** A second overflow, on the emergency stack, halts silently. The deepest use measured is the
  stack overflow report: 5,552 of 32,768 bytes.
- **Faults while a lock is held.** Recovery in the test kernel does not release locks, so a test that panics while
  holding one makes later tests fail. The normal kernel halts on panic.
- **Single core.** Only the boot CPU runs, with one emergency stack. The GIC, timer and lock are set up for it alone. Before SMP: on GICv2,
  `gic::acknowledge` must keep the source bits of an SGI for the EOI and mask them from the ID.
- **HVF limits.** HVF does not support GICv2 (`gic-version=2` fails to start). EL2 entry under HVF
  (`virtualization=on`) needs HVF nested virtualisation: an M3 or later and a recent macOS. It works on the M4 with
  QEMU 11.1.2. On an M1 or M2, QEMU stops at VM creation (`Nested virtualization not supported on this system`;
  seen on an M1 with QEMU 11.1.3). Before `auto-el2` (`make test-qemu`) and `boot-auto-el2` (`make test-boot`),
  `tools/run-qemu.sh --probe-el2` creates the VM with `--dumpdtb`, once without and once with `virtualization=on`.
  If only the second fails under HVF, the configuration prints `[LABEL] skipped: HVF on this Mac has no EL2 (needs M3
  or later)` and does not run. `make test` counts this as a skip, not a failure, and lists it in its summary. On such
  a Mac EL2 entry is covered only by TCG (`tcg-el2`, `boot-tcg-el2`, which always run). Under TCG (CI) nothing is
  probed. The probe tests the host, not the kernel: if HVF creates the EL2 VM, a kernel failure at EL2 still fails.
- **Storage stack.** `t_storage` kills the driver "during a commit" and "during a read", but on one CPU the driver
  ends before the request reaches the device, so no test kills a driver with a write in flight on the device; the
  kernel's quarantine of TASK-12 is what covers that case. No test sees whether a flush reaches the device: the crash
  test keeps the host's page cache (below), and the engine's host crash test is what catches a skipped flush. `slots`
  calls `handle_type` with generation 0, so a handle in a reused slot is not shown: the kernel has no call to list a
  table. One disk, one queue, no spawning from the volume: programs still come from the ramdisk. A
  restart ends the shell with the rest, so the current directory and the jobs go. After three restarts in a boot the
  volume stays down until the next boot, with nothing measured behind three. The crash test kills QEMU, so it stops the
  writer at a random point but does not lose unflushed writes: QEMU has already given them to the host. Losing and
  reordering them is tested only on the engine's simulated device. `make test-qemu` shares one image among its
  configurations.
- **Filesystem.** CRC32C is
  table-driven (no CPU instructions, ADR-0016). A volume is at most about 20.9 GiB. One copy of each block: a bad
  metadata block makes the volume read-only until offline repair, and a bad newest tree root block makes it
  unmountable. The first create after each mount costs a commit (id reservation). A read-write mount holds the
  allocator bitmap in memory (one bit per block). `fsck --repair` cannot repair damaged tree blocks. Requests are
  served one at a time.
- **Ports.** The ported programs are built against the real `libc.a` but not run yet: they need 6d's `posix_spawn`,
  `fork`, `wait*`, signals and terminal, which fail with `ENOSYS` until then. dash's job-control patches (ADR-0142) are compiled, not run: the broker of 6d is their
  first test. toybox has no mount table, file notification or path-based extended attributes on Zoraya (its
  portability patch fails them); no T1 applet uses them. tcc's host check uses a tcc compiled for the host from the same source, not the Zoraya binary (ADR-0143). Programs are
  installed with their debug information.
- **CI.** The CI workflow (`.github/workflows/test.yml`) runs `make fetch-ports` and `make test` on Linux with TCG
  (the fetch step for `make test-libposix`'s `libc.a`, ADR-0109) and, in a second job, `make ports`.
- **`libposix` (stage 6c).** Process functions, signals, the terminal, `/tmp` and FIFOs are 6d's and fail with
  `ENOSYS`. A file read or write through a bad pointer faults the process instead of `EFAULT` (streams give
  `EFAULT`); listings have no `.` or `..` (`d_ino` is the object's id, as `st_ino`); `O_TRUNC` of an existing non-empty file is a
  second request; `CLOCK_REALTIME` is the monotonic clock `fsd` stamps files with; `st_blocks` comes from the size;
  times before 1970 cannot be set (ADR-0180). `posixrun` is the only launcher of C programs until the broker, and only `zosh` names it.

## Roadmap

| Stage | Content | Done when |
|---|---|---|
| 0 (done) | Boot, UART output, panic handler, device tree parsing, test harness | `make test` passes |
| 1 (done) | Exception vectors, GIC, timer, MMU (kernel in upper half), physical frame allocator, kernel heap | timer ticks; `Box`/`Vec` work in the kernel |
| 2 (done) | Threads, context switch, scheduler, EL0, first native syscalls, ELF loader, initial ramdisk | a user process prints through a native syscall |
| 3 (done) | Handles, ports and process objects per ADR-0003/0004/0006; native `spawn` | a process spawns a child and waits on its exit via a port |
| 3a (done) | Native console shell (TASK-11): `zosh` over the serial console, no files | a person works at a prompt after `make run` |
| 4 (done) | virtio-blk, filesystem service, `mkfs` on host | files survive reboot |
| 5 (done) | virtio-gpu, virtio-input, compositor, terminal as GUI client | screenshot shows a terminal window; TASK-14: `zosh` runs in it and keys typed into it run file commands (`make test-gui`) |
| 6 | `libposix` T1 per ADR-0005, port of tcc, make, dash | `tcc hello.c && ./a.out` inside Zoraya |
| 7 | design system, text rendering, window management, keyboard layouts, scaling, toolkit, chooser, panel, file manager, editor, settings, clipboard, list settings, rooms (ADR-0160 to ADR-0172) | a person opens a file from the file manager in the editor through the chooser, in a window the compositor framed, with Ukrainian and Czech names on screen, at scale 1 and 2 (`make test-gui`) |

## Where things are

```
CLAUDE.md                 rules for agents
.claude/agents/           the test-runner and mutant-triage subagents
LICENSE                   BSD-2-Clause, except tools/posix-surface/patches/ (its README.md), ports/patches/ and ports/musl/zoraya/ (ports/README.md)
Makefile                  build, run, run-gui, debug, test-unit, test-qemu, test-boot, test-files, test-libposix, test-crash, test-gui, test, adr-check,
                          fetch-ports, ports, check-ports
rust-toolchain.toml       pinned stable toolchain and target
.cargo/config.toml        kernel runner (tools/run-qemu.sh) and cargo aliases
.github/workflows/        CI: make test on Linux/TCG
docs/
  design.md               this file
  dependencies.md         external crates (none) and host tools
  adr/                    architecture decisions, invariants.md, index.md
  research/               TASK-01..02b research record (do not modify)
  tasks/                  task specifications and their reviews
kernel/                   zoraya-kernel: boot, arch, machine-dependent code only
  linker.ld               image layout (linked high, loaded at 0x40200000)
  build.rs                linker script, git commit
  src/arch/aarch64/       entry, EL2 -> EL1, vectors, MMU control, timer, system registers
  src/dtb.rs              locating the device tree blob
  src/console.rs, pl011.rs  UART console
  src/memory/             layout, translation tables, frames, heap
  src/gic.rs, irq.rs      interrupt controller and dispatch
  src/timer.rs            monotonic clock and deadlines
  src/sync.rs             the kernel's lock
  src/sched.rs            threads and the scheduler
  src/process.rs, job.rs, object.rs, port.rs  processes, jobs, handle contents, ports
  src/channel.rs, device.rs  channels; device, interrupt and pin objects
  src/stream.rs           byte streams
  src/syscall.rs, ramdisk.rs  system calls, the embedded ramdisk
  src/psci.rs             SYSTEM_OFF
  src/panic.rs            panic handler
  src/testing.rs          in-QEMU tests (feature qemu-test)
crates/                   no_std, host-tested
  zoraya-dtb/             device tree parser and platform extraction; fixtures from QEMU
  zoraya-pmm/             physical frame allocator and range sets
  zoraya-heap/            kernel heap logic
  zoraya-pagetable/       translation table building and editing
  zoraya-time/            clock arithmetic and the deadline queue
  zoraya-abi/             the native ABI, with its checks
  zoraya-elf/, zoraya-ramdisk/  total parsers for programs and the ramdisk
  zoraya-handle/          handle tables: slots, generations, rights
  zoraya-console/         the console input ring
  zoraya-channel/         channel queues, bounds and the no-cycle rule
  zoraya-stream/          the byte stream's ring and its transfer rules (ADR-0101)
  zoraya-protocol/        the wire format and the protocol! macro
  zoraya-proto-block/, zoraya-proto-file/  the block and file protocols
  zoraya-shell/           zosh's language (parser, globs, evaluator over a host trait, ADR-0150, ADR-0151),
                          its line editor and exit records in words; describe/ holds the language builtins' pages
  zoraya-describe/        command descriptions (NAME.cmd): the total parser of ADR-0156
  zoraya-settings/        /config/settings: a total reader and a writer that keeps every other line (ADR-0157)
  zoraya-command/         what every native command shares: argv from its description, --help, error lines,
                          statuses in words, exit codes (ADR-0155)
  zoraya-events/          signals, port subscriptions and ready lists
  zoraya-rt/              the runtime of Zoraya programs (aarch64 only)
  zoraya-fs/              filesystem engine: format, tree, commits, requests, checker (ADR-0010..0016)
  zoraya-fs-testkit/      test devices, reference model, crash harness, image field map (host only)
  zoraya-fs-service/      the service's loop and block adapter, over a transport trait
  zoraya-virtio/          virtio-blk over split queues; a generic split queue, the device page identification
                          of ADR-0091, and a device-side fake (feature `fake`)
  zoraya-virtio-gpu/, zoraya-virtio-input/  the GPU and input drivers' logic (ADR-0090, ADR-0092)
  zoraya-proto-display/, zoraya-proto-input/, zoraya-proto-gui/, zoraya-proto-term/  the GUI's protocols
  zoraya-keymap/          key codes to text, the US layout (ADR-0092)
  zoraya-font/            Zoraya's bitmap font, its renderer and exact reader (ADR-0095)
  zoraya-term/            the terminal: VT-subset parser, grid, scrollback, keys to bytes, rendering (ADR-0096)
  zoraya-compositor/      the compositor's loop over a platform trait (ADR-0093, ADR-0094)
  zoraya-posix/           the Rust half of libposix (ADR-0100): resolver, fd table, errno, the file functions; ffi.rs
                          is the C boundary in libc.a; tests/kernel is a fake kernel over the real fs-service
user/                     the programs in the ramdisk, on zoraya-rt: init, zosh, blkd, fsd, gpud, inputd, compd,
                          term, posixrun (the launcher of C programs, stage 6c), test programs
  src/stack.rs            starts the driver and the service
  src/gui.rs              scans device pages, starts and restarts the GUI (init and t_gui)
  src/command.rs          a described program's start: its arguments through zoraya-command (INV-0155-2)
  describe/               command descriptions (ADR-0156): builtins/, commands/, demos/, topics/, system/;
                          build.rs refuses a bad one, describe! puts one in its program's ELF
rootfs/                   the first contents of build/disk.img
tests/shell/              scripts the runner plays on the serial line
tests/gui/                the GUI script the runner plays over QMP
tests/posix-core/         stage 6c's C criterion program and the script make test-libposix plays
tools/
  run-qemu.sh             the only place that knows the QEMU command line
  dump-dtb-fixtures.sh    regenerates the zoraya-dtb fixtures
  test-runner/            zoraya-test-runner: runs the test kernel, checks the protocol, plays serial and GUI
                          scripts (serial text with SGR sequences dropped); zoraya-crash-test
  test-summary.sh         make test's summary, with the logs in build/logs/
  mutants/                zoraya-mutants and its mutation lists (*.toml)
  adr-check/              zoraya-adr-check: ADR-0001 invariants
  mkramdisk/              zoraya-mkramdisk: packs user programs, and their descriptions as NAME.cmd
  zoraya-mkfs/            creates a filesystem image from a host directory
  zoraya-fsck/            checks (and on request repairs) a filesystem image
  posix-surface/          TASK-02/02b measurement pipeline (unchanged)
  zcc                     the C compiler driver for aarch64-unknown-zoraya (ADR-0109)
  fetch-ports.sh          downloads and verifies the ports' archives (make fetch-ports)
  zoraya-ar/              the archiver: GNU ar with a symbol index
  posix-surface-check/    libc.a against the tier list, the svc scan, the hiding of other names
  zoraya-elf-check/       zoraya-elf over programs on the host (INV-0109-1)
  footprint/              a process's memory for the watchdog (macOS's physical footprint), and memhog
ports/                    stage 6b: sources.lock, the build of the sysroot and the ports, patches (ports/README.md)
fuzz/                     cargo-fuzz targets for zoraya-fs and the shell's parsers (nightly; outside the workspace)
build/                    kernel ELFs, disk images, logs/ and mutants/; ports/, sysroot/, rootfs-posix/ (not committed)
```

`libposix`'s file, descriptor, pipe and memory half exists (stage 6c); its process half and the broker arrive in 6d.
