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

The surface is a list of measured CSV rows, and only an ADR can grow it. Out of scope, with written reasons: `ptrace`,
System V IPC, general `poll`/`epoll`, fault handlers, record locks across processes (`fcntl` record locks exist inside one
process, ADR-0319, which supersedes ADR-0005 item 6), `flock`, `dlopen`, set-uid, and T3 (clang/lld). ADR-0005 also excluded
threads and sockets; later ADRs added them: POSIX threads as tier T9 (ADR-0308, stage 9b, "Threads in a process" below) and
the stream client of the network tier (ADR-0225, ADR-0287, stage 8, "POSIX sockets" below).

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
copies and owns input and focus; a terminal that speaks a VT subset to the shell over a channel; and `init`'s GUI
mode. "GUI (as built)" below describes the code. Stage 7 (TASK-24) builds the desktop of
[ADR-0160](adr/0160-window-management.md) to [ADR-0172](adr/0172-rooms.md) on it, part by part; 7a replaced stage 5's
bitmap font with the design tokens of [ADR-0161](adr/0161-design-system.md) and the text of
[ADR-0162](adr/0162-fonts-and-text.md) ("Tokens and text" below).

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
  EL0, and kernel threads run until they yield or block. A 10 ms slice is armed only when another thread is ready,
  or when the thread's job has a CPU ceiling (below, "Quotas", Q4).
- **Thread stacks.** 64 KiB with an unmapped guard page, in the stack window.
- **Context switch.** Callee-saved registers, `FPCR`, `FPSR`, `TPIDR_EL0` and the stack limit. All else is in the
  exception frame. A thread ended is freed by a later one, since none can free its own stack.
- **Address spaces.** One table per process, with an ASID of 8 bits, so 255 live processes at most. User pages are
  not global. With no current process, `EPD0` is set and `TTBR0` is 0.
- **User layout.** Programs from 4 MiB up; a 64 KiB stack mapped in full, ending one page below 2^47, with an unmapped
  page below it.
- **Processes.** One address space, one handle table, and from one to 128 threads (below, "Threads in a process").
  All memory is mapped at creation, so there is no demand paging. A process ends in two phases: anyone else (or one of its
  threads) fixes the exit record once and wakes the first thread; each thread acts on it at its next return to EL0, or
  before its first instruction, and leaves. The thread whose leave empties the list releases the memory, ASID and
  handles, and the process stays as a zombie holding only its `ExitRecord` until its last handle goes.
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

### Threads in a process (stage 9b: the kernel, ADR-0300, 0301, 0307, 0313; `libposix`, ADR-0302 to 0306, 0314, 0315)

The code is `kernel/src/thread.rs`, the thread list and wait list in `crates/zoraya-thread`, and `t_thread` at the raw ABI.

- **Calls 56 to 61.** `thread_create(entry, stack, argument, tls)` makes a thread of the caller's process that starts at
  `entry` with `x0 = argument`, `SP = stack` and `TPIDR_EL0 = tls`, and returns a handle (rights `WAIT | SIGNAL`, not duplicable,
  signal `TERMINATED`). `thread_exit(clear, unmap_base, unmap_length)` unmaps its own stack if asked, stores 0 at `clear` and
  wakes its waiters, refunds its figures and leaves. `thread_self` makes a handle to the caller. `thread_interrupt(thread)`
  sets a one-shot flag that ends the target's next or current `futex_wait` or `sleep_until` early (`Ok` with value 1; no new
  `Status`); `port_wait` is not touched. `futex_wait(address, expected, deadline)` and `futex_wake(address, count)` are the
  wait on a word of the process's own memory (no requeue, no cross-process key). The wait reads the word and lists the thread
  with interrupts masked; the lists are made at a process's first `thread_create` or `futex_wait` and charged then.
- **Limits.** 128 threads a process, 1,000 user threads in the system; the deadline queue has 1,024 entries (a binary heap,
  at most 12 heap positions written by an insert, a cancel or a pop) and `on_interrupt` runs at most 64 due entries a pass.
  A process's mapping calls (`memory_map`, `memory_unmap`, the unmap in `thread_exit`, `snapshot_process`) take a sleeping
  per-process lock for the whole call, steps included, released by a guard on every path.
- **Ending.** `request_exit` fixes the record and wakes one thread, never all (`Job::kill` calls it under the tree's lock).
  The first thread to leave wakes the others in steps of 64 and each leaver wakes the new first one. `job_suspend` parks every
  thread at its next return to EL0; `cont` wakes one parked thread, which wakes the rest in steps. A fork takes the same park
  for its siblings (`thread::Freeze`) while `snapshot_process` copies.
- **Quota.** `threads` is a field of its own (`QuotaField::Threads = 3`), counts every live thread (the first of each process
  included), is charged after the process's slot and before its memory, and is refunded at `thread_exit`. The limits
  record's bytes 16 to 20 hold it (0 = none) and the usage record is 152 bytes. The room keeper derives `3 x procs`; `init`
  gives the session 600.
- **`libposix` and musl (first slice, ADR-0314).** musl's `src/thread` is compiled over the shim: `__clone` (Rust, in
  `crates/zoraya-posix/src/ffi.rs`) makes the thread with `thread_create`, `SYS_futex` is `futex_wait`/`futex_wake`
  (`zoraya_posix_futex`), `SYS_exit` is `thread_exit` and `__unmapself` gives a detached thread's stack and object back. Two
  futex locks (`crates/zoraya-posix/src/lock.rs`): the library lock around `Posix`, the memory lock around the heap and the
  mapping table (256 entries). `make test-posix-threads` runs `tests/posix-core/t_threads.c` (create, join, detach, mutexes,
  condition variables, once, keys and `_Thread_local`, rwlocks, barriers, spin locks, semaphores, stdio).
- **`libposix`, waits, signals, cancellation, `fork` (ADR-0315).** A call that waits long returns `PENDING` inside the library
  and waits on a port of its own with the library lock free, then runs again with the outcome. A thread table
  (`crates/zoraya-posix/src/threads.rs`) holds each thread's signal mask and pending set; `pthread_kill` and `raise` are
  thread-directed (`tkill`, `thread_interrupt`) and a process's signal goes to the lowest-numbered thread that has it unblocked;
  `sigwait`, `sigtimedwait` and `sigwaitinfo` exist. Cancellation is deferred (`ports/musl/zoraya/cancel.c`: a flag plus
  `thread_interrupt`, tested at the library's wait points). `fork` with running siblings takes the locks
  (`ports/musl/zoraya/atfork.c`) and freezes the others for the copy; the child has one thread. `exec` holds the library lock
  until the old image ends. Tests: `make test-posix-threads` (`t_threads`, with 1000-round cases), `tests/posix/threads.script`
  in `make test-posix`, and `make test-posix-suites` (musl's libc-test thread tests, exclusions with reasons in
  `tests/libc-test/threads-excluded.txt`). Known limits: `PTHREAD_PROCESS_SHARED` and priority inheritance are refused
  (`ENOTSUP`), robust mutexes are `ENOSYS`, asynchronous cancellation acts as deferred.
- **Record locks (ADR-0319, which supersedes ADR-0005 item 6).** `libposix` keeps `fcntl` record locks in the process: `F_SETLK`, `F_SETLKW`, `F_GETLK`
  and the `F_OFD_*` forms (`crates/zoraya-posix/src/records.rs`, `fcntl_locks.rs`), released on `close` and `dup3`. Limit, by
  design: a second process does not see the lock (the file service has no owner per process, ADR-0103). `flock` is absent.
  `make test-posix-threadtest3` runs SQLite's `threadtest3` (rollback-journal scenarios; WAL needs a shared mapping, ADR-0100) on SQLite's
  default unix VFS, under HVF and TCG (INV-0319-3); `make test-posix-suites` runs musl's libc-test thread tests (ADR-0309).
- **Not done yet.** `grants` printing the figure, `t_bound`'s cases for 128 waiters and 4 x 128 threads, and the
  CPU-ceiling and stepped-map variants of the ending test (see the notes in ADR-0300, 0301 and 0307, and the report); the
  checks of INV-0306-3, INV-0306-5 and INV-0308-3, `check-cancel-points` (INV-0305-3) and the mapping count of a 128-thread
  process (ADR-0315's open questions); record locks that a second process sees (`docs/backlog.md`).

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
- **Storage limits (format 2, [ADR-0265](adr/0265-storage-limits.md), [ADR-0290](adr/0290-what-the-engine-charges.md)
  proposed).** A directory may carry a limit record `{ limit, used }` (`K_LIMIT`), and every file and link a link to the
  directory it is charged to (`K_FPARENT`). `used` counts, below the directory, one 4 KiB block per object and a
  file's or link's size in blocks; it is kept by every request that creates, grows, shrinks, removes or renames, and
  is part of the copy-on-write tree, so a crash leaves it true. A growing request walks the directories up to the
  root and fails with `OverQuota` if any limited one would pass its limit, changing nothing; a shrinking request is
  never refused. A file renamed across a boundary moves its charge; a directory refuses with `CrossesLimit`. An
  unnamed file is charged where it was made, and a file unlinked while open keeps its charge until freed.
  `set_limit(name, bytes | none)` needs `WRITE` on the parent's connection; `get_limit` reports the record. A new
  limit on a directory with content scans its subtree in one request. `zoraya_fs::check` recomputes every `used`.
  A growing one-block write costs 111 ns more (7.0 us against 6.9 us, release build, in-memory device) with a limit
  three levels above the file; `zoraya-mkfs --limit PATH=SIZE` limits a directory of an image.
  The file protocol has `OVER_QUOTA` (16) and `CROSSES_LIMIT` (17), `SetLimit` (38) and `GetLimit` (39); `libposix` maps
  them to `EDQUOT` and `EXDEV`. Known limitation: no program sets a limit yet except through the protocol and the
  image tool (the room keeper of Q6a will), and `stat`/`files` do not show it yet.
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
  **Accounts** ([ADR-0262](adr/0262-who-pays-for-service-objects.md) item 2, [ADR-0291](adr/0291-accounts-in-the-file-service.md)
  proposed). Every connection belongs to one account; `open`, `clone`, `narrow` and `share` join the caller's. The root
  connection's is the session's. `CloneAccount { rights, budget }` (request 40) makes a new account carved from the
  caller's: `budget` open handles, an eighth of that in mapped buffers and a thirty-second in held replies (at least
  one each). Carved from the session's, a budget is 1 to 32 and at most four such accounts exist (the untrusted
  column: 128 open, 16 buffers, 4 held replies; the session keeps 768, 224 and 892). At its open bound an account's
  requests get `TOO_MANY_OPEN` (`ENFILE`), at its buffer bound `LIMIT_REACHED`; a reply that cannot be sent and
  would be a second held one (the client's queue full; a `NoMemory` reply is held and retried) closes its connection. The bounds of all live accounts add up to `MAX_OPEN` (896),
  `MAX_BUFFERS` (240) and `MAX_HELD` (896); an account whose last connection closed and whose sub-accounts are gone is freed and returns its bounds (while a sub-account lives it stays with what it has left, so the untrusted column cannot be widened).
  A `Sync` on an untrusted account's connection is answered at the next commit (the timed one or another client's
  `sync`) and the connection is not read meanwhile; a durable write from such an account is folded the same way. Each refusal at an account's bound is counted for its launcher (`Service::account_refusals`). Worst-case memory
  (`zoraya_fs_service::worst_case`, 4-byte test handles): 192 KB fixed, 766 B per open handle, 64 KiB per held reply,
  so an untrusted account costs at most 90 KB and the whole service 59 MB, 56 MiB of it the session's possible held
  replies (as before accounts). Known limitation: no launcher calls `CloneAccount` yet (Q6a), and the loop serves
  ready connections in the port's order, not one request per account per pass.
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
  after a second. Since 7g (ADR-0246) it also looks for the display event every 100 ms between requests and tells
  the compositor a new size (`Resized`); a second `Attach` at a new size gets the other resource on scanout 0, then
  the old one is unreferenced and its pin released.
- **`inputd`** (`zoraya-virtio-input`, ADR-0092). One process per device; it reads the configuration to learn whether
  it drives a keyboard or an absolute pointer, keeps 64 event buffers offered, checks every event, and sends one
  `zoraya-proto-input` frame per `SYN_REPORT`, with axes scaled to 0..65535 and a pointer's wheels (`REL_WHEEL`,
  `REL_HWHEEL`, or their `HI_RES` axes when the device has them) as `WHEEL` records in 120ths of a notch, positive
  toward the person and to the right (proposed ADR-0322). A frame that does not fit is dropped and
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
  `zoraya-keymap`; pointer events to the surface under the pointer, and a frame's wheel turns, summed, as one `Wheel`
  (ordinal 46) to the toplevel or popup body under it, never during a move, a drag or the switcher, and with the
  modifiers only to the focused client (proposed ADR-0322). A button press focuses and raises; F12 moves focus
  to the next window and reaches no client. Events that do not fit a client's queue are dropped and counted for it.
  At most 32 clients (`zoraya_proto_gui::MAX_CLIENTS`); an untrusted room's are counted in its account, 3 a room and
  12 for all, refused `OVER_QUOTA`, so the session keeps 20 (proposed ADR-0323). With no display driver it goes on; a new one gets the same framebuffer and one full flush.
- **`term`** (`zoraya-term`, `zoraya-text`, ADR-0096, ADR-0162). A compositor client with an 80 × 24 grid of 8 × 18
  cells (JetBrains Mono at `text.mono`, 13 pixels at scale 1) inside `padding.terminal` (14), 668 × 460 pixels, and
  500 lines of scrollback (Shift with Page Up or Page Down, or the wheel, three lines a notch). It parses the VT subset of ADR-0096 with ADR-0154's faint
  (SGR 2, ended by 22: half way to the background), draws bold (SGR 1) with JetBrains Mono Bold, takes its colours
  from the `term.*` tokens of the light set, answers no query, and sends the shell the bytes keys make
  (`zoraya-proto-term`): it holds up to 4 KiB of keys the shell has not read yet and sends them as soon as the shell's
  queue has room. It draws only the rows that changed, and commits again only after the last commit's reply.
- **`zosh`** uses a channel at slot 6 as its terminal: output as `Output` messages (waiting while the terminal's queue
  is full), input from `Input` messages, and its width from `Size`. Without one it uses the console.
- **`init`** (ADR-0097). With a GPU page and an input page it starts the storage stack, `gpud`, an `inputd` per input
  page, `compd`, `term`, and `zosh` with the volume at slot 5, the terminal at slot 6 and the console to write only. A
  dead driver is restarted alone, at most three times each, and handed to the compositor over the manager channel. A
  dead compositor or terminal ends the GUI and the shell and starts them again, at most three times; then the shell
  runs on the console. A storage restart starts a new terminal with the new shell.

Measured on the M4 under QEMU 11.1.2, the last run of `make test` (`t_gui` in the test kernel at 128 MiB, the criterion
script at 512 MiB). A full redraw is a client committing its whole 1024 × 768 surface and waiting for the reply, which
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

## Tokens and text (as built)

Stage 7a (TASK-24), on the accepted [ADR-0161](adr/0161-design-system.md) and
[ADR-0162](adr/0162-fonts-and-text.md), with proposed [ADR-0240](adr/0240-fonts-embedded-until-the-launcher.md).

- **`zoraya-tokens`** reads `crates/zoraya-tokens/tokens.txt`, which is the architect's direction file
  (`docs/research/desktop/mockups/clear-field/tokens.txt`, compared by a test), at compile time with a `const`
  parser: a missing name or a bad value fails the build. Sizes are `Logical` values that only the crate can make and
  that become physical pixels only through a `Scale` (1 or 2); colours are `Color`s of the light or the dark
  `Palette`, blended in sRGB with integer arithmetic; durations are `Millis`. `make check-tokens` keeps colour and
  duration literals out of the compositor, the terminal and the GUI programs (INV-0161-1).
- **`zoraya-text`** is ADR-0162's Option C: a TrueType `glyf` reader that checks every offset (`cmap` 4 and 12,
  `kern` 0, composites at most 8 deep and 256 components, at most 4,096 points and 64 contours, else the box), a
  scan-line rasteriser in 26.6 fixed point whose coverage is exact integer arithmetic (the same bytes on every host and
  in QEMU: reference digests on the host and in `t_text`, INV-0162-2), a two-way set-associative glyph cache of 1,024
  glyphs that counts its hits, the `Shaper` trait with the simple shaper (fallback along a font list, combining marks
  over their base at no advance), the box glyph drawn by the rasteriser, and drawing that never leaves its clip
  (INV-0162-1, with a fuzz test over mutated fonts, hand-built hostile fonts and random strings). Sizes are 1 to 512
  pixels per em: a larger request draws at 512, which bounds every glyph's raster. Inter keeps its kerning in GPOS, which the reader does not read,
  and JetBrains Mono has none: `kern` 0 kerns nothing in the system's fonts today (ADR-0162's open question).
- **Fonts** (`fonts/`, `zoraya-fonts`): Inter 4.1 Regular and Medium, JetBrains Mono 2.304 Regular and Bold, unmodified,
  with their licences, in a fonts table of `docs/dependencies.md` checked by hash (INV-0162-5). Until the launcher of
  7d hands out `/system/fonts`, the programs that draw text embed them (ADR-0240): `term` grows by about 0.96 MB.
  Since 9f-text the disk of `make run`/`run-gui` (`tools/run-disk.sh`) and NetSurf's test root carry the same files in
  `/system/fonts`, which a POSIX program with a `window` line reads (ADR-0326, "NetSurf's text" below).
- **Reading screens back.** `zoraya_term::read` draws every character a cell may hold (printable ASCII, Latin-1,
  Latin Extended-A, Cyrillic with Ґ ґ) with the terminal's fonts, size and colours, and a cell reads as the character
  whose pixels are identical. Twenty Cyrillic and Latin letters are drawn alike by any font (А and A, і and i, ...):
  they read as the Latin, and the GUI runner compares a script's text in the same classes (INV-0162-4 lists them).

Measured, `plan.md`'s figures for 7a: drawing a full 80 × 24 grid of Cyrillic in the terminal's look (`t_text`, the
test kernel, one CPU), with an empty glyph cache and then from it.

| | Host (M4, release) | HVF (debug) | TCG (debug) |
|---|---|---|---|
| Grid, empty cache | 0.52 ms | 0.47 to 0.57 ms | 4.5 to 4.6 ms |
| Grid, from the cache | 0.30 ms | 0.36 to 0.44 ms | 3.7 to 3.8 ms |
| Cache after two grids | 64 misses, 3,776 hits | 64 misses, 3,776 hits | 64 misses, 3,776 hits |

The ranges are the three HVF and four TCG configurations of one `make test` (2026-11-08). Rasterising costs little
next to drawing: the 64 misses add about a fifth to the first grid. The screens of `make test-gui` under HVF and TCG
are byte for byte the same (`build/shots/`).

## Windows and frames (as built)

Stage 7b (TASK-24), on the accepted [ADR-0160](adr/0160-window-management.md) and
[ADR-0164](adr/0164-display-scaling.md).

- **Windows.** A client may have up to eight toplevel windows (`CreateToplevel`, sized in logical pixels, its buffer
  the logical size times the scale) and four popups (`CreatePopup`, relative to a toplevel's body, above it, closed by
  a press anywhere but on a popup); stage 5's `Create` is still a framed surface of its own. The compositor places
  every window (the first frame at (72, 42) below the bar, then cascaded by 32), stacks it, and sends a toplevel
  `Configure` (its logical size, the scale, light or dark, motion) at creation and after a resize; a new buffer
  (`SetBuffer`, only at the configured size) is shown from its first commit, and until then the old pixels stay, on
  the paper where the body is larger (INV-0160-5). Nothing a client sends moves, raises, focuses or decorates a window
  (INV-0160-2).
- **Frames** (`zoraya-paint`, integer arithmetic only): a title bar of `frame.height` in `bg.frame` or
  `bg.frame.focused`, the client's title (`SetTitle`, UTF-8, at most 256 bytes, control characters dropped) in Inter
  Medium, clipped to the bar, the close cross and the minimise dash at 18 and 42 logical pixels from the left
  (coloured on the focused window, `fg.text.disabled` elsewhere), a one-pixel `border`, smooth corners of
  `radius.window` (a cubic superellipse spanning 1.6 radii), and a two-layer shadow whose alpha tells the focused
  window. The frame is drawn from the tokens alone (INV-0160-1). Room tints, badges and grant pills come with 7d.
- **The person's gestures.** A press on the title bar moves the window (its title bar stays below the bar and on
  the screen); a press within `frame.grip` of a toplevel's edge resizes it, and the client hears of it only at the
  release (`Configure`); the dash minimises (`Minimised`); the cross sends `CloseRequested`, and five seconds later a
  second press ends a client that did not close. The system key (Super; the Command keys under QEMU on a Mac) is the
  compositor's with every chord: System+W asks to close, System+M minimises, System+Left and System+Right tile to a
  half; System+Space, Enter, R and ? are reserved for 7c and 7d and reach no client either (INV-0160-3). Alt+Tab
  opens the switcher (Tab and Shift+Tab move, Alt's release chooses and shows a hidden window, Esc leaves it); F12
  goes at once to the next window in the same order (the focused one, then the stack from the top, hidden ones last).
- **The bar and the wallpaper.** The wallpaper is ADR-0161's `deep` one, computed once per display size in 16.16
  fixed point, with a blurred copy for the glass. The top bar (`bar.height`) is that glass tinted with `bg.overlay`
  at `panel.alpha`, with its band of light and a bottom border, "Zoraya" at the left and the layout at the right; it
  is drawn above every window, and no client reaches it (INV-0160-7). There is no clock: the wall clock of ADR-0226
  is not built yet.
- **The switcher** is drawn in the trusted material (a blur of the scene under it, the tint, the band of light, the
  rim, the halo), a group line per room (only `home` until 7d) and a row per window with its own pixels shrunk by a
  box filter, its title, and "hidden" for a minimised one.
- **Scale** (ADR-0164). The compositor takes scale 2 for a display at least 1920 pixels wide, or at least 1280 on a
  display of 150 pixels per inch or more (ADR-0247: QEMU's cocoa window gives its size in pixels and its
  physical size in the EDID, which `gpud` reads with `VIRTIO_GPU_F_EDID` and passes in `InfoReply`) (the setting
  `auto`; `gui.scale` 1 or 2 fixes it); `DisplayScale` gives the logical size and the scale, every size a client hears is logical,
  pointer events to a toplevel are logical, and a client's pixels reach the screen unchanged (INV-0164-1).
  `tools/run-qemu.sh --gui-scale 2` asks QEMU for 2048 × 1536, and `--gui-size WxH` for any size (7g). Since 7g the
  display follows QEMU's window (ADR-0246, "The display follows the window" below).
- **`term`** is a toplevel named `zosh`: a `Configure` of another size gives its grid and buffer a new size (and the
  shell a `Size`), and the close button ends it; `init` takes a terminal that ended with 0 as a closed window and
  starts a new terminal and shell, as after `exit`.

Measured (`t_gui` in the test kernel, `make test-qemu-auto-default` and `make test-qemu-tcg-default` at 9659e2d,
2026-11-09, QEMU 11.1.2 on the M4 Mac mini):

| | HVF | TCG |
|---|---|---|
| Full redraw of a 1024 × 768 legacy surface with its frame and shadow: compose per frame | 6.1 ms | 55 ms |
| The frame that opens the switcher (blur of the scene under it) | 18.8 ms | 107 ms |
| A frame committed under the open switcher | 18.7 ms | 104 ms |

A commit under the open switcher blurs the scene again (its glass is a blur of what is under it), so it costs what
opening does; a pointer that only moves over it reuses the last frame. Under TCG a full redraw is 55 ms: every GUI
script passes under TCG, typing at 8 ms a key with the frames that lag behind it.

The compositor's start and its frames (`compd`'s own report on the console, `make test-gui`'s terminal script,
2026-11-09, QEMU 11.1.2 on the M4 Mac mini; GitHub's runners are about 2.3 times slower under TCG):

| | HVF, 1024 × 768 | TCG, 1024 × 768 | HVF, 2048 × 1536 | TCG, 2048 × 1536 |
|---|---|---|---|---|
| The wallpaper and its blur, at a display's attach | 21 ms | 71 ms | 84 ms | 282 ms |
| The first frame | under 1 ms | 8 ms | 2 ms | 32 ms |
| The next 100 frames: mean | 1.2 ms | 7.6 ms | 3.8 ms | 24 ms |
| The next 100 frames: longest | 17 ms | 121 ms | 51 ms | 467 ms |

The wallpaper and its blur are made once per display size and appearance, at a quarter of the size each way and
scaled up bilinearly (both are smooth): at full size they took 202 ms, 589 ms, 1.08 s and 9.3 s, and at scale 2
under TCG on CI's runner more than a client's first wait, so `term` panicked (main, 2026-11-09). `term` now waits as
long as the compositor lives. A window's frame and shadow are made once per size and kept (`chrome`); a frame
composes only the damaged region. The longest frames are the full redraws (a window shown, the switcher).

## Keyboard layouts (as built)

Stage 7c (TASK-24), on the accepted [ADR-0163](adr/0163-keyboard-layouts.md).

- **Tables** (`zoraya-keymap`'s `layouts.rs`): `us` basic, `ua` unicode, `ru` winkeys and `cz` QWERTZ basic, four
  levels a key (plain, Shift, AltGr, Shift+AltGr) of a character, a dead key or nothing, from the reference layouts
  the survey records (`docs/research/desktop/survey.md#keyboard-layouts`). Third and fourth levels are only Czech's
  and the Ukrainian ґ: the other layouts' are left out until something needs them.
- **Dead keys**: thirteen accents with their compositions over Latin-1 and Latin Extended-A; a dead key then Space
  gives the spacing accent, then a letter it does not compose passes the letter, and a second dead key replaces the
  first; any other key (Enter, an arrow, a Ctrl chord) cancels it, a key the compositor takes (a chord) leaves it as
  it was, and the state is cleared at a focus change and a switch. Caps Lock acts on case pairs only (it does not
  capitalise Czech's number-row letters, as XKB's semi-alphabetic type would); the right Alt is
  AltGr in a layout with a third level (held across a switch, it keeps its role until released); Ctrl gives control
  characters by the key's Latin character in the active layout, the US one in a Cyrillic layout.
- **Switching**: System+Space cycles the enabled layouts (`us ua cz` until 7f's `gui.keyboard-layouts`), Shift goes
  back; held modifiers stay held, and the switch reaches no client as a key (INV-0163-4). The bar shows the active
  layout's short name; a client hears `Layout { name }` when it gains focus and at each switch.
- **The title**: `zosh` sends `Title` (its directory as the prompt shows it) at start and after each `cd`, out of band
  (ADR-0168); `term` titles its window `zosh — <dir>`.
- **The GUI test** `tests/gui/layouts.script` switches to `ua`, types a Ukrainian name, switches to `cz`, types
  `příklad.txt` and letters only dead keys reach, and reads them back; the runner types each letter with the keys the
  layout's table gives (`layout NAME`), a letter with no key through a dead key.

## Rooms, the dock and the panel (as built)

Stage 7d (TASK-24), on the accepted [ADR-0165](adr/0165-widget-toolkit.md), [ADR-0168](adr/0168-first-applications.md)
and [ADR-0172](adr/0172-rooms.md), with the proposed [ADR-0242](adr/0242-the-panel-beside-init-in-stage-7d.md),
[ADR-0243](adr/0243-a-rooms-tint-on-the-frame-only.md) and [ADR-0244](adr/0244-the-toolkits-own-events.md).

- **`zoraya-ui`**: label, button, text field (editing by character), check box, switch, radio group, list, scroll
  view, separator, menu, dialog, progress bar; rows, columns and grids with `gap.controls` and `padding.window`;
  painting through `zoraya-paint` and `zoraya-text` from the tokens, every widget clipped to its rectangle
  (INV-0165-1); Tab and Shift+Tab over the focusable widgets, arrows in lists and radio groups, Enter, Escape closing a
  menu or dialog (INV-0165-4). Widgets take two events of their own, a key press and a logical pointer (ADR-0244).
- **`zoraya-app`**: an application's loop over a `Platform` (the compositor's channel and memory objects): it opens
  toplevels of widget trees, paints and commits one frame at a time, answers `Configure` with a new buffer, and turns
  keys, the pointer, focus and `CloseRequested` into widget events or into what the application acts on. It sends
  only client requests (INV-0165-2). No program uses it yet: `files`, `edit` and `settings` (7e, 7f) will.
- **The room table**: `/config/rooms`, one room a line, `NAME [--untrusted] [--colour N] +r PATH +w PATH...`, read
  and written by `zoraya-settings`' `rooms` module (a total reader, the writer keeping every other byte); a trusted
  room without `--colour` takes the lowest free slot, more than four repeat the least used.
- **`room` in `zosh`**: `room` lists (the current one marked `*`), `room NAME` enters (the prompt shows the name, the
  current directory becomes the room's first `+w` directory, every line gets the room's grants, which `grants` shows
  as `(room NAME)`), `room --leave`, `room new NAME +w PATH...` (a path under home is written with `~`), `room remove
  NAME`. `room new --untrusted` is refused with a hint naming ADR-0056. The panel starts a shell in a room with
  `zosh --room NAME`.
- **The panel** (`panel`, ADR-0168; `zoraya-panel` holds its logic): `init` starts it once per GUI with the client
  manager (`AttachClientManager`), a clone of the root, the console to write, its job and the image directory. It
  reads `/config/rooms` at start, at every dock press and every second, tells the compositor the table and the current
  room (`SetRooms`) and the dock's applications (`SetDock`), and on `DockPressed` launches a terminal into the current
  room (the dock's icon with no window, or System+Enter) or enters a chip's room. A launch gives `term` the
  compositor's channel and its end of the shell's channel, and `zosh` the console, its job, the image directory, its
  end and a start-up record (ADR-0153 rule 8) of exactly its room's grants, never the volume's root (ADR-0242's
  condition); nothing else (INV-0168-1). `home` gives `+r /`, `+w ~`, `+w /config` and `+w /tmp`: the volume to read,
  and the person's files, configuration and temporary files to change. The shell serves its own files and every
  command's grants from that table, and refuses a path it was not given with ADR-0153's hint (`hint: cp tried to
  change ~/copy.txt, which it was not given`, `to allow it: +w ~ cp notes.txt ~/copy.txt`); a room's shell takes its
  room from the table, since it cannot read `/config/rooms`; a trusted room's shell also holds `+r /bin`, `+r /usr`
  and `+r /etc` (the toolchain, unless a room grant covers it; ADR-0172's decision of 2026-11-09), and every shell
  the panel launches has a broker beside it (slots 7 and 8, as `init` gives its first; `user/src/beside.rs`), which
  ends when the shell's connection closes and nothing it started runs, so POSIX programs run in rooms
  (`tests/posix/room-gui.script`); a grant
  whose path does not exist gives it nothing to hold or list. A tree entry cannot publish (ADR-0153's table), so such
  a shell writes `/config/rooms` and `/config/settings` in place, not by an atomic replace (accepted for now; the
  crash window is in `docs/backlog.md`). The
  console's shell keeps the root. The compositor learns the client with its record (`AddRoomClient`: the room, its
  trust and colour, the application, the entries in ADR-0153's words, the line), and the pills show exactly what the
  shell holds. `init` still starts the first terminal, with `home`'s record, and tells the compositor its entries
  itself (ADR-0242).
- **What the compositor draws of rooms**, from its records only (INV-0172-1): the title bar tinted by the room at 46 of
  255 (the body stays the client's, ADR-0243); at the right of the title bar the room's dot and name with `untrusted`,
  and the record's entries as pills, folded into `+N` when they do not fit; on every window of an untrusted room an
  edge of `room.untrusted.edge` pixels in `room.untrusted` inside the border (INV-0172-10). *Fields*: when two or more
  rooms have windows on screen, each coloured room's windows grown by `field.padding` with round corners, `field.gap`
  inside the work area and cut back from every other coloured room by the gap, at alpha 34 with a one-pixel line, a
  header with the room's name and grants above its top-most window (INV-0172-6 to -9). They are drawn into a copy of
  the wallpaper when a window appears, goes, is hidden, shown, tiled, or ends a move or a resize, never per frame.
- **The dock** (the README's dock B), drawn by the compositor above every window (INV-0168-3) on the blurred
  wallpaper: a tile per application (the panel's list, then any other with a window), a dot under it per room it has
  windows in (`home` grey), a ring in `room.untrusted` around an application with an untrusted window, a divider, a
  chip per room (`home` first, the current one filled with its colour at 46 and outlined), and `+`. A press on an icon
  shows and raises the application's windows, or asks the panel to launch it when it has none; a chip asks the panel
  to enter its room; `+` does nothing yet (the rooms overview is not built). The hovered icon's name and line stand in
  a tooltip of the trusted material. The dock's strip (76 pixels) is reserved: tiling ends above it.
- **Show-commands** (System+?): every icon and chip has its line under it and the dock grows by 30 pixels (the
  fields are recomputed above it), every window has its close and minimise lines (`kill term · bg term`) under its
  title bar, and the bar's middle says how to leave. One table holds every gesture's text, and the gestures without
  one say why (focus, tiling, moves; ADR-0172's open question; INV-0172-5).
- **The GUI test** `tests/gui/rooms.script` starts a second terminal from the panel and reads its shell's `slots`
  (exactly its record's handles), hides both terminals and brings them back from the dock, makes a room with
  `room new`, enters it by its chip, launches a terminal in it (its prompt `project ~/project>`, its title bar's tint
  read back), and shows the commands; under HVF and TCG.

Measured (`t_gui`, `make test-qemu` at bedd670, 2026-11-08): `zoraya-ui` painting a settings-like page of 640 × 480
(headings, a radio group, a switch, a check box, a list, a field, two buttons) takes 0.35 ms with an empty glyph cache
and 0.23 ms after under HVF, 5.0 ms and 3.2 ms under TCG; the settings window itself comes with 7f.

Not built in 7d, each waiting for a later step: the rooms overview (System+R) and the `+` chip's form; the hover card
of a window's pills; the switcher grouped by room (it shows `home` only); one job per room (each launch has its own
job under the panel's); `room NAME COMMAND`, `room add` and `room drop`; the `Room` message from a shell to its
terminal; the panel starting the first terminal and the new shell after `exit` (ADR-0242). Fields follow a move at
its end, not during the drag.

## The chooser, the file manager and drops (as built)

Stage 7e (TASK-24), by [ADR-0167](adr/0167-granting-files-in-the-gui.md), [ADR-0168](adr/0168-first-applications.md)
and [ADR-0170](adr/0170-clipboard-and-drag-and-drop.md), with the proposed
[ADR-0245](adr/0245-the-choosers-wiring-in-stage-7e.md):

- **The chooser** (`chooser`, its logic in `zoraya-chooser`): a compositor client the driver manager marks with
  `AddSystemClient`, holding the volume's root. The panel gives every application it launches outside an untrusted
  room a chooser channel (slot 6) and hands the other end to the chooser with the application's compositor tag
  (`AddApplication`). On `Open`, `Save` or `PickDirectory` the chooser lists the folder through `zoraya-fsclient` and
  asks the compositor for a sheet under that application's window (`CreateSheet`, system clients only); the
  compositor draws the strip with the emblem and "Open a file for APP", the bar's mark, and gives the owner no input
  while the sheet is open. Only a key or a press in the sheet confirms, and none in its first 500 ms nor a key or
  button held from before it mapped (ADR-0245's condition, `SHEET_GUARD_NS`); in show-commands mode, which the
  compositor tells system clients (`Commands`), the line under the sentence is the grant's `zosh` line (for `Save`,
  `+w FOLDER APP · gives less: create or replace NAME`). The answer is one connection with ADR-0153's
  rights for the kind (`Open` reads; `Save` is the folder narrowed to the one name, with create's rights;
  `PickDirectory` reads or is a `tree`), the display name and the kind's words (`+r plán.txt`), never the path; the
  chooser then tells the compositor `Granted`, and the window's frame shows the pill (`+r ~/project/plán.txt`). One
  sheet at a time; other requests wait.
- **The data directory**: the panel makes `/data/ROOM/APP` (`/data/home/APP` in `home`) and gives it at slot 7 with
  every right but publish.
- **The file manager** (`files`, its logic and window in `zoraya-files`; redesigned in 7h, below). It holds the volume to read and, to change, `~` in `home` or its room's
  grants, in a start-up record, never `/config` (INV-0168-2 as changed on 2026-11-10), and opens a file only through
  the panel:
  `OpenWith` with a connection, which the panel clones down to read and clone, then launches the application for the
  name's extension (`.txt` and `.md`: `t_open` in 7e, `edit` from 7f) with it at slot 10.
- **Drops** (ADR-0170): a press on a file in `files` starts a drag of it, read only. The compositor takes `Drag` only
  for the press it delivered to that client, still held and named by its serial within 500 ms; while the entry is
  carried it draws its name beside the pointer and tells no client of the pointer or the keys; at the release the
  window under the pointer gets `Drop` with the connection and the display name, and its frame the pill `dropped: NAME
  read`; the source gets only `DragEnded`. Escape, a release over the source or over no window, or a window of an
  untrusted room (until the confirming card exists, INV-0172-3) delivers nothing and closes the entry.
- **The test application** `t_open`: Open and Save through the chooser, and drops; it prints on the console what it
  read and whether a write was allowed.
- **The GUI test** `tests/gui/files.script` (HVF and TCG): `files` opens `notes.txt` in `t_open` (read; the write
  refused), `t_open` opens it through the chooser (the same) and saves `nový.txt` (written; a second name beside it
  refused), `t_open` is moved aside and the file is dropped on it (read; the write refused), and the first terminal
  lists both files.

Measured (the chooser's round trip, plan 7e; `tests/gui/files.script`, 2026-11-09, QEMU 11.1.2 on the M4 Mac mini):
from the request read to the sheet asked for, its folder listed and its first frame committed, 0.90 ms and 0.94 ms
under HVF (Open, Save), 22.5 ms and 17.5 ms under TCG; from the confirming key to the connection made and the answer
and the pill sent (the sheet's window closed first), 9.0 ms and 10.5 ms under HVF, 69.9 ms and 74.7 ms under TCG. The
person's own time between the two is not counted.

Not built in 7e: the rooms' own launches of `files` with a narrower root; the application opened from the file
manager starts in `home`; the chooser's centred-window style; `multiple` in `Open`; a sheet's re-placement when the
scale changes; the confirming card for drops on an untrusted window; the clipboard (7f).

## Settings, the editor and the clipboard (as built)

Stage 7f (TASK-24), by [ADR-0169](adr/0169-settings.md), [ADR-0170](adr/0170-clipboard-and-drag-and-drop.md) and
[ADR-0171](adr/0171-list-settings.md):

- **The desktop's settings** are declared in the compositor's description (`user/describe/system/compd.cmd`, which
  `compd` embeds and `zosh` knows): `gui.theme`, `gui.motion`, `gui.scale` (`auto`, `1`, `2`), `gui.wallpaper`,
  `gui.keyboard-layouts` (ADR-0171's list: of `us ua ru cz`, 1 to 4, unique; default `us`) and
  `gui.keyboard-layout`. `init` gives the compositor `/config` to read at slot 11; it reads the file once at start
  (each bad line reported, its default kept; a fixed scale applies to the display). `settings` in `zosh` shows a
  list's items with their indices and takes `settings NAME ITEM...`, `+ITEM [--at N]` and `-ITEM`.
- **The settings window** (`settings`, its logic in `zoraya-settings-window`): appearance and keyboard on one page,
  each control bound to its declaration. Every change is written at once, atomically (a new file published over the
  old one, through `/config` narrowed to `settings`, which no other launch gets to write, INV-0169-4), then
  `SettingsChanged` goes to the panel, which reads the file and sends the compositor `ApplySettings`; the compositor
  checks every value again and applies them all or none. The theme reaches the frames, the wallpaper's blur and every
  window's `Configure`, and `zoraya-app` repaints its windows in it; the layouts apply at once; the scale too since 7g.
- **The editor** (`edit`, its buffer and view in `zoraya-editor`): one file, opened from the file manager (read only)
  or through the chooser, saved through the chooser's `Save` and then again to that name; cursor and selection by
  characters, undo and redo (1000 edits), find, and a question before a close with unsaved changes.
- **The clipboard** (ADR-0170): System+C and System+X send the focused window `Copy`; its `Offer` within 500 ms of
  that is kept, any other refused; System+V sends the focused window `Paste`. No message reads the clipboard back.
  `edit` copies, cuts and pastes; drops are 7e's.
- **The GUI test** `tests/gui/settings.script` (HVF and TCG): the theme turns dark in the settings window and the
  frames follow; Czech is enabled and put in use and the terminal types by it at once; `edit` takes Czech text on
  that layout and saves `poznámky.txt` through the chooser; the file manager lists it.


Measured (`settings`' own report, `make test-gui GUI_RUNS="settings:auto:1 settings:tcg:1"` at f758a1d, 2026-11-09,
QEMU 11.1.2 on the M4 Mac mini): a change in the settings window, from reading the file to the new file published,
then from `SettingsChanged` to the panel's answer (the compositor has applied it):

| | HVF: written | HVF: applied | TCG: written | TCG: applied |
|---|---|---|---|---|
| The theme turned dark (every window restyled, the wallpaper's blur made again) | 0.27 ms | 20 ms | 35 ms | 34 ms |
| Czech enabled | 0.15 ms | 0.30 ms | 19 ms | 4.7 ms |
| Czech put in use | 0.18 ms | 0.35 ms | 3.8 ms | 4.3 ms |

Not built in 7f: the terminal keeps the light set (its screen is read in the GUI tests by the light set's glyphs);
the wallpaper setting accepts only `''` (no image decoder); copy and paste in the toolkit's text fields (only `edit`
answers them); the clipboard's entries (only text); a `settings` command reaching the running compositor (the next
session, as ADR-0169's open question says).

## The display follows the window (as built)

Stage 7g (TASK-24), by proposed [ADR-0246](adr/0246-the-display-follows-the-window.md):

- **`gpud`** looks for virtio-gpu's display event every 100 ms between requests (`events_read`, one register read),
  asks `GET_DISPLAY_INFO` and tells the compositor a new size with `Resized`. When it may tell is
  `zoraya-virtio-gpu`'s `Announce`: once something is attached, for a size other than the attached one, one before
  each `Info`, and none between an `Info` and the request after it, since the compositor takes the next message on
  the channel as its reply (INV-0246-2). A second `Attach` at a new size makes the other resource, shows it on
  scanout 0, then unreferences the old one and unpins its framebuffer.
- **The compositor**, once any flush in flight is answered, asks `Info` and attaches a framebuffer of the new size
  (none when the size came back to its own); `compd` releases the one before once the new is shown. The scale comes
  from `gui.scale` or, under `auto`, from the width (2 from 1920 pixels); popups close, and sheets too at a new
  scale; every toplevel keeps its logical size as far as the work area holds it and is moved back into it, legacy
  windows are moved back, and every toplevel hears `Configure`; the bar and the dock follow the width.
- **`gui.scale`** applies at once from the settings window too.
- **Tests**: host tests of `Announce` and of the second resource (`zoraya-virtio-gpu`), and of the compositor with
  a fake driver that changes size; `make test-gui` boots the terminal's script at 1600 × 1000 and 1920 × 1200 too
  (`tools/run-qemu.sh --gui-size`). QEMU's QMP has no command that resizes a console, so no test in QEMU resizes a
  running guest.

Measured (`compd`'s own report, `make test-gui` with the terminal's script at 75ab053, 2026-11-10, QEMU 11.1.2 on
the M4 Mac mini): each new size costs the wallpaper and its blur once, then a full redraw.

| | HVF, 1600 × 1000 | HVF, 1920 × 1200 (scale 2) | TCG, 1600 × 1000 | TCG, 1920 × 1200 (scale 2) |
|---|---|---|---|---|
| The wallpaper and its blur | 36 ms | 51 ms | 138 ms | 199 ms |
| The first frame | 1 ms | 2 ms | 16 ms | 26 ms |
| The next 100 frames: mean | 1.9 ms | 6.3 ms | 8.9 ms | 34 ms |
| The next 100 frames: longest | 17 ms | 50 ms | 114 ms | 463 ms |

Not built in 7g: a window a client creates larger than the work area is not made smaller (at 1920 × 1200 the
terminal's 80 × 24 runs under the dock); popups do not learn a new scale (they close instead); the compositor's
`call` still takes the next message as its reply, so the handshake's order rests on `gpud` (review 7g, A).

## The file manager (as built)

Stage 7h (TASK-24, the architect's request and the approved mockups
`docs/research/desktop/mockups/clear-field/files*.png`), with [ADR-0248](adr/0248-what-the-file-manager-needs.md):

- **`zoraya-files`**: `model` holds the folder shown and its items (name, size, modified), the view sorted by a column
  (folders first; a header click sorts, again reverses) and filtered by the search, the selection (click, Shift for a
  range from the anchor, Command or Ctrl to add or take away), the cursor, back and forward, rename in place (F2 or
  Enter on the menu; the stem selected; Enter keeps, Escape leaves), delete asked first in a card, a new folder ("New
  folder", "New folder 2", ...), the context menu with each item's `zosh` line (`edit`, `mv`, `rm`, `mkdir`, `room
  NAME term · cd`), drops on its own folders, and the status line; every request is an `Act`. `view` is one
  `zoraya-ui` widget drawing the toolbar (back, forward, up, the path as crumbs, search), the sidebar of places with
  `+w` or `+r`, the list with its header, the status line with free space, the menu and the card, in the mockup's
  measures at both scales. A place's room colour is not drawn by the application (ADR-0172: colour means a room, and
  the compositor alone draws it): a neutral ring marks the room's own grants.
- **`files`** carries out each act: lists through its places (the longest that covers a path; a place's write right
  found by asking the service for a connection that writes), opens a file through the panel (`OpenWith`, with
  `app: edit` for Open in edit), asks the panel for a terminal at a folder (`OpenTerminal`), offers a path to the
  clipboard (an unasked `Offer` within a second of the menu's click: a press in its body, never in an untrusted
  room, the right gone with a focus change or a System+C), moves within a place (`rename_into` with the
  target's token) and copies otherwise or with Option (a folder whole, at most 4096 entries and 32 deep), deletes a
  folder whole, starts drags, and on `DragEndedAt` over one of its own folders copies or moves the selection there. It
  prints its holds, every status line and every order on the console for the GUI tests.
- **ADR-0248 (accepted)**: System chords the compositor does not take (`TAKEN_CHORDS`) reach the focused client with
  `SYSTEM`; `WindowPointerMods` (ordinal 30, beside `WindowPointer`) carries the modifiers (to the focused client only); `Commands` goes to every client; one
  unasked `Offer` within `GESTURE_OFFER_NS` of a press in a trusted client's focused window; `DragEndedAt` (ordinal 31, beside `DragEnded`) carries the place of a drop on the
  source's own window; `OpenWithApp` (panel channel, ordinal 35) names the application, the older `OpenWith` keeps its layout (ADR-0052); `FreeSpace` in the file protocol; `OpenTerminal` launches a terminal in the caller's room at a
  path that room's holds cover (`terminal_at`).
- **Tests**: `zoraya-files`' host tests (the model, the view's hit-testing, gestures through the widget, painting at
  both scales); `tests/gui/files-manage.script` (99 steps, HVF and TCG): selection, sorting, the keyboard, search,
  rename, a new folder, delete with its card, a drag that moves and one that copies, a double click, the menu's Copy
  path and Open terminal here, show-commands mode, up, back and the places; `tests/gui/files.script` as in 7e, with
  the file manager tiled so that the drop lands on `t_open`. The runner gained `dclick`, `rclick`, `mclick`, `mdrag`
  and chords of several modifiers.

Measured (`tests/gui/files-manage.script`, 2026-11-10, QEMU 11.1.2 on the M4 Mac mini, HVF): listing a folder of 2 to
5 items with each one's size and time, 0.17 to 0.50 ms; right after a change, 6 to 28 ms (the file service commits
first).

Not built in 7h, or known: a trash (`docs/backlog.md`); a copy or delete that fails part way (a link inside a
folder, no space, past 4096 entries) leaves what was done and says only "Cannot copy NAME"; a forwarded System chord's
release carries no `SYSTEM`; `OpenTerminal` is not rate-limited (one request at a time per launcher channel, as
`OpenWith`); moving or copying several files by a drag to another window (a drop
carries one file, ADR-0170); a scroll wheel (the list follows the keyboard's cursor); the window's title stays
"Files".

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
  archive exports exactly the 243 names of `docs/adr/posix-surface-T1.txt` (ADR-0141) and the names ADR-0142,
  ADR-0144 and ADR-0210 add in their files' `posix-surface-added` blocks (5 and 48: ADR-0210's are what the
  workloads' own programs and pdpmake link against). Each exported function is reached through a call-log wrapper
  (`zw-NAME.o`, ADR-0211), and `usr/lib/zoraya-calls.txt` gives the bitmap's order; `POSIX_STATS=0` builds without
  it. `<zoraya/posix.h>` (`ports/include/`) declares Zoraya's
  additions (ADR-0142). The figures above are stage 6's: the export list has since taken the socket names of ADR-0287, the
  thread surface of tier T9 (ADR-0308, `docs/adr/posix-surface-T9.txt`; musl's `src/thread` is now compiled over the shim,
  "Threads in a process") and ADR-0310's 22 names (9a). The sysroot also has
  `crt1.o` (`ports/crt/crt1.S` and musl's `_start_c`), musl's `crti.o` and `crtn.o`, musl's empty libraries, and
  tcc's `libtcc1.a` and headers in `usr/lib/tcc`.
- **The ports.** All build with `zcc`, and none with `__linux__`: dash (TASK-02's M4 patch and ADR-0142's two
  job-control patches: the fork group hint, and `vforkexec` through `posix_spawn` with `SETPGROUP` and
  `POSIX_SPAWN_TCSETPGRP`), GNU make (one line of `config.sub`), pdpmake (unpatched, stage 6e), busybox (`tr`, `diff`, unpatched), toybox (`cat head
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
- `make check-tls-roots`: the roots the image ships (`rootfs/system/tls/roots`, Mozilla's store, 108 certificates from the CCADB report of
  2026-11-12, ADR-0228) on the source: at least 100 blocks, each a DER certificate, none repeated, none of the test PKI's, the
  header's count, source and date, and `docs/dependencies.md` agreeing; `cargo test -p zoraya-tls --test production_roots` loads every
  one into the verifier;
- `make check-fetch-ports`: the ports' fetch step against a local file, offline (INV-0109-3, INV-0109-4);
- the in-QEMU tests in seven machine configurations, with `build/test-disk.img` attached and `zoraya-fsck` after them;
- `make test-libposix`, the stage 6c criterion (below, under "How the in-QEMU tests run");
- `make test-posix`, stage 6e's acceptance in `make test` ("Stage 6 acceptance" above); on CI a job of its own. Two
  of its scripts run on images without the session file, so that `zosh` starts with the broker beside it:
  `tests/posix/zosh.script` (POSIX programs from `zosh`, ADR-0153 rule 10, ADR-0200) and `tests/posix/tour.script`,
  the whole of `docs/tour.md`, whose GUI twin `make test-tour-gui` plays (INV-0154-4);
- `make test-net` (N1, N2): the test kernel with `--net` and a pcap, whose `t_net` runs `ethd` against the real device and
  `t_netd` runs `netd` over it, then the normal kernel with `--net` and `tests/shell/net.script` (under TCG too with
  `FULL=1`); it is skipped, and listed in the summary, on a QEMU without user-mode networking locally, and
  fails there on CI.
- `make test-gui`, the stage 5 criterion: the normal kernel with `--gui`, headless, under HVF (on a Mac) and TCG.
  The runner (`tools/test-runner/src/gui.rs`) types over QMP, one key every 8 ms (QEMU's virtio-input queue holds 16
  keys and drops what a busy guest has not taken), and reads QEMU screendumps back as text with `zoraya_term::read`:
  every cell of the terminal's grid is matched exactly against every character drawn by the same crates, and PPM is
  lossless, so the tolerance is zero. It also checks the background and the focused frame pixel by pixel, and `shot
  NAME` keeps a screen in `build/shots/`. `tests/gui/terminal.script` waits for the prompt, writes and reads a file, runs a
  pipeline, presses F12, types a line wider than the window in one burst and checks that it is redrawn in place,
  reads Ukrainian and Czech names and text back (from `rootfs/texts/`, completed with Tab until 7c's layouts), measures
  key latency, and powers off. The serial line's input is
  closed, so keys reach `zosh` only through the window;
- `make test-boot`, which builds the normal kernel and boots it under every machine configuration. The test runner
  plays `tests/shell/console.script` on the serial line and checks the output, through to `poweroff` and QEMU's status
  0; `tests/shell/leak.script` runs more programs than a handle table has slots, `tests/shell/exit.script` ends the
  shell, `tests/shell/jobs.script` checks job control (INV-0158-1 to -3, with the `requests` test program) and
  `tests/shell/reboot.script` resets the machine (`run-qemu.sh --reboot`, INV-0155-7).

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
  `zoraya-fsck` after each; `tests/shell/conventions.script` runs every builtin and native command with `--help` and
  an unknown option (INV-0155-1), and `tests/shell/grants.script` checks what a command gets, through link objects
  too (ADR-0153, with the `probe` test program). Then a disk that cannot be mounted (`init` stops after three restarts) and a write failed in
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
  ramdisk directory, `Power` at slot 4, the real-time clock's `Device` at slot 5 and the entropy seed at slot 6 (each empty when the device tree has none; N5), and `Device` handles for the virtio pages from slot 7. It starts `rngd` when there is a seed, and `zosh` in a sub-job with only what
  the shell needs, or, when the volume has `/etc/zoraya/session`, `ttyd` and the broker (stage 6d), and turns the
  machine off when the shell or the session ends.
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
- **`power_reset`** (call 50, ADR-0159, TASK-18 step 12). Needs `POWER` on the `Power` object, as `power_off`; resets
  through PSCI `SYSTEM_RESET` (`zoraya-dtb`'s `has_system_reset`, PSCI 0.2 and later), printing `resetting`, and
  otherwise returns `NotSupported`, a new status (15).

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
- **The bootstrap record (ADR-0107).** `zoraya_abi::posix`: `ZPB1` with the process's facts, `umask`, the ignored
  set and the signal mask on entry (ADR-0190's Decision), the time base (ADR-0191), cwd as an entry
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

## Processes and the terminal (as built)

Stage 6d (TASK-21), on the accepted [ADR-0102](adr/0102-snapshot-process.md) to
[ADR-0108](adr/0108-programs-from-the-filesystem.md) and [ADR-0142](adr/0142-surface-for-dash-job-control.md),
with the choices they leave open in [ADR-0190](adr/0190-process-choices-of-stage-6d.md),
[ADR-0191](adr/0191-one-time-base.md) (the time base) and [ADR-0192](adr/0192-snapshot-in-the-posix-tree.md)
(`snapshot_process` into any `posix` job its handle allows), all accepted.

- **The session.** When the root volume has `/etc/zoraya/session`, `init` starts `ttyd` and the broker instead of
  `zosh` (ADR-0104 item 12): it makes the terminal's output stream and two channels, gives `ttyd` the read end and
  the server ends with the console (or, with a GPU, the GUI terminal's channel), and the broker the write end, the
  client ends, a root connection, its own `posix` job and the boot's time base. The broker starts `tmpd`
  (`fsd --memory 16`), reads the session file (`command` and `env` lines, ADR-0190 item 8), and spawns the command
  as pid 2 in a new group, in the foreground. The session ends when pid 2 is reaped and no other pid is live; the
  broker prints `BROKER-STATS` and `init` turns the machine off.
- **The broker (`crates/zoraya-broker`, `user/src/bin/broker.rs`).** The pid table, groups as jobs in the session
  job, and `zoraya-proto-posix`'s requests, over a `Kernel` trait the host tests fake. A pid is never given twice;
  `setpgid` never moves a process; `kill` is one `job_signal` on a group job, or each member by its stop record for
  the stop signals. Orphans are the broker's. The mode with no first shell and `Group(job)` are in the logic and the
  program (`--no-shell`), which `init` starts beside `zosh` on a volume with `/bin` (TASK-18 step 10).
- **The terminal (`crates/zoraya-tty`, `user/src/bin/ttyd.rs`).** The line discipline (canonical, echo, ERASE, KILL,
  EOF, `ICRNL`, `ONLCR`, INTR, QUIT, SUSP), connections tagged with a group, the foreground job, one held read per
  connection. Ctrl-C is `job_signal(foreground, Interrupt)`; Ctrl-Z goes to the broker as `suspend`. Backends: the
  serial console, and the stage 5 terminal window, to which `ttyd` speaks `zoraya-proto-term` as `zosh` does.
- **The temporary directory service.** `fsd --memory MIB` over a `MemoryDevice`, with FIFO nodes: the service keeps
  which objects are FIFOs (ADR-0190 item 4), and `fifo_open` (file ordinal 34) gives a stream end for a side, at
  once, `ENXIO`, or held (INV-0006-9). Its clock is the boot's time base (`--base`).
- **`libposix`'s process half.** `jobs.rs` (wait points, the broker's client, `waitpid`, the terminal read,
  `inherit` and `fork_moves`), `signals.rs` (actions, mask, pending set), `wait.rs` (statuses), and `ffi/procs.rs`
  (the C boundary: handlers between calls, `posix_spawn`, `execve` with `#!`, `fork` through `snapshot_process`,
  `pselect`, `sigsuspend`, `nanosleep`, `kill`, the terminal's calls). fd 0's handle is at slot `FD0` (ADR-0190 item
  1). `ZORAYA_POSIX_STATS` adds `POSIX-PROCESS` (the broker's round trips, forks); `ZORAYA_POSIX_DEBUG` says why a
  fork, spawn or exec failed.
- **Tests.** Stage 6e's acceptance, below ("Stage 6 acceptance: the toolchain inside Zoraya"), and
  `make test-posix`'s `invariants.script` (`t_posix`'s checks) and `scenarios.script` (SIGPIPE, `make` with two rules,
  Ctrl-Z, the hostile cases); `make test-posix-gui` runs `gui.script` in the window.

Measured on the M4 under QEMU 11.1.2, `make test-posix`'s `measure.script` (the normal kernel, 128 MiB), one run per
accelerator: `ZORAYA_POSIX_STATS=1 t_posix bench` (20 each of `posix_spawn` and wait of a program that exits at
once, `fork` and wait, `fork`, `exec` and wait) and `ZORAYA_POSIX_STATS=1 sh -c` with twenty subshells for dash's own
fork; the broker's round trips are the library's count around each call (`POSIX-PROCESS`), means.

| | HVF | TCG |
|---|---|---|
| `spawn_prepare` round trip (dash; `t_posix`) | 13 µs; 26 µs | 0.41 ms; 0.66 ms |
| `spawn_commit` round trip (the new child runs first) | 0.47 to 0.89 ms | 2.0 ms |
| `reap` round trip | 20 to 25 µs | 0.66 to 0.80 ms |
| A fork of dash, in the parent; its `snapshot_process` | 1.32 ms; 0.36 ms | 12.9 ms; 8.5 ms |
| `posix_spawn` and wait of a program exiting at once | 26 ms | 183 ms |
| An `exec` (fork, exec and wait, less fork and wait) | 25 ms | 195 ms |

An `exec` or a spawn is dominated by loading the image: the 1.4 MiB program read through the file protocol into a
memory object, `image_create`, and `spawn`'s copy of its segments; nothing is cached between spawns of one program.
A commit's time includes the child's first slice on the one CPU before the broker answers.

## Stage 6 acceptance: the toolchain inside Zoraya (as built)

Stage 6e (TASK-22), on the accepted [ADR-0110](adr/0110-acceptance-harness.md) and
[ADR-0005](adr/0005-posix-scope.md), with [ADR-0210](adr/0210-surface-for-the-workloads-products.md) (the names the
workloads' own programs link against), [ADR-0211](adr/0211-acceptance-choices-of-stage-6e.md) (the harness's
choices) and [ADR-0212](adr/0212-loading-programs-and-cis-budget.md) (what fits CI), all accepted. Inside Zoraya, tcc
builds itself, Lua and SQLite, with GNU make, pdpmake and dash, and the measured shell session, W8's recursive build,
its interruption and W10's background jobs run as TASK-02 and TASK-02b ran them on Linux, with the same commands and
checks.

**What a POSIX session has.** The image `make posix-image` builds (`build/posix-disk.img`, from `build/posix/root`):
the ports in `/bin` (dash as `sh`, GNU make, pdpmake, tcc as `cc`, toybox's `cat head ls mv rm sleep touch true
uname sort wc yes`, busybox's `tr diff`), the sysroot in `/usr` (tcc's headers and `libc.a`), `/etc/passwd` and
`/etc/group` with the session's one identity, the workloads' sources as `ports/sources.lock` pins them in `/src`
(`tcc`, `lua`, `sqlite`, `hello`, `w8`, `w10`, `w3`), a fresh tree per build in `/work` (copied by
`tests/posix/trees.sh` on the host: T1 has no `cp`), the scripts in `/tests`, and `/sets/t1p`, where `make` is
pdpmake. The session runs dash with `PS1='PSURF> '`, `ENV=/dev/null`, `PATH=/bin:/usr/bin`, `HOME=/home`,
`TERM=dumb`, as TASK-02's `env -i` did. The guest has 512 MiB: W8 at `-j4` ran out of 128 MiB (ADR-0211 item 9).

**Running a workload by hand.** `make posix-image`, then boot it with the serial console in the terminal (Ctrl-A X
leaves QEMU):

```sh
make posix-image
target/debug/zoraya-mkfs --size 256M --from build/posix/root build/posix-disk.img
tools/run-qemu.sh --mem 512M --disk build/posix-disk.img build/zoraya-kernel.elf
```

and at the `PSURF>` prompt:

| Workload | Command | It passes when it prints |
|---|---|---|
| W1, tcc builds itself | `sh /tests/w1.sh` | `W1-OK`, `W1-STATUS 0` |
| W2, Lua with pdpmake and with GNU make | `sh /tests/w2.sh serial` | `W2-OK w2-p`, `W2-OK w2-g` |
| W2-j, Lua at `-j2`, `-j4`, both jobserver styles | `sh /tests/w2.sh parallel` | four `W2-OK` |
| W3, SQLite | `sh /tests/w3.sh serial` | `W3-OK w3` |
| W3-j | `sh /tests/w3.sh parallel` | four `W3-OK` |
| W8, the recursive build, five ways | `sh /tests/w8.sh` | five `W8-OK`, `W8-RECIPES ... 72` |
| W10, background jobs and pipelines | `sh /tests/w10.sh` | `W10-OK` |
| W4, GNU hello's `configure` (not acceptance) | `sh /tests/w4.sh` | how far it gets |
| The roadmap's criterion | `cd /home && printf 'int main(){puts("hello");}' > h.c && tcc h.c && ./a.out` | `hello` |

Each tree in `/work` is for one build; boot a fresh image to run one again. W5 and W9 are interactive:
`tests/posix/w5.script`, `w9-int.script` and `w9-term.script` list their keys. `exit` ends the session; the broker
then prints the session's `POSIX-STATS` and `POSIX-CALLS` lines and the machine powers off.

**The targets.**
- `make test-posix` (a step of `make test`; on CI one job a script, ADR-0213): the export check of `libc.a`
  (INV-0005-1, INV-0005-2), then `tcc hello.c && ./a.out`, W5, W8, W9 (Ctrl-C and SIGTERM), W10, `invariants.script`
  and `scenarios.script`, each a fresh boot under HVF (on a Mac) and TCG, `zoraya-fsck` after each; W8 under the
  first accelerator only.
- `make test-posix-long` (not in `make test`; stage 6 is not merged without it): W1, W2, W2-j, W3, W3-j under both,
  W8 under the second, then `make posix-calls`.
- `make posix-calls`: `tools/posix-stats --calls` over the logs of the nine T1 runs (`target/test-logs/posix-*.log`)
  against the tier list: it fails on a function TASK-02 saw called that no run calls (INV-0005-3, INV-0110-2), except
  ADR-0211 item 16's three, named on every run.
- `make measure-w4`: T2's applets linked against `libc.a` (`ports/Makefile t2-probe`), W4 and the W5 extension
  (Ctrl-Z, `fg`, `bg`), and `build/w4-report.md` (INV-0110-3), which ends with the order to T2 decided by the
  architect.
- `make posix-table`: the table below from the logs there are.

**Observation.** `libc.a` is built with `zoraya-posix`'s `stats` feature and the call log (ADR-0211): each exported
function is a two-instruction wrapper that sets its byte of a bitmap, so a program's calls are logged and the
library's own are not, as TASK-02's interposer saw. Each process sends its counters and bitmap to the broker before
it exits or `exec`s; a process ended by a signal does not. The runner's `% NAME` steps give wall time on the host.

**Measured** on the M4 under QEMU 11.1.2 (one CPU, 512 MiB), one run per accelerator, from the clean clone's `make
test` and `make test-posix-long` of TASK-22 (`make posix-table`), beside Linux: TASK-02's and TASK-02b's counts on the Alpine aarch64 VM
(four cores; `tools/posix-surface/results/syscall-surface.csv`, `par/syscall-delta.csv`; durations under `strace`,
which TASK-02 recorded only for W8). "Programs loaded" is `exec` plus `posix_spawn` on Zoraya, `execve` on Linux
(each `vfork`+`execve` there is a `posix_spawn` here).

| Workload | Wall time, HVF / TCG | Linux | Programs loaded (Linux) | Share loading, HVF / TCG | `fork`s; mean `snapshot_process`, HVF / TCG |
|---|---|---|---|---|---|
| W1 (configure, make, check) | 3.5 s / 35 s | – | 59 (64) | 44 % / 32 % | 11; 0.68 / 7.2 ms |
| W2 (pdpmake, then GNU make) | 7.9 s / 74 s | – | 128 (127) | 65 % / 52 % | 2; 0.66 / 11 ms |
| W2-j (four builds) | 13 s / 134 s | 0.3 to 0.5 s a build | 181 (4 × 43 + checks) | 61 % / 48 % | 4; 0.77 / 14 ms |
| W3 | 1.1 s / 11 s | – | 7 (6) | 24 % / 17 % | 3; 0.60 / 8.2 ms |
| W3-j (four builds) | 4.3 s / 45 s | 0.2 s a build | 25 (4 × 4 + checks) | 23 % / 15 % | 12; 0.59 / 7.5 ms |
| W5 (session) | 2.2 s / 3.8 s | – | 8 (8) | 12 % / 52 % | 3; 0.76 / 8.8 ms |
| W8 (five builds) | 89 s / 488 s | 13.4, 6.8, 3.4 s at `-j1`, `-j2`, `-j4` | 571 (5 × 111) | 61 % / 66 % | 20; 0.44 / 10 ms |
| W9, Ctrl-C; SIGTERM (each once tables are being written) | 4.5 / 30 s; 4.8 / 97 s | – | – | – | none; 18 / 83 (the shell's wait loop) |
| W10 (parallel, serial) | 4.4 s / 25 s | – | 24 | 36 % / 32 % | 12; 1.7 / 9.8 ms |
| `tcc hello.c && ./a.out` | 0.4 s / 4.6 s | – | 2 | 11 % / 8 % | 0 |

The guest has one CPU, so W8 takes the same time at every `-j` (89 s is 17.7 s a build under HVF): `-j` gives
nothing here (`docs/backlog.md`). Loading programs is the largest single cost: each `exec` and `posix_spawn` reads
the whole program through the file protocol (99 ms mean in W8 under HVF, 423 ms under TCG); an image cache in the
broker is the next task (ADR-0212's Decision). Every child takes three broker round trips (prepare, commit, reap) in
every workload. The rest of ADR-0110 item 11, over every run above: path resolution 0.46 to 0.76 µs mean under HVF
(W1: 1,892 resolutions, 0.46 µs) and 33 to 119 µs under TCG; `share` 0 to 1,115 and `seek` 12 to 85,298 requests a
run; the request poll is 0.1 to 0.5 % of system calls in the builds and 5 to 21 % in the interactive sessions; W8 at
`-j4` wakes a process 5 times a second under HVF; the largest `spawn` buffer is 1,120 bytes of ADR-0042's 64 KiB; at
most 13 pids live at once of ADR-0031's 255, and 10 fds open in one process; `/tmp` at most 52 KiB (the temporary
volume's own blocks included).

**W4** (`make measure-w4`, `build/w4-report.md`): GNU hello's `configure` stops in its first lines under both makes,
`sed: not found` and `expr: not found` (T2's applets are not built), after 120 forks of dash: `snapshot_process` 0.55
ms mean under HVF and 5.7 ms under TCG, the whole fork in the parent 0.68 and 10.7 ms. T2's applets do not link
against `libc.a`: toybox's (T2's 31 of `applets-by-tier.tsv`) lack 20 names (`regcomp`, `regexec`, `regerror`,
`getline`, `mkdtemp`, `getgrgid`, `getgrnam`, `getgroups`, `getgrouplist`, `link`, `lchown`, `mknod`, `dirname`,
`dprintf`, `getentropy`, `gethostbyname`, `hstrerror`, `__h_errno_location`, `sethostname`, `towlower`), busybox's
`awk expr gzip` 9 (`regcomp`, `regexec`, `regerror`, `regfree`, `rand`, `srand`, `strncasecmp`, `fileno_unlocked`,
`fputs_unlocked`). On Linux W4 forked 1,167 times without `exec` (764 in `configure`, 403 in `make`) and ran 2,881
programs: at these means its forks would cost about 0.8 s under HVF and 13 s under TCG, and its loads, at W8's mean
of 95 ms and 580 ms, about 270 s and 1,700 s. The fork is not what T2 waits for; loading programs is. The order to
T2, as the architect decided (2026-11-04): the image cache first (ADR-0212), then T2's 29 missing names, then W4
measured for real.

## Console input and the first process

- **Console.** Raw bytes from the UART's receive interrupt, in a fixed ring (`crates/zoraya-console`), with a
  `READABLE` signal and `console_read`, which never blocks. The kernel has no line discipline (ADR-0060).
- **First process.** `init` (ADR-0053). The kernel reports it and powers off if it ends.
- **Line discipline.** Only in `ttyd` (ADR-0105, INV-0105-4: `make check-kernel-tty`).

## Using the desktop

`make run-gui` boots with a GPU, keyboard, tablet and the network (`--net`: the Internet through SLIRP, so `fetch` and `ping` work in the terminal; `NET=0 make run-gui` starts without it) and opens QEMU's window: on macOS at 2048 × 1536 with scale 2,
elsewhere at 1024 × 768 (`GUI_SCALE=1` or `GUI_SCALE=2` chooses). On a Mac the window can be resized and the desktop
follows it (7g: the bar and the dock follow the width, windows stay in the work area, and from 1920 pixels wide the
scale is 2, from 1280 on a Retina screen, ADR-0247), and Command is the system key (written System below). The serial line stays in the terminal for `init`'s reports; Ctrl-A X there quits QEMU.

- **What is on the screen.** The bar at the top (the layout in use at its right; the chooser's mark in its middle
  while a sheet is open), the windows, and the dock at the bottom: Terminal, Files, Edit and Settings, then the rooms'
  chips and `+`. A dot under a tile is a room in which that application has a window.
- **Windows.** Drag a title bar to move a window, an edge to resize it; the dash minimises it and the cross closes
  it. Alt+Tab shows the switcher, grouped by room; F12 goes to the next window; System+W, System+M, System+Left and
  System+Right close, minimise and tile; System+Enter opens a terminal. A new window takes the keyboard when you
  click it.
- **Terminal.** `zosh`, with the commands of `make run` (Using the shell, below); Shift with Page Up or Page Down
  scrolls back. `exit` closes it; in the first terminal `init` opens a new one; `poweroff` turns the machine off.
- **Files.** Your home folder (in a room, its places), as the 7h mockups draw it: double-click a folder to enter it
  or a file to open it with its application (`.txt` and `.md` in Edit), read only; click, Shift-click and
  Command-click select; the headers sort; arrows, Enter, Command+Up, F2 to rename in place, Delete (asked first),
  Command+Shift+N for a new folder; type to search; right-click for the menu (Open in edit, Copy path, Open terminal
  here); drag a file onto a folder to move it (Option: copy), onto a window to give it that file to read (its frame
  then shows `dropped: NAME read`).
- **Edit.** Open… and Save as… show the chooser's sheet under the window: pick a file, or a name in a folder, and
  only that is given to Edit (the frame's pills show it). Save writes to the name chosen; Find selects the next
  match; Ctrl+Z undoes, Ctrl+Y redoes, Ctrl+A selects all. Closing with unsaved changes asks first.
- **Copy and paste.** System+C copies what is selected, System+X cuts it, System+V pastes; nothing else can read
  what was copied.
- **Settings.** Theme (light or dark), motion, scale (Automatic follows the window's size) and the keyboard: the layouts
  System+Space switches between and the one in use. A change is saved to `/config/settings` and applied at once.
  `settings gui.theme dark` in a terminal writes the same file; the desktop reads it at the next session.
- **Keyboard layouts.** System+Space switches (US, UA, RU, CZ, those enabled in Settings); on the Czech layout the
  key right of `-` is the dead acute, with Shift the dead caron.
- **Rooms.** `room new project +w ~/project` in a terminal makes a room; its chip appears in the dock. Press the chip,
  then an application's tile: it opens in the room, holding the room's grants and nothing else (and, for a terminal,
  the toolchain to read), its title bar tinted and its grants shown as pills. System+? shows every gesture's command
  line.

## Using the shell

`make run` boots to `zosh`, the native shell (TASK-18; ADR-0150 to ADR-0159 and ADR-0200), on the serial console:
`Type help to begin, or help tour.` and the prompt `~> `; Ctrl-A X leaves QEMU, and `make run-gui` shows it in a
terminal window. [`docs/tour.md`](tour.md) is how a person meets it, every example with its exact screen;
`make test-posix` plays the whole tour on the serial console and `make test-tour-gui` in the window, from scripts
that `zoraya-tour` generates from that file (INV-0154-4). This section follows the tour's order and adds where each
part is built.

- **Getting your bearings** (tour 1 to 3). The shell starts in `/home` (`shell.home`), shown as `~`; `/config` holds
  `settings`, `start.zosh` and `history` (ADR-0157). `make run` (and `run-net`, `run-gui`) attaches `build/disk.img`, made from
  `rootfs/` by `tools/run-disk.sh` on first use and kept; when the on-disk format or `rootfs/system` changes, the old
  disk becomes `build/disk.img.old` and a new one is made (a disk `fsd` cannot mount gave the prompt `/>` and
  `fetch` "the file service is gone", TASK-26 hand-run). Without a volume the shell starts in `/` and file commands
  fail. `make test-hand-run` boots these three machines as a person runs them. `help` lists the topics, the commands
  and the demonstrations a screen at a time; a command that succeeds prints only its result.
- **Files and the language** (tour 4 to 8, 22 to 24; ADR-0150, ADR-0151; `zoraya_shell::syntax`, `glob`, `eval`).
  Words split only at spaces; `'...'` is literal, `"..."` expands `$name` and `$(...)`. Variables are lists
  (`set name 'meeting notes.txt'` is one argument), never split or globbed; `$name[i]`, `count $name`. `*`, `?` and
  `**` match names, a pattern that matches nothing is an error, and a match that starts with `-` comes as
  `./-name`. `if`/`else if`/`else`, `for NAME in WORDS`, `while`, `fn NAME PARAMS` (comment lines above it are its
  help), `try`/`catch`, `&&`, `||`, `!`, `source FILE`; `...> ` continues an open block or quote. Bash habits get a
  hint (`x=1`: `to set a variable, type: set x 1`; `grep`, `wc`, `man`, `less`, `more`, `which` when nothing of
  that name exists). `export NAME VALUE` and `NAME=value command` are the only environment a program gets.
- **Finding things and help** (tour 9 to 13; ADR-0154, ADR-0156; `zoraya-describe`, `zoraya-command`,
  `zoraya_shell::editor`). Every command has a description in `user/describe/` (the language's own in
  `crates/zoraya-shell/describe/`, POSIX programs' in `user/describe/posix/`, test programs' in
  `user/describe/tests/`, unlisted): `NAME --help`, `help NAME` and `help --search WORD` (paged), Tab completion of
  commands, paths and options, and argument parsing all come from it. A question command's "no" (`search` finding
  nothing) is status 1 and stops nothing. The editor shows a faint suggestion from history (Right takes it), turns
  an unknown command word red, searches history with Ctrl-R and prefix Up and Down, and redraws a line wider than
  the terminal in place (`ESC [ n A`, ADR-0154's note).
- **What a program is given** (tour 14 to 18; ADR-0153, ADR-0200; `zoraya_shell::grants`, `zoraya_user::namespace`).
  A program gets `in`, `out` and `err`, its own job inside its line's, the current directory to read and a start-up
  record (rule 8) with what its description designates: each path operand or option by its kind (`cp a b`: `a` to
  read, `b`'s directory narrowed to `b`; when that one name is a directory, the file service opens it through the
  narrowed connection to read and list only, so "create or replace this one name" never reaches what is inside it),
  and what `+r PATH` and `+w PATH` before the command grant. Nothing else:
  no console, no volume root. `grants LINE` prints it in the order of the line. An operand from a value reaches only
  inside the current directory or a grant (`set g '+w /'; $g rm x` is a command named `+w /`). A designated path
  through a link object is given as its target only inside the current directory or a grant (rule 5), and a
  native program follows links itself over its table, so no link leads out of it. A path a program names outside
  its entries is refused and reported, and the shell prints the grant that would allow it; a missing path inside an
  entry is just missing. Test programs without a description get what TASK-11's shell gave them.
- **POSIX programs** (tour 15 to 17, 27; ADR-0153 rule 10, ADR-0200; `zoraya_user::posix_child`). On a volume with
  `/bin`, `init` starts the broker beside `zosh` (`--no-shell`). `shell.path` (`/zoraya/bin /bin`) finds a command
  in the ramdisk, then on the volume, or by a path with a `/`; a program without the native mark (`.zoraya.native`,
  written by `zoraya-rt`, read by `zoraya_elf::is_native`) runs through the broker: `SpawnPrepareIn` in a `posix`
  job of the line's job, a bootstrap record `zosh` builds (the toolchain set: `/bin`, `/usr` and `/etc` to read,
  `/proc/self/exe`, `/dev/null`; a `/tmp` of the line's own, removed when the line ends; the current directory; and
  what a description compiled into `zosh` designates for the path it names), fds 0 to 2 on the line's streams,
  `PATH` and `HOME` unless the line sets them, `SpawnCommit`, and `Reap` at the end. TinyCC's messages are its own.
- **Pipelines and streams** (tour 19 to 21; ADR-0152). `zosh` holds the shell's end of every stream and relays it
  from its event loop, never waiting on one: output to the terminal, a file or a `$(...)` (one element per line);
  input from the keys (a line at a time, as a terminal in canonical mode), a `< FILE` or the builtin or function
  that starts the pipeline. `a | b | c` is one job with one stream per `|`; `&|` sends `err` too; `>`, `>>`, `<`,
  `2>`, `2>>` and `&>` redirect programs, builtins and functions. A pipeline's status is ADR-0152 rule 6's
  (`pipeline_status`); a writer whose reader closed exits 0. `zoraya-rt`'s `print!` writes a print of up to 512
  bytes at once, so lines of two programs on one stream stay whole; `cat` passes bytes unchanged, and the terminal
  shows bytes that are not UTF-8 as U+FFFD. A line joins the history once it has run.
- **When things go wrong** (tour 25 to 27). Errors read `NAME: SUBJECT: PROBLEM` with a `hint:` when one is known
  (ADR-0155). A failure nothing checks stops the rest of the line (`(the rest of the line was not run)`), block,
  function or script (`zosh: FILE line N: NAME: WHAT`); `$status` is the exit code or a word (`interrupted`,
  `crashed (...)`), and `status` describes the last record.
- **Jobs** (tour 28 to 30; ADR-0158, ADR-0200; `zoraya_shell::jobs`). A line's pipeline is one job, numbered from 1;
  `&` runs it in the background with its input at its end and its output above the line being edited. Ctrl-C
  interrupts the job in front, and a second Ctrl-C after `(Ctrl-C again to kill it)` kills it; Ctrl-\ sends Quit;
  Ctrl-Z stops it (each native element by `job_suspend` of its own job, each POSIX element by `SIGTSTP` through the
  broker, whose handler runs first) and ends the line there. `jobs`, `fg`, `bg`, `wait` and `kill` act on `%N`; a
  job's end is reported once, by `wait` or `kill` or before a prompt. `exit` with jobs warns once.
- **Settings and ending** (tour 31, 32; ADR-0157, ADR-0159). `settings`, `settings NAME [VALUE]`,
  `settings --reset NAME`, `settings --all`, `settings --check`; `shell.prompt`, `shell.home`, `shell.path`,
  `shell.history`, `shell.history-size`, `shell.suggestions`, `shell.colour`, `console.columns`. `exit` ends this
  shell and `init` starts a new one (a new window in the GUI); `poweroff` syncs and turns the machine off, `reboot`
  syncs and resets it through PSCI `SYSTEM_RESET` (`power_reset`).
- `posixrun [-m PREFIX=PATH[:ro]]... [-C CWD] [-o FILE] [-e NAME=VALUE]... PROGRAM [ARG]...` runs a C program from the
  volume through `libposix` without the broker (stage 6c's test launcher, `user/src/bin/posixrun.rs`).

## Conventions for commands

Every native command follows ADR-0155, the style guide, through `zoraya-command` and its description (ADR-0156):
one option syntax (`-f`, `--force`, grouped short options, `--` ends options, a value after an option or with
`=`), `--help` printed from the description (`NAME: SUMMARY`, `usage:`, the options, `more: help NAME`), a usage
mistake as `NAME: WHAT` with the usage and exit 2, errors as `NAME: SUBJECT: PROBLEM` with an optional `hint:`, and
the words for statuses from one table (`not found`, `not granted`, ...). A command is quiet when it succeeds: it
prints only its result. Exit 0 is success, 1 a failure or a question's "no", 2 a usage mistake, as its description
lists them; a writer whose reader has gone exits 0. A command reads its arguments only through `zoraya-command`
(INV-0155-2, `make check-argv`), and a description that designates paths is authority: only the system image's
designate (ADR-0153's Decision). Builtins are the commands that act on the shell itself (`cd`, `set`, `jobs`, ...)
and `sync`; the file commands are programs in the ramdisk. `rm` refuses a path that is `/`, or whose last
component is `.` or `..`, with or without `--recursive`; `project/` is `project` (TASK-18 review S20).

## Quotas (stage 8, Q1, Q2 and Q4: memory, processes and CPU on jobs, bounded kernel work)

A job can carry a memory limit and a process limit (`job_set_limits`, needs the `LIMIT` right, which `job_create`
returns and a launcher withholds from what it hands on). A limit never passes the parent's effective limit. Everything a
program creates is charged to its job and to every job above it (user frames, page tables, memory objects, a process
with its thread, kernel stack and handle table, channels, ports, streams, events, images, subscriptions, and each queued
message to its writer). A request that would pass a limit fails with `OverQuota` and changes nothing; the job whose limit
refused it asserts `OVER_QUOTA`. Nothing is killed; a CPU ceiling slows (Q4, below). `job_get_usage` reads `limit`, `used`, `peak`,
`refused`, and the user/kernel split.

`init`'s job holds the frames free after boot less 1/16 (the kernel reserve, ADR-0261 point 10's assumption, kept until a Q task measures what the kernel needs), and `init` puts `ttyd`, the broker and the
shell in a session job with three quarters of that and 200 processes. `room new --untrusted` is still refused
(`make check-untrusted-gate`; the list of tests that end the refusal is `tools/untrusted-gate.txt`).

Measured with Q1 (HVF, default guest, test kernel): `portbench` 1 idle 2208 ns, 10000 idle 1167 ns; `chanbench` round trip
16 B 8167 ns. No run of the same benchmarks before the change was made in this session, so the cost of the charge on
those is not known; the charge is one lock and one to four adds per allocation. The reserve: at 1/16 and at 1/32 the 256 MiB
GUI test refused the files manager's menu; the GUI guests now have 512 MiB and the reserve stays 1/16 (the footprint
is a row of `docs/backlog.md`).

What is charged for the reserves that the heap shows only as they fill: a process, its handle table at the largest (1024
slots) while it lives, and the charge goes back when it exits (a zombie keeps its record, not the table); a port, its
record, its place in the event tables and room for 64 packets, then 64 more each time `port_queue` finds its ready list
longer than what it has paid, kept until the port ends; a channel, its queues' slots at their most (1 KiB).

Measured at the end of the GUI test at 256 MiB, before the guests were raised (`files-manage:auto:2`): `init`'s job at
230.85 of 233.6 MB used (peak 233.1 MB), with 10.7 MB of frames really free, the charge being within 0.5 MB of the frames
and heap pages in use. The over-charges are therefore not what refuses the terminal window: the session really uses
nearly all of RAM. Before the change to ports and exits the figure was 231.2 MB.

Known limits of Q1: a live process is charged a 1024-slot handle table whatever it holds; `snapshot_process` is refused page by page, not before
copying.

**Q2: processes, the fork bomb and bounded kernel work.** The `processes` field is charged first in `spawn` and
`snapshot_process`, before any page: a fork bomb in a limited job stops at its limit (`t_quota`: 24 processes reached in
about 5 ms with 23 refusals) and a command started outside it ends at once. `libposix` reads the field of the last
`OverQuota` (`zoraya_rt::last_refusal`, kept by the runtime from the value register) and maps it by call: `processes`
to `EAGAIN` for `fork`, `vfork` and `posix_spawn`, `memory` to `ENOMEM` (`zoraya_posix::errno::from_refusal`).

Long calls proceed in steps (ADR-0270): `memory_create`, `memory_map`, `memory_unmap`, `snapshot_process`, the image
copy and stack of `spawn` and the close of a chain of channels do at most 64 pages or messages and then call
`steps::between` or `steps::pause`, with interrupts on and no spinlock held (the test kernel asserts it). `between`
fails with `Cancelled` if `job_kill` came meanwhile, and the call frees what it did: nothing is visible until the last
step. The frames of a closed memory object over 64 pages and of an address space over 64 frames go to the `reaper`
kernel thread (`kernel/src/reaper.rs`) with their charge, which stays on the job until they are freed. An allocation
that finds no free frame first frees a step of the reaper's queue, so memory on its way back is not refused. The test
kernel counts the longest time IRQ stayed masked (`MASKED_MAX_NS`, `kernel/src/steps.rs`), without the idle thread's wait.
Measured under HVF, 24 MiB (`t_bound`): before the steps `memory_create` 1707 us, `memory_map` 1333 us, `memory_unmap`
786 us and the close of the object 962 us masked; after, one step and the whole are the same, about 19 us, 15 to 19 us,
8 to 10 us and 11 us. See the report for the rest.

**Q4: the CPU ceiling** (ADR-0264, proposed ADR-0285 for the details). `job_set_limits` takes `cpu`, 1 to 1000
thousandths of the machine (0 and more than 1000 are `InvalidArgument`, as is one above the parent's effective ceiling);
it is set together with the other two limits or not at all, and `None` clears the job's own ceiling. A period is 20 ms
on boundaries every job shares, so 250 is 5 ms of CPU in each period. The logic is the crate `zoraya-cpu` (host
tested on a simulated clock); `kernel/src/cpu.rs` gives it the job tree and `kernel/src/sched.rs` calls it:

- At every switch away from a user thread its run time since it was switched in is charged to its job and every job
  above it (`used_ns` since creation, `period_used` for the period of the switch; a run that crosses a boundary counts
  only its part after it). Time in interrupt handlers while the thread runs is charged to it too.
- A thread reaching the front of the run queue whose job, or an ancestor, has spent its budget goes to the parked list
  instead of running (at most 16 per pick; one past that runs for the 50 us floor and is parked at its next pick).
  Otherwise it runs with a slice of the smaller of 10 ms and the budget left (at least 50 us), armed also when it is
  alone. Unlimited jobs keep ADR-0030's rule.
- One deadline at the next boundary, armed only while something is parked, returns the parked threads to the end of the
  run queue, 64 a time with an immediate deadline for the next 64, so interrupts are unmasked between batches. The
  deadline queue keeps two slots for the scheduler (the slice and this one).
- `job_get_usage` writes 120 bytes when the buffer holds them (`cpu.limit`, `cpu.used_ns`, `cpu.period_used`,
  `cpu.parked`); the 88-byte record of Q1 is still accepted and read with the CPU words as zero.

Measured under HVF, default guest, test kernel (`t_cpu`, one run each): eight spinners in a room at 25% for 2 s used
502 ms of a budget of 500 ms (parked 802 times); three rooms at 25% under a job at 50% used 1006 ms of 1000 ms; the longest
interval with IRQ masked over those tests 314 to 1051 us (units of work at most 69), against 2 ms. The wake-up lateness
of an uncapped process that sleeps 1.7 ms at a time next to two rooms at 25% each under 50%, each with 16 spinners and 16
processes that sleep until 1 ms before a boundary and then spin 3 ms (400 wake-ups):

| Period | median | p99 | max |
|---|---|---|---|
| 10 ms | 749 us | 5.6 ms | 5.7 ms |
| 20 ms (the default) | 682 us | 9.3 ms | 9.6 ms |
| 100 ms | 683 us | 49.7 ms | 50.1 ms |

The maximum is about the capped rooms' share of one period (50% of the period), as the arithmetic of ADR-0264 option D
says, and the 50 ms of INV-0264-3 is met with room at 10 and 20 ms and only just at 100 ms. This is the scheduler's part of
a key's latency; the terminal's own echo time comes on top of it. The GUI test of INV-0264-3 (a keystroke to a terminal
in `home` next to two untrusted rooms) was not written: it needs rooms with `--cpu` (Q6a) and untrusted rooms are
refused (the gate). TCG was not run here; CI runs it with the calibration `t_bound` uses.

After the review (W1): at the cap of 16 parks per pick the next spent thread goes back to the front and idle calls `schedule`
again, so a spent job with 200 threads runs no more than one floor per period (host test); `t_cpu` runs 40 spinners in a
room (500 ms of 500), a job about 30 levels deep (155 ms of 152 ms; masked 317 us) and a `job_kill` of a room whose threads are
parked (gone in 37 ms at a 100 ms period). The ceiling is also checked by counts: next to an uncapped spinner (so the CPU
never idles) a room at 250 counted 17 to 23% of the two (ceiling 25%), three rooms at 500 counted 45%.

Known limits of Q4: rooms under one parent get no share (with three rooms at 25% under 50%, one used 0 ms in 2 s);  a ceiling below about 0.25% runs at about that (the 50 us floor); a thread parked when its job is
killed ends at the next boundary (at most one period); IRQ time is charged to the interrupted thread; one CPU (an SMP kernel gives each CPU a share of one pool, ADR-0264).

## Rooms with limits and the room keeper (stage 8, Q6a)

A room is one job, whoever launches into it (ADR-0269, proposed ADR-0293). `init` starts `rooms`
(`user/src/bin/rooms.rs`; the decisions are `crates/zoraya-rooms`, the wire is `crates/zoraya-proto-rooms`, both host
tested with a model of the kernel) when the volume is up. It holds the session job (`LIMIT`) and a read connection to
`/config`, makes the untrusted job under the session (half its memory, 96 processes, 50% CPU; ADR-0266 point 4) and one
job per room at its first `Launch`: under the session for a trusted room and `home`, under the untrusted job for an
untrusted room, with the limits of the room's line. A launcher gets a *launch job*, a sub-job of the room's job with
every right but `LIMIT`, and launches in it as before. Three kinds of channel speak the protocol: `Manage` (the panel,
the console's shell: `Launch`, `Leave`, `Set`, `Usage`, `Report`, `Stop`, `Open`), `Room(R)` (the shell the panel
launches in a room, and `top` run in it: `Launch` and `Usage` of R) and `Usage` (reads). Which kind a message came on
decides what it may do, never what it says. At most four untrusted rooms run; an untrusted room is used from one
`Manage` kind at a time (`room: agent-1: running from the desktop`; a lease is per channel and is given back by that
channel's `Leave` or its closing, and the holder is known by kind, never by slot); a room with no processes, no lease and
no launch for 5 s is let go. The keeper tells `init` what it makes
(`Keep`: a duplicate of each job, with `LIMIT`, its trust and its last holder's kind, sent before the launcher gets the
launch job) and lets go (`Forget`); `init` restarts it up to three times and gives
the new one those jobs, so `Stop`, `Set` and `Usage` still work and a room never gets a second job. The panel's and the
console shell's `Manage` channels come from `init` (slots 8 and 11 of those programs) with a second channel (9 and 12)
on which `init` gives a new one when the keeper has been restarted; a shell in a room gets its `Room(R)` channel at
slot 11 from the keeper through the panel (`Open`).

The room's line has six more words (`zoraya-settings`: `--mem SIZE --cpu PCT --procs N --disk SIZE --conns N --tmp
SIZE`, ADR-0266): a trusted room has the ones it is given, an untrusted line has all six (the keeper fills a missing one
in memory from ADR-0266 point 3's defaults; `room new` writes them), and no limit may be above its parent's: the keeper
refuses it when it makes the job or applies `Set` (`room: agent-1: --mem 512M: above the untrusted rooms' 178M`).
`room new NAME --mem 64M --cpu 25% +w DIR` makes a room with limits; `room set NAME --mem 128M` rewrites the line, asks
the keeper to apply it to the running room's job and puts the old line back if the keeper refuses; `--disk` is set on
each `+w` directory through the file protocol's `SetLimit` (ADR-0292 option A); `room remove NAME --kill` ends the room's
processes first; `room` lists the limits after the trust; `grants` in a room ends with
`limits --mem 64M --cpu 25% ...  (room NAME)`, read from the keeper's record. When a line leaves the room's job at its
memory or process limit the shell prints one hint naming the field and the room (`hint: room agent-1 is at its memory
limit of 64M; room set agent-1 --mem 128M`; in a room's own shell, `from home: room set ...`).

Try it: `room new agent-1 --mem 64M --cpu 25% --procs 16 +w ~/sandbox/agent-1`, `room agent-1`, `grants ls`,
`memhog` (takes memory until refused), `room set agent-1 --mem 68M`, `room --leave`, `room remove agent-1 --kill`
(`tests/shell/rooms.script` does this on the console). The QEMU test `t_rooms` runs the real keeper under a session job
it makes: one job for two launchers; no launch job carries `LIMIT`; a `Room(R)` channel may send only `Launch` and `Usage`
of R; a hog is refused and the keeper's `Refused` reaches the room's, the overview's and the console's channels; `Set`
above the untrusted job is refused; four untrusted rooms, each with a fork bomb and a hog, while a `home` program starts
16 processes and maps 16 MiB; a restarted keeper stops a room it did not start. The keeper's round trip per `Launch` was
24 us (average of 50, HVF, `home`).

Known limits of Q6a: `room new --untrusted` and the panel's untrusted launch are still refused (the gate); an untrusted
room's `/tmp` (`--tmp`, a `tmpd` per shell in the room's job) is not built, and the console shell's broker and `tmpd`
are `init`'s, outside an untrusted room's job (INV-0268-3 is open; the panel's broker is in the room's launch job); the
rooms overview's `New room` form does not exist (Q6b built the hover card, below); a POSIX session
(`/etc/zoraya/session`) starts no keeper channel for its shell; the keeper restart in `init` itself, and the reconnect
channel, are not run in QEMU (`t_rooms` stands in for `init`); a launcher that never sends `Leave` and keeps its channel
holds an idle untrusted room. A resumed room is leased by its old holder's kind until a launch of that kind claims it, it
leaves, a `Manage` channel of that kind closes or the room is stopped (no time limit); `home` is refused `Stop` and `Set`; a shell or the panel will not enter a room whose line has limit words
without a launch job from the keeper.

## What the person sees of limits (stage 8, Q6b)

One `Reading` (`zoraya-settings`' `usage` module) is the keeper's usage record in plain numbers, and every text of a room's
limits and use is made from it (proposed ADR-0294, ADR-0267): `top`'s table, `room`'s words with the use after each limit
(`--mem 64M (58M)`), `grants`' `limits ...` line, the pills and the hover card; `zoraya-rooms` has the conversion from a
record (`reading_of`) and `Tracker`, which gives each room's CPU share since the last reading. Usage is read at most once a
second by every reader (`Throttle`, INV-0267-2); nothing is read on a calm desktop beyond that.

- **`top`** is a builtin of `zosh` (ADR-0294 point 1; ADR-0267 says a program with a keeper slot): `top` prints `ROOM MEM
  CPU PROCS DISK CONNS`, a row per room that runs and a `session` row that sums them, `USED of LIMIT` where a room has
  a limit and `(refused 3 mem)` at the end of a row; `top ROOM` adds the room's limits and the line that raises the limit
  that refused; `top --watch` prints again every second until Ctrl-C. A room's own shell sees that room alone; without a
  keeper it prints `top: no room keeper` and exits 1. DISK and CONNS show the limit only (no launcher forwards the
  services' use yet); `top --session` and the job tree of `top ROOM` are not built (the keeper has no session ceiling to
  give, and no kernel call lists a job's children).
- **The panel** reads `Usage` from the keeper on its once-a-second tick and sends the compositor `SetUsage` (request 44 of
  the client manager's channel) when the text changed. The compositor checks every line again, takes all or none, and
  draws from it and from nothing a client sends (INV-0172-1): a **pill** `mem 99%` (`procs`, `cpu`) at 90% of a limit in the
  title bar of each window of that room, before the grants so that a narrow bar folds the grants first; and, while the
  pointer is on a framed window's title bar, a **hover card** outside the window (above it, else below it; starting on the title bar if neither side is free), where no pixel of the client's buffer reaches: the room, its grants as the record has them, then
  `mem 65473K of 64M`, `procs 3 of 16`, the `limits ...` line, `(refused 1 mem)` and `raise: room set project --mem 128M`
  (the notice's text; there is no `Raise` button until the rooms overview exists). A room with no limit has no card and no
  pill, so no text of its window is covered. The card is the compositor's, above the windows and the dock and under the bar,
  and takes no input; it is worked out again after every restack, close and new window.
- **The compositor's accounts** (ADR-0262 point 2, INV-0262-1 and -3 for the compositor): an untrusted room's windows are
  an account of 3 surfaces and 2 display areas of buffer pixels, all untrusted rooms together 12 surfaces and 8 display areas
  in at most 4 accounts, which leaves 20 of the 32 windows to `home` and the trusted rooms. `CreateToplevel`, `CreatePopup`
  and `SetBuffer` past a bound answer `OVER_QUOTA` (status 8), count the refusal for the room (`Compositor::accounts`) and
  change nothing else. The count does not reach the keeper yet.
- **The settings window's `Rooms` section** lists each room of the table (the panel answers `RoomsQuery`: the window cannot
  read `/config/rooms`) with a field per limit word (`--tmp` for untrusted rooms), the use beside it, the `room set` line of
  what is being edited, and Apply. Apply sends `SetRoomLimit` per changed word to the panel, which does what `room set` does
  (`rooms::with_limit`, the file, `Set` to the keeper, the old line back if refused); a settings window launched in an
  untrusted room is refused. The window holds no `Manage` channel and no new right (ADR-0294 point 8).

Try it: `room new project --mem 64M --procs 16 +w ~/project`, enter it from the dock's chip, System+Enter, `top`, `memhog --hold 6 &`
and look at the title bar and move the pointer onto it (`tests/gui/limits.script` does this: 37 steps, `make test-gui GUI_RUNS=limits:auto:1`,
shots `limits-calm`, `limits-pill` and `limits-card` in `build/shots/`); `top` on the console is in `tests/shell/rooms.script`.

Known limits of Q6b: the rooms overview (System+R, the `+` chip's form) is not built, so its usage lines and `Raise` are not
either; the badge and the dock chip carry no pill; disk and connection use are not reported; the compositor's refusals are not
forwarded to the keeper; the settings window's Rooms section is not run in QEMU (its logic is host-tested; the panel's handler
is not).

## Network driver (stage 8, N1: the driver answers ARP)

`tools/run-qemu.sh --net` (ADR-0230 item 2) puts `virtio-net-device` on transport 16 (page 2, alone, ADR-0091) over
QEMU's user-mode network (SLIRP: guest `10.0.2.15/24`, gateway `10.0.2.2`); with `--test` it is `restrict=on`, and
`--net-pcap FILE` keeps the frames (256 bytes each, `--net-capture-all` for whole frames). `--net` with `--gui` works
(N6, ADR-0221 item 8). The three machines a person starts: `make run` (the serial console, no network), `make run-net`
(the serial console with the network, without `restrict`) and `make run-gui` (the window, with the network unless
`NET=0`); each is one definition in the Makefile (`RUN_CMD`, `RUN_NET_CMD`, `RUN_GUI_CMD`), which `make test-hand-run`
boots as it is. The service on top of
the driver is described under "Network service" below.

- **`ethd`** (`zoraya-virtio-net`, ADR-0221 item 1, proposed ADR-0280). One process, the device at slot 5 and the
  client's channel at slot 6. It negotiates `VERSION_1`, `VIRTIO_NET_F_MAC` and `STATUS` only (offloads the device
  offers are not taken), reads the MAC from the configuration, and sets up one receive and one transmit queue of 128
  descriptors (two per buffer: a header cell in the driver's own queue memory and the frame slot). The logic is host
  tested against the fake device, including every hostile value: a used length beyond the buffer or below the header, a
  head that is odd, out of range or not offered, a used index that jumps, a slot completed twice, a transmit of a bad
  slot, length or state, and a random mix of requests and lies (`cargo test -p zoraya-virtio-net`). The client half
  (`zoraya_virtio_net::serve`, over a `Host` trait) is host tested too: a full channel for many rounds and then
  drained loses and doubles nothing, a hostile client is refused or ends the driver. A broken device
  ends the driver (reset, unpin); a runt frame is dropped and its buffer reposted; a request the client may not make is
  refused and counted. The driver never reads a frame slot (INV-0280-1).
- **The frame protocol** (`zoraya-proto-nic`, ADR-0221 item 2). The client creates a contiguous object of 128 slots of
  1536 bytes (64 receive, 64 transmit; 192 KiB), sends it with `Attach`; `Received { at, frames[] }`, `Sent { slots[] }`,
  `Rejected { slots[] }` (a refused transmit, with its reason) and `LinkChanged` are pushed, and what the client's
  channel cannot take is kept and written again (bounded state, not a queue: ADR-0280 recommendation 5), so no `Sent`
  is lost and a slot is not reused before it was reported; `Transmit` and `Release` are the client's. `zoraya_proto_nic::arp` parses and builds
  ARP over IPv4 with checked slices only (truncated, wrong hardware or protocol type or sizes, unknown operations, a
  multicast sender: none panics; host tests and the fuzz target `fuzz/fuzz_targets/net_rx.rs`, not run).
- **`init`** starts `ethd` and `netd` when a virtio-net device is alone on its page, and supervises both (N2, below).
- **`t_net`** (a test of the test kernel, `make test-net`) plays `netd`: it starts `ethd`, attaches a frames object, sends
  hostile requests (refused, with a reason), then sends 20 ARP who-has for `10.0.2.2` through the transmit path and reads
  SLIRP's reply through the receive path. `make test-net` forwards a host port to the guest and connects to it, so that
  SLIRP asks who has `10.0.2.15`; `t_net` answers by hand, and the step checks the pcap holds both replies. At the end
  `t_net` closes the channel and expects `ethd` to reset the device, unpin both memories and exit with code 0 by itself.
- **The ID register.** The kernel prints `ID_AA64ISAR0_EL1` at boot (ADR-0227's open question). QEMU 11.1.2:
  `0x0221100110212120` under HVF on the Apple CPU (AES 2, SHA1 1, SHA2 2, CRC32 1, atomics 2, RNDR 0), so RNDR is not
  there under HVF; TCG (`cortex-a72`): `0x0000000000011120` (AES 2, SHA1 1, SHA2 1, CRC32 1, atomics 0, RNDR 0), RNDR absent
  too. ADR-0227's RNDR input is therefore unavailable on both, and the provider check below does not need it.
- **The TLS provider check** (ADR-0228's Decision). `rustls 0.23.46` (`default-features = false`, features `ring` and
  `tls12`) with `ring 0.17.14` builds for `aarch64-unknown-none` on the pinned toolchain, with the host's `clang` as
  `CC_aarch64_unknown_none`, `CFLAGS_aarch64_unknown_none="--target=aarch64-unknown-none-elf -ffreestanding -isystem
  <sysroot>/usr/include"` (musl's headers from `build/sysroot`; without them ring's C stops at `assert.h`), and two
  more things the survey did not know: ring's feature `less-safe-getrandom-custom-or-rdrand` (else `SystemRandom` has
  no implementation), and `getrandom 0.2` with its `custom` feature and `register_custom_getrandom!`. `cargo build
  --release` produces ring's C objects and the libraries. Nothing was run on the target: whether ring's aarch64 code
  works without an operating system, and the source of its randomness, are N5's. The POSIX side's library is OpenSSL 3.5 (ADR-0312, 9a), not BearSSL.
- **Measured** (`t_net`, 20 ARP rounds, QEMU 11.1.2, test kernel, 128 MiB, 1 CPU, 2026-11-11): HVF: ARP round trip
  (guest transmit to the reply in `t_net`, through the driver and SLIRP) median 42 us, min 40 us, max 258 us;
  interrupt to the frame message read in `t_net` (the driver's wake, the channel write, `t_net`'s wake and read) median
  7 to 8 us, max 11 us. TCG: ARP round trip median 384 us, min 372 us, max 1309 us; interrupt to message median
  190 us, max 794 us. The runs share a host with other work; a single run each.

## Network service (stage 8, N2: `netd` answers ping and DHCP)

`netd` (`user/src/bin/netd.rs`, ADR-0221 item 3) is one process, one thread and one port. It is the client of `ethd`
(the frame protocol of N1), runs smoltcp 0.14 over it (`zoraya-net`, features: Ethernet, IPv4, ICMP, UDP, TCP with
Reno, the DHCPv4 client; no IPv6, no `std`, no `log`), and serves the scope channels of `zoraya-proto-net`. The
stack's TCP client and its resolver are described under "TCP, names and `fetch`" below.

- **Start.** `init` starts `ethd` and `netd` once the storage stack is up (`netd` reads `/config/settings` and needs the
  volume for it; without a volume it runs on the defaults). `netd`'s table: the console, slot 5 the channel to `ethd`,
  slot 6 a read-only `/config` directory connection (read once, then closed: there is no reload), slot 7 the root
  connection whose other end `init` keeps. `netd` asks the driver for `Info`, creates the contiguous frames object,
  `Attach`es it, builds the stack and prints `netd: net0 MAC`. It holds no device, interrupt or pin; it prints that after
  looking at its own start-up table (`zoraya_user::start_table`), and `ethd` prints that it holds one channel
  (INV-0221-1; a self-report, since no call lists another process's handles).
- **The interface.** `zoraya-net`'s `Stack` (a `Link` of copied frames under smoltcp's `Device`). The MAC is checked
  unicast before smoltcp sees it (the interface panics otherwise). A frame is copied out of its slot into a private
  buffer, the slot is released, and only then is the copy parsed (the driver can write a lent slot). At most 64 frames
  in and 64 out are moved in one turn; `poll` then returns 0 and the loop serves its other ports. smoltcp does not answer
  echo requests addressed to the guest (feature `echo-reply` is on in the host tests only).
- **DHCP.** smoltcp's client. The lease is applied only if the address is unicast with a prefix of 30 or less; the
  host name from `net.hostname` is sent as option 12; the lease time is read from the server's packet. A renewal
  changes what `status` returns and writes nothing (INV-0229-3; `netd` holds no writable connection to `/config`). The
  stack starts, `net.address = static` applies `net.ip`/`net.gateway` at once, and a static setting without what it needs
  leaves the interface without an address and says why once.
- **Requests** (`zoraya-proto-net`, `Service`). `status` (right `status`), `ping` (right `reach` and a reach rule that
  covers the address: a diagnostic of up to 16 echo requests, one every 200 ms, one run at a time in the process;
  proposed ADR-0281), `narrow` (right `narrow`, a scope and rights inside the caller's). An unknown ordinal gets
  `ErrorReply`; a message that does not decode, or comes with handles it should not have, ends that connection and no
  other. At most 64 connections. Replies carry the request's transaction. A connection that does not read its replies
  is closed.
- **The scope** (`zoraya_abi::net::Scope`, ADR-0220). A total decoder over bytes, a matcher, and `within` for `narrow`
  (a child rule must lie inside one rule of the parent). Names are lower-case ASCII with DNS's limits and are never
  matched against an address rule (INV-0220-3, -4: 11 host tests with random scopes, truncations and mutations).
- **Supervision.** `init` ends both jobs and starts both again, at most three times per boot, when either ends (service
  first, so the new driver resets the device and the kernel releases the dead one's frames). A running shell keeps its
  dead connection and `net status` says the network is not available until the shell is started again; ADR-0221 item 6's
  services channel is not built.
- **`net status`.** The shell holds a root connection at slot 9 (`init` makes it with `narrow` before each shell start).
  A base-system command whose description says `needs net-status` (`net`) gets a connection with only the `status`
  right and an empty scope, in the new `net` slot of its start-up record. `net status` prints
  `net0  up  52:54:00:12:34:56`, the address with its source and the lease left, the gateway, the DNS servers and the frame
  counters. The `net.*` settings are declared in `netd`'s description (`address` and `prefix` are new setting types):
  `settings net.gateway 10.0.2` is refused.
- **`ping`** (proposed ADR-0330). `ping [-c COUNT] [-W SECONDS] HOST` (count 1 to 16, default 4; timeout 1 to 30 s, default 2)
  sends the echo requests of `netd`'s diagnostic `ping` (ADR-0281 item 3; no raw socket), one every 200 ms. HOST is a name or a
  dotted IPv4 address, written on the line: the description's `host` operand kind gives the command a connection whose rule for
  that host is ICMP echo only (`Protocols::ECHO` in `zoraya_abi::net`; no TCP, no UDP; `grants ping 10.0.2.2` shows
  `echo only (operand HOST)`). From a value the host needs a `+net` word that covers it (a `+net` word covers TCP, UDP and
  echo). A name is resolved by `netd` with its resolver, inside the grant, as `fetch` does; the `Ping` request has the
  name (`Data<253>`), the reply the address. Output: `PING NAME (ADDRESS): N requests`, a line per reply
  (`40 bytes from 10.0.2.2: seq=0 time=0.412 ms`) and a summary (`4 sent, 4 received, 0% loss`,
  `rtt min/avg/max = ... ms`); the exit status is 1 when no reply came and 2 for wrong use. `netd` answers when the run ends,
  so the lines come together at the end. `PingReply` carries each request's round trip (`rtts`). Covered by
  `tests/shell/net-fetch.script` (`make test-net-fetch`: an address and a `net.hosts` name), `net-lookup.script` (`make
  test-net-lookup`: the first command of a boot, by name over the wire, answered late or never), `net-dns.script` (names that do not
  resolve or are local, with the host's resolver), `hand-run-net.script` (`make test-hand-run`, the production image against
  SLIRP's gateway), `t_netd` (an echo-only grant opens no TCP connection and pings no other host) and the host tests of
  `zoraya-abi`, `zoraya-net`, `zoraya-shell` and `zoraya-proto-net`.
- **What SLIRP gives.** Under `restrict=on` (every test) the lease holds the address and its time only: no router and no
  DNS server, so `net status` shows neither. Without `restrict` (`make run-net`) SLIRP gives gateway 10.0.2.2 and DNS
  10.0.2.3. The lease time SLIRP gives is 24 h. ICMP to the gateway 10.0.2.2 is answered by SLIRP either way, and the
  address is on-link.
- **Tests.** `cargo test -p zoraya-net` (39 tests: the scripted DHCP server and echo responder, renewal, silence and
  retry, static and unconfigured, ping with replies and without, 6000 hostile frames, a lying link, a flood served in
  bursts, the slot bookkeeping against a hostile driver, the requests, `narrow`, the settings), `cargo test -p
  zoraya-proto-net`, `-p zoraya-abi`, `-p zoraya-describe`. `make test-net` runs `t_net` (N1), `t_netd` and the normal
  kernel with `--net` and `tests/shell/net.script`. `t_netd` plays `init`: lease, `net status`'s text, `ping 10.0.2.2`
  (4 of 4), scopes, hostile requests, `netd` killed with a ping in flight (the client sees PEER_CLOSED, `ethd` exits 0
  after resetting the device, a new start gets a lease), `ethd` killed with frames in flight (`netd` exits 1, the next
  `ethd` releases the quarantine, a new lease, 4 of 4 echoes). The step also checks the pcap for the DHCP exchange and
  an echo request and its reply. The fuzz target `fuzz/fuzz_targets/net_stack.rs` was compiled and driven with 200,000
  random and near-valid inputs in a scratch crate in debug mode (not run under libFuzzer: no nightly toolchain here).
- **Measured** (`t_netd`, QEMU 11.1.2, test kernel, 128 MiB, 1 CPU, 2026-11-11, single runs): HVF: lease 110 ms after
  `netd` starts; `ping 10.0.2.2` 4 of 4, round trip min/avg/max 199/399/538 us; both processes started and leased in
  about 115 ms; restart after `netd` killed 124 ms to a lease, after `ethd` killed 126 ms. TCG: lease 114 ms; `ping 10.0.2.2` 4 of 4, round trip 3492/4321/4853 us; restart after `netd` killed 180 ms to a lease, after
  `ethd` killed 183 ms; `netd` killed with a ping in flight, client closed and `ethd` stopped 324 ms later (300 ms of that is the
  wait before the kill).

## TCP, names and `fetch` (stage 8, N3: a native program fetches a page over HTTP)

`netd` now opens TCP connections for its clients (`zoraya_net::conn`, `tcp`, `dns`; proposed ADR-0286). `fetch
http://HOST[:PORT]/PATH` is the first program that uses them.

- **`connect`** (`zoraya-proto-net` ordinal 3, ADR-0222 item 3 with the client's stream ends, its note of 2026-11-10).
  The client makes the connection channel and two streams (`user/src/netconn.rs`), so the 128 KiB of the connection are
  its own room's, and passes `netd` the channel's server end, the *write* end of the stream it reads and the *read* end of
  the stream it writes. `netd` checks the right `reach`, the scope (a name needs a name rule, an address an address rule
  and the port a port range), the three handle types (`channel`, `stream-write`, `stream-read`), replies `Connecting` on
  the scope channel at once and sends `Connected { address, port }`, `Failed { reason }` or `Reset { why }` on the
  connection channel. `Failed` and `Reset` are the last message; `netd` closes its stream ends after them. A client that
  closes the write end it keeps (`shutdown(write)`) makes `netd` send a FIN once the ring is dry; the peer's FIN is end of
  file on the inbound stream after the data; smoltcp keeps unread data across a reset, so a reader sees the data, then
  `Reset { Peer }`.
- **The copy.** `Conns::turn` is one turn of the single loop: frames in and timers, the resolver's answers, every
  connection, the frames those queued out. A connection copies with smoltcp's own closures (`send`/`recv` hand out the free
  space and the readable bytes) and non-blocking `stream_read`/`stream_write` inside them, so a byte is copied once in
  `netd`, with no staging buffer, and the backpressure is the rings' and the sockets': a full inbound ring stops reading
  the socket (the advertised window closes), an empty outbound ring or a full send buffer stops sending. At most 16
  rounds per direction per connection per turn. A stream end is subscribed on the port only while the connection waits
  for it (outbound readable after an empty read, inbound writable after a full write), so a ready stream nobody waits for
  does not wake the loop. 32 KiB socket buffers each way, at most 8 sockets and 16 connections in the process (a bound
  until the per-scope account of Q3), 10 s to connect per address, smoltcp's 60 s without an acknowledgement.
- **The resolver** (`zoraya_net::dns`; the decoder is `zoraya-dns`). `net.hosts` first (and exempt from the local rule),
  then a cache of 64 names kept for the answer's TTL up to an hour, then A queries over one UDP socket to the servers of
  `status` (`net.dns`, else the lease's), attempts at 0, 1, 2 and 4 s to the servers in turn, up to 8 queries at once,
  giving up at 8 s (`zoraya_proto_net::LOOKUP_US`) with `status::TIMED_OUT` (`reason::LOOKUP_TIMED_OUT` for a `connect`;
  proposed ADR-0316): "the name lookup timed out" in `ping` and `fetch`, `EAI_AGAIN` and `ETIMEDOUT` in `libposix`, apart
  from `Unresolvable` (no such name, no A record, a server that said it failed). Every client's wait derives from that
  bound (`ping_answer_us`, `CONNECT_ATTEMPT_US`) plus 5 s, so "the network service did not answer" means a `netd` that is
  gone or stuck. A query to a server whose MAC is unknown is not lost: smoltcp keeps it in the socket and sends it with
  the ARP reply. smoltcp asks for one neighbour per second, though, so the stack asks for the gateway and the DNS servers
  on the link itself (an ARP request it builds) when the interface is configured, and for the gateway when a lookup goes
  out and its last request is older than 30 s (`PROBE_FRESH_US`; smoltcp forgets a neighbour 60 s after its last frame):
  the connection or echo request that follows a first lookup does not wait that second. The answer to a `ping` by name
  comes inside a turn, after the stack's poll, so `Service::answer_lookups` makes the loop come back at once when its
  run starts there (before the fix of proposed ADR-0316 the loop slept on the delay of before the run, none at all, and
  the first `ping NAME` of a boot ended with the client's wait). A lookup asked while a DHCP lease is still to come
  (no `net.dns` and no lease, or the lease gone) is kept under its normal bound and goes out once the lease is there;
  `NoServer` (a static interface with no `net.dns`) is `status::UNAVAILABLE` and, for a `connect`, `reason::UNREACHABLE`.
  An answer counts only from a configured server's port 53, with an identifier and question of a query in
  flight; the decoder takes an address only from a record owned by the name or by a CNAME chain from it. An address that is
  loopback, link-local, multicast, unspecified, the interface's own, the gateway or a DNS server is refused with `Local`
  unless an address rule covers it (ADR-0223 item 4). Not built: AAAA, TCP for a truncated answer, negative caching, a
  `resolve` request. The identifier and the source port come from the clock and the MAC, which an off-path sender can
  guess (ADR-0227's service replaces them in N5).
- **`fetch`** (`user/src/bin/fetch.rs`, `zoraya-http`). `fetch [-o FILE] [--sum] URL`: `http://` only, GET, no redirects,
  a body of at most 64 MiB, a head of at most 8 KiB and 64 fields. The decoder is incremental and strict about framing
  (both lengths, two lengths, a length that is not digits, any transfer coding but `chunked`, a bare LF and folding are
  errors). The body of a non-2xx response is not printed; `--sum` prints the size, an FNV-1a checksum and the time.
- **The grant.** `zoraya-describe` has a new operand kind `url`. For a base-system command with such an operand, `zosh`
  makes the command's network connection with `narrow` (`zoraya_shell::netgrant::net_for`, host-tested): right `reach`, one
  rule for the URL's host (a name, or an address if dotted decimal) and port, TCP. As ADR-0224 item 2 says, only a
  *literal* operand designates; one from a variable, a substitution or a glob is refused before the line runs (`zosh: URL:
  from a value, outside the line's network grants`). **Not built:** the `+net` and `+listen` words (which would let a value
  designate), `grants` output for the scope (`grants fetch URL` prints no network rule), the REPORT message and the hint.
  Rooms and the panel's shells still have no `NET` slot, and `--net` still refuses `--gui`.
- **Tests.** `cargo test -p zoraya-dns` (16: the query, a decoded answer, CNAME chains, every prefix and byte change of a
  message, 100,000 random messages, pointer loops and forward pointers, name and length limits), `-p zoraya-http` (17: URLs,
  the request, every framing, every split of the input, the caps, 60,000 hostile inputs), `-p zoraya-net` (61: 21 for the
  connections over a scripted peer, with small rings and odd chunk sizes, FIN, reset (also after the peer's FIN), the linger
  limit, refusal, timeout, a client that stops reading, the socket limit, the resolver with forged and misdirected answers, and the local-address rule; one for
  `connect`'s checks), `-p zoraya-proto-net`. `make test-net` also runs `tests/shell/net-fetch.script`
  (`make test-net-fetch`) against `tools/test-httpd`, a bounded server on the host's 127.0.0.1 that the guest reaches at
  10.0.2.2: a page byte for byte, bodies of 1,000,000 (length), 300,000 (chunked, by name), 70,000 (until close) and
  4,000,000 bytes by checksum, a file written and read back, 404, a redirect, a cut connection, a refused port, SLIRP's DNS
  forwarder answering `localhost` (refused as local) and `nothing.invalid` (unresolvable), and bad URLs. `make
  test-net-lookup` (`tests/shell/net-lookup.script`, part of `make test-net`) boots the test image against
  `tools/test-dns`, a bounded DNS server on the host's 127.0.0.1 that answers late (3 s) or never: the boot's first network
  command is `ping` by name, then `fetch` by name, then the two silent names ("the name lookup timed out") and one that
  does not exist; its `netd` takes the server's port from `/config/net-test-dns-port` (feature `net-test` only; port 53
  needs root). `zoraya-test-dns --check-pcap` then reads the run's capture: one ARP request for the server before the
  first query, and the echo request and the SYN within 0.5 s of their answers. The step runs the
  network without `restrict=on` (`tools/run-qemu.sh --net-open`); every other step keeps it. The fuzz target
  `fuzz/fuzz_targets/net_dns_http.rs` was compiled and driven with 400,000 random inputs in a scratch crate (not run under
  libFuzzer: no nightly toolchain here).
- **Measured** (`fetch --sum` against `tools/test-httpd` on the host, QEMU 11.1.2, 128 MiB, 1 CPU, normal kernel,
  2026-11-11; 5 fetches of 16 MiB and 10 fetches of 1 byte per accelerator, medians): **HVF** 16 MiB in 232 ms, of which 19 ms
  is `fetch`'s own FNV hashing: 213 ms of transfer, 79 MB/s; all five between 218 and 238 ms. **TCG** 1463 ms, 40 ms of it
  hashing: 1423 ms, 11.8 MB/s; 1440 to 1498 ms. The page of one byte, connect to end of file (a new connection each time,
  `Connection: close`): HVF 27.7 ms (17.5 to 28.0), TCG 7.8 ms (6.4 to 26.1). The HVF figure is three times TCG's and is
  not explained: the guest's 10 ms timer tick or smoltcp's 10 ms ACK delay are the suspects (open item). The copies'
  share of the 16 MiB is **not measured**; two copies of it at the stream rate of "Stage 6 objects" (1.0 to 1.8 GB/s)
  would take 19 to 34 ms, 9 to 16 percent of 213 ms *(an estimate, not a measurement)*, and the frame slot is copied byte
  by byte with volatile reads (`Frames::read_slot` in `netd`), which is more than a bulk copy of the same bytes. The
  check on the way: 1,000,000 bytes 40 ms, 4,000,000 bytes 85 ms (HVF, in `make test-net`).
- **Idle connections.** smoltcp's timeout (60 s) ends a connection that is silent both ways with `Reset`; `netd` ends a
  half-closed one (one side's end of file seen) after 30 s without a byte with `Reset { Aborted }` and a connection whose
  socket was reset after the peer's FIN with `Reset { Peer }`, so a server cannot keep sockets by closing and going quiet.
  `fetch -o FILE` makes or empties FILE at the first byte of a 2xx page; a cut transfer leaves it partly written, exit 1.
- **Limits and what is not built.** One thread: a slow reader of a connection never blocks the others (its ring fills
  and its socket's window closes), but the 8 sockets and 16 connections are global, so one client can take all of them
  until Q3's per-scope account. No AAAA, no TCP fallback for DNS, no `resolve`, `datagram` or `listen` requests, no
  `peer`/`local`/`share` on a connection, no TLS (N5), no redirects, no `+net` words, no hint, no REPORT message (N3b). A
  reset after unread data delivers the data first. smoltcp does not say whether a timeout or a reset closed a socket, so
  `Reset { TimedOut }` is never sent.

## POSIX sockets (stage 8, N4: a POSIX program fetches a page)

`libposix` has the stream client's socket calls (`crates/zoraya-posix/src/{socket,net}.rs`, the C boundary in
`src/ffi/sockets.rs`; proposed ADR-0287 on top of ADR-0225). `zfetch` (`tests/posix-net/zfetch.c`, built by `zcc` against
`libc.a`) is `fetch` written with `getaddrinfo`, `socket`, `connect`, `send` and `recv`.

- **A socket fd** is a `Desc::Socket` of the descriptor table: the kind, a state (`Fresh`, `Connecting`, `Connected`, `Dead`),
  the options, and the three handles of ADR-0222's note, made by the library: the read end of the inbound stream, the
  write end of the outbound stream and the connection channel. `socket()` makes the object and asks nobody. `connect`
  makes the two streams and the channel, sends `netd` the three far ends, and waits for `Connected` or `Failed` on the
  channel (`SOCK_NONBLOCK`: `EINPROGRESS`, then `SO_ERROR` or `select` finishes it). `read`/`write`/`send`/`recv` are
  `stream_read`/`stream_write` with ADR-0101's blocking loop; the end of the inbound stream is `recv` returning 0, after
  a `Reset` has been given once as `ECONNRESET`; `shutdown(SHUT_WR)` closes the outbound end. `close` closes the three.
- **Names.** `getaddrinfo` answers a dotted quad itself (strictly: four decimals, no leading zeros) and otherwise sends
  one `resolve`; the addresses are remembered by name (64, the oldest goes first), and a `connect` to a remembered
  address is sent to `netd` as the *name*, so a scope of names works (INV-0225-2). Service names are a table of nine.
  `/etc/hosts` and `/etc/resolv.conf` are never read.
- **The grant.** The connection to `netd` reaches the program in the second bootstrap message beside `SELF`
  (`ZPBN`, two handles; `ZPBS` is the message without one). `zosh` makes it for a POSIX program the image describes
  (`user/describe/posix/zfetch.cmd`) when the line has a literal `url` operand, exactly as for `fetch` (`narrow`: one host and
  port). Without it `connect` is `EACCES` (the report and hint of ADR-0224 rule 5 are not built). A fixed slot was tried first
  and found wrong: a handle that arrives in a message takes the slot after the last one `spawn` filled.
- **Errors.** `connect`'s statuses and `Failed`'s reasons are `EACCES`, `ECONNREFUSED`, `ENETUNREACH`, `ETIMEDOUT`,
  `EHOSTUNREACH`, `ENOBUFS` (a connection or name bound of the account); a dead `netd` is `ENETDOWN`. `getaddrinfo` gives
  musl's `EAI_*` numbers, `EAI_SYSTEM` with `errno` for a refusal.
- **Surface.** `posix-surface-check` reads ADR-0287's block beside the tier list (it prints a note while the ADR is
  proposed): 22 names, `libc.a` has 315 exports against a list of 318. `socket` and `getaddrinfo` are off the check's deny list,
  as ADR-0225 item 1 says; ADR-0005's text is the architect's.
- **Tests.** `cargo test -p zoraya-posix`: 48 unit tests (11 for the socket module: addresses, `inet_aton`'s forms, the plan of
  `getaddrinfo`, the cache, the options, the errno rows) and 13 in `tests/net.rs` against a fake `netd` through the fake
  kernel (it counts request kinds: one `resolve` per name, the name in the `connect`, none for a literal; a page by address;
  every status and reason; non-blocking connect; a `Reset` after data; `shutdown`; handles back to the count; `fstat`,
  `lseek`, `fcntl`; unoffered calls; a dead `netd`). `make test-net-posix` (a step of `make test-net`): `zfetch` on the disk
  in the normal boot, a page byte for byte, bodies of 1,000,000, 70,000, 300,000 (by name) and 4,000,000 bytes by checksum
  against the server's own, 404, a refused port, a cut page, a value as URL (refused by the shell), and a run without the
  grant through `posixrun` (`Permission denied`).
- **Measured** (`zfetch --sum` against `tools/test-httpd`, the same boot as `fetch --sum`, QEMU 11.1.2, 128 MiB, 1 CPU,
  2026-11-12; 5 fetches of 16 MiB and 10 of one byte each, medians; both programs hash the body): **HVF** 16 MiB in 226 ms
  (218 to 233) for `zfetch`, 236 ms (230 to 240) for `fetch`; one byte, 23.1 ms (7.4 to 24.0) against 28.1 ms. **TCG**
  16 MiB 1,305 ms (1,299 to 1,354) against 1,380 ms (1,338 to 1,397); one byte 23.1 ms (6.6 to 32.3) against 20.5 ms (1.9 to
  29.7). The C path is not slower than the native one: the copies are the same (`stream_read` into the program's buffer),
  and `zfetch` hashes in C at -O2. The one-byte numbers vary by the 10 ms tick, as in N3.
- **Not built** (ADR-0287 item 9): `bind`, `listen`, `accept`, datagram sockets, `socketpair`, `poll`, `sendmsg`/`recvmsg`,
  `getnameinfo`, `gethostbyname`, `getservbyname`, `if_nametoindex` (`getrandom` since 9a); a socket does not cross `spawn` (`EBADF`) or
  `fork` (`ENOTSUP`) and a child has no network; `SO_RCVTIMEO` is accepted and ignored; `getsockname` gives `0.0.0.0:0`;
  curl and wget (W11, W13) were built in stage 9 (see "TLS for C programs, curl and wget"), which needed `select` and TLS.

## The clock, the random service and HTTPS (stage 8, N5)

`fetch https://HOST[:PORT]/PATH` fetches a page over TLS with the system's date, the system's randomness and the system's
roots (proposed ADR-0288 and ADR-0289 on top of ADR-0226, ADR-0227 and ADR-0228).

- **The clock** (ADR-0226). The kernel makes a `Device` for the PL031's page (`device::init_rtc`: one range, `intid` 0, so no
  interrupt can be made of it); `init` holds it at slot 5 and, each time it starts the storage stack, maps it, reads `RTC_DR`
  (whole seconds) and passes `--time-floor NANOSECONDS` to the root volume's `fsd`. `time_base(latest, monotonic, floor)` is
  `max(latest + 1, floor) - monotonic`. `ZNS1` (the native start-up record) has a 32-byte header with `realtime_base`;
  `zoraya_user::namespace::realtime()` is that base plus the monotonic clock; the base's lowest bit says the real-time clock gave the date, and `checked_realtime_seconds()` (what `fetch` gives TLS) is `None` without it, so a certificate is never checked against the volume's latest time alone; `date` prints it as `YYYY-MM-DD HH:MM:SS UTC`.
  Virtio devices now start at slot 7 (`FIRST_DEVICE_SLOT`); slot 6 holds the entropy seed. Under QEMU the guest's `date`
  agrees with the host's within 2 s (`tests/shell/clock.script`, the runner's `D` step).
- **The random service** (ADR-0227). The kernel copies `/chosen/rng-seed` into a memory object for `init`, which gives it to
  `rngd` (`user/src/bin/rngd.rs`; the logic is `zoraya-rng`, host tested). The generator is ChaCha20 with fast key erasure
  and an absorbing `mix` (a construction of ours; ADR-0288 item 5 asks the reviewer to decide on it). A connection speaks
  `Bytes(n)` (exactly `n` bytes, `n` up to 4,096, at once) and `Share(server end)`. The shell holds one and shares a fresh one
  with every native program it starts (the `random` slot of `ZNS1`); `random [COUNT]` prints bytes; `zoraya_user::random::random`
  is the program's call; `libposix`'s `getrandom` (9a, below) reads a connection the shell gives a POSIX program. **Not built:** the virtio-rng reseed, `/dev/urandom`, the connection of the
  panel's and the broker's programs, of a program `dash` or `make` starts and of a `fork` child, a restart of `rngd`.
- **TLS** (ADR-0228). `zoraya-tls` is a blocking client over rustls 0.23.46's unbuffered API with ring 0.17.14 as the provider,
  `no_std`: `Config::new(roots, now)`, `Connection::connect(config, name, transport, seed)` (32 fresh bytes from the random service are mixed into the generator first; `seed_random` refuses fewer than 32; the handshake and nothing else, so no
  application byte goes out before it succeeds), `read`, `write_all`, `close`. The date is the function the program gives (the
  wall clock; none, no handshake); ring's `getrandom` is a `zoraya-rng` generator the program seeds with 32 bytes from its
  random connection (`rngd` refuses a seed under 32 bytes or all zero and does not serve; only `init`'s and the shell's connections may `Share`, 64 live children each). `fetch` reads `/system/tls/roots`, which the shell adds (read only, "with the network") to the table of a
  program that gets `reach`, and verifies the host of the address, the name that was granted. `netd` sees ciphertext.
- **ring on `aarch64-unknown-none`.** N1's check built ring's objects but never linked: ring assembles its aarch64 assembly
  only for operating systems it lists, and `none` is not one. `vendor/ring` is the registry copy with that one line changed
  (`tools/vendor-ring.sh`), `[patch.crates-io]` selects it; `tools/ring-cc` (the host's clang, freestanding, three header stubs
  in `tools/ring-include`) and `target/debug/zoraya-ar` (the host's `ar` makes an empty archive of ELF objects) are set for the
  target in `.cargo/config.toml`; `make` builds the archiver first. No musl sysroot, Perl or NASM is needed. BearSSL, the
  recorded fallback, was not needed.
- **The roots** are Mozilla's list from the CCADB report (108 certificates with the Websites trust bit, no distrust date and no
  applied constraints; CDLA-Permissive-2.0 data; 2026-11-12) in `rootfs/system/tls/roots`. The path is a constant (the
  `tls.roots` setting of ADR-0229 is not built). The file is in the root volume, and `/system` is read-only at run time for everyone (the architect's decision, ADR-0289 item 8, ADR-0228's open questions): `fsd` seals the root's `system` entry on every connection, the console shell's included, so no write, create, remove, rename, publish or limit reaches it and no clone, narrowing or `+w` grant adds one; the shell refuses `+w /system...` with a hint. It changes only when the image is built (`zoraya-mkfs --from rootfs`); an explicit system-update path is a later ADR. A name-constraint test (`outside.pem` against the test root) shows the verifier refusing a name outside `.zoraya`. Every `.key` under `tests/net/tls` is public and for tests only (its README; name-constrained to `.zoraya`).
- **Tests.** `cargo test -p zoraya-rng` (10: RFC 8439's block vector, the generator, the service), `-p zoraya-tls` (9: a rustls server
  on loopback with the test PKI of `tests/net/tls`, 200 kB each way; expired, another name, a name outside the root's constraints, an unknown root, no date, a bad name,
  no roots, PEM; and no random without a seed), `-p zoraya-fs-service --test time` (the floor), `-p zoraya-dtb`, `-p zoraya-abi`.
  `make test-files` (`system-ro.script`: `/system/tls/roots` unchanged after the console shell's and a `+w /` grant's writes; `clock.script`: `date` twice, three `random`s that differ), `make test-net-fetch` (HTTPS: 1,000,000 bytes by
  checksum, the page, `expired.zoraya`, `wrong.zoraya`, by number, a plain server on an HTTPS address, `ftp://`; and the test
  server's log has no request line for the refused names). The test server runs three TLS terminators (`--tls-dir`) in front of
  its plain server, on ports it reports; this replaces the plan's `guestfwd` on 443.
- **Measured** (QEMU 11.1.2, HVF, 1 CPU, one run, 2026-11-12): `fetch --sum https://test.zoraya:PORT/pattern/1000000`: handshake
  51.1 ms (ECDSA P-256 certificate, 108 roots parsed, `user` profile at opt-level 1), then 1,000,000 bytes in 25.1 ms against
  38.8 ms for the same page over HTTP in the same boot.
- **Not built:** `https` for the POSIX `zfetch`, `tls.roots`, revocation, resumption (curl and wget came in 9a, below, over OpenSSL; no BearSSL port),
  the HTTPS lines under TCG, the GUI shells' roots entry (they have no network connection yet).

## TLS for C programs, curl and wget (stage 9, 9a)

`curl` and `wget` run inside Zoraya and fetch over HTTP and HTTPS (ADR-0310 on `libc.a`'s names, `getrandom` and the
volume, ADR-0311 on the `names` key and the roots, ADR-0312 on OpenSSL in place of ADR-0228 item 2's BearSSL; TASK-27 9a).

- **The ports.** `ports/sources.lock` pins OpenSSL 3.5.9 (the LTS line), zlib 1.3.1, curl 8.23.0 and wget 1.25.0. `make apps`
  (`ports/Makefile`, after `make fetch-ports`) builds them with `zcc` into `build/ports/deps` (the libraries) and
  `build/rootfs-apps/bin/{curl,wget}` (6.7 MiB each, static, no debug information). OpenSSL is `no-shared no-dso threads
  no-engine no-asm no-async no-sock no-dgram no-quic no-secure-memory no-ssl3 no-tls1 no-tls1_1 no-dtls`
  `--with-rand-seed=getrandom` (TLS 1.2 and 1.3 only; no `/dev/urandom` code at all), with four patches: `getrandom` is its
  seed source, no `posix_memalign`, the default certificate file is `/system/tls/roots`, and the library reads no environment
  variable and loads no `openssl.cnf`. curl is HTTP and HTTPS only with blocking name resolution (no threads), no proxy,
  cookies or netrc, and reads no `.curlrc` or trust variable (`CURL_CA_BUNDLE`, `SSL_CERT_FILE`, `SSL_CERT_DIR`); wget reads
  no `.wgetrc`, `WGETRC`, `SYSTEM_WGETRC` or `/etc/wgetrc`, has HSTS and proxies off, and refuses `--config` and `-e`
  (patches; INV-0310-4); wget is built with OpenSSL's plain-fd BIO (patch) and reads a byte at a time where it peeked (patch:
  libposix has no `MSG_PEEK`). wget's configure takes the host triple `aarch64-unknown-linux-musl` for gnulib's guesses only;
  the compiler is still `zcc`, which defines nothing of Linux (the end of `make apps` reads the logs for it).
- **`libc.a`** exports 337 names (22 more: `getrandom`, `gmtime`, `strerror_r`, `setbuf`, `tzset`, `strncat`, `rand`, `srand`,
  `fnmatch`, the `regcomp` family, `sigismember` and six `*_unlocked`). `ports/apps/zshim.c`, linked into `curl` and `wget`
  only, holds the names their sources reference and `libc.a` does not export: `select` (= `pselect`), refusals for `bind`,
  `listen`, `accept`, `link`, `setsid`, `setresuid`, `setresgid` and `tcsetattr`, a `clock_getres` that answers, a no-op
  `rewinddir` and two program-name variables.
- **`getrandom`** (`zoraya-posix/src/random.rs`): one `Bytes` request per 2 KiB on the program's connection to `rngd`, the whole
  of the buffer or an error; without the connection `ENOSYS`, never a weaker source; a `fork` child has none. The second
  bootstrap message has a form `ZPBX` plus a flag byte (net, random) with the handles in the order SELF, net, random
  (`zoraya_abi::posix`); `zosh` makes one `Share` of its own random connection per POSIX program (`posix_child::Launch::random`).
  A program a POSIX parent starts has none yet.
- **The grants.** `user/describe/posix/curl.cmd` and `wget.cmd` are the image's own descriptions: a literal `url` operand
  gives a connection that reaches that host and port (as for `zfetch`), and with it `/system/tls` read-only (a POSIX program is
  given directories only, and the directory holds the roots file alone: `make check-tls-roots`). `grants curl -O
  https://x.example/y` shows `create or replace y (option -O)` for the current directory (the descriptions say `cwd none`:
  the program is given that one name, as a directory handle narrowed to it, and nothing else of the directory; a line that
  designates nothing in it starts the program in `/bin`), `/system/tls  read (with the network)` and `net: x.example:443  reach (operand URL)`. The new description key `names
  OPTION OPERAND` (`zoraya-describe`, `url_file_name`) is how `-O` is designated: the shell takes the last segment of the URL's
  path, if it is plain letters, digits and `. - _ + ~ @ , =`, and refuses the line otherwise. `-k`, `--cacert`,
  `--no-check-certificate` and the rest that would weaken a check are not options of the descriptions: such a line is a
  usage mistake and the program gets no connection at all. `grants` shows the network line and the roots for POSIX programs now.
- **The volume.** `tools/run-disk.sh` adds `EXTRA_ROOT` (default `build/rootfs-apps` when it exists; `EXTRA_ROOT=` for none) to
  the volume of `make run`, `run-net` and `run-gui`, and puts its checksum in the disk's key: a new `curl` makes a new disk, and
  the old one is kept as `.old`. `RUN_NET_CMD` boots with 512 MiB (see the limits).
- **Tests.** `cargo test -p zoraya-posix --test random` (7: the buffer is the service's bytes in 2 KiB requests, no request for
  zero bytes, `ENOSYS` without the connection and the buffer untouched, `EINVAL` for an unknown flag, a short answer and a gone
  service are `EIO` with the buffer untouched, a fork child has none), `-p zoraya-describe` (the `names` key and
  `url_file_name`), `-p zoraya-shell` (`curl_o_*`), `make check-run-disk` (`tools/test-run-disk.sh`), `make test-apps`
  (`tests/shell/apps.script`: both programs against `tools/test-httpd` over HTTP and over TLS with the test PKI's root: a page,
  files by `-o` and `-O`, bodies of 70,000 to 1,000,000 bytes by size, 404 and a refused port with their exit
  statuses, an expired and a wrong-name certificate refused by both with the server's log showing no request byte, a plain
  server on an HTTPS address, by number, `-k` and `--no-check-certificate` as usage mistakes, `grants` for both, a refused
  `-O` name, a value as URL with and without `+net`, and wget without `-O` creating nothing; and in a QEMU run of its own,
  `tests/shell/apps-refusals.script`, every option of ADR-0321 refused with `+net` to the test servers, the expired
  and wrong-name ones' log showing no request byte), `make test-hand-run` (the
  production image: plain HTTP with both programs, HTTPS to the certificate and refused there, `grants curl -O`).
- **Known limits.** *Memory:* the leak found in 9a (each launch of `curl` or `wget` kept about 8.5 MiB: `posix_child::load`
  kept a duplicate of the image's memory object) is fixed (`work/9a-leak`); `tests/shell/apps-launches.script` (in `make test-apps`)
  runs `curl --version` 16 times on 128 MiB. `make run-net`, `run-gui` and the apps' tests still run on 512 MiB (the Makefile's
  comment gives the leak as the reason). *Other launchers:* a program started by `dash` or `make` has no random connection, so OpenSSL cannot seed.
  *Names:* `tls.roots` (ADR-0229) is not followed by C programs. *Protocols:* no HTTP/2, proxy, cookies, FTP or IPv6.
  *Threads:* OpenSSL is built with them (musl pthreads in libposix); curl and wget are single-threaded programs (synchronous resolver); `tests/apps/tlsthreads.c` runs handshakes from four threads.

## CPython (stage 9, 9c)

`python3` runs inside Zoraya (proposed ADR-0320): CPython 3.15.2 cross-built static with `zcc`, as a POSIX program from the
volume's `/bin/python3`, with its standard library in `/usr/lib/python3.15`.

- **The build.** `ports/sources.lock` pins the archive by the SHA-256 python.org publishes (the Sigstore bundle it also
  publishes is not checked). `make python` builds the host Python of the same version (`build/ports/hostpython`, minimal: `--without-ensurepip`, no zlib needed; for
  `--with-build-python` and for compiling the library's byte code), SQLite for the target (`THREADSAFE=1`, no extension
  loading, no WAL, FTS5), `libpyshim.a` (`ports/apps/pyshim.c`: `execv`, `madvise`, `msync`, `sched_yield`, `pause`, `dirfd`,
  `gethostbyname` over `getaddrinfo`, `getprotobyname`, `getservbyname`, `hstrerror`...) and CPython itself, into
  `build/rootfs-python` (`bin/python3`, 15.5 MB, and the trimmed library, 37 MB, byte code `unchecked-hash`; 52 MB, 1226 files).
  Built in: `_socket`, `select`, `_ssl` and `_hashlib` (OpenSSL), `_sqlite3`, `zlib`, `_asyncio`, `_thread`. No shared modules
  (no `dlopen`), `ctypes`, `readline`, `curses`, `tkinter`, `bz2`, `lzma`.
- **Patches** (`ports/patches/python/`): `_ssl` uses a file-descriptor BIO (OpenSSL has no sockets); the environment, byte code
  files and the per-user site directory are off; `asyncio`'s self-pipe is `os.pipe()` (no `socketpair`); `main` runs the interpreter
  on a thread with a 2 MiB stack (committed in full, copied by every `fork` child; measured 8, 2 and 1 MiB, ADR-0320 item 5) and ends the process with `exit(status)` from it, because a process starts on 64 KiB and CPython's overflow guard keeps 32 KiB (the main thread blocks every signal, so Ctrl-C reaches the interpreter's thread); no `pyvenv.cfg`
  search; `-c`, `-m` and a program read from standard input put no directory on `sys.path`; `urllib` takes no proxy from the environment. `configure` is told there is no `<sys/ioctl.h>` (libposix's `ioctl` serves terminals only), so `set_blocking` uses `fcntl`, and `_PYTHREAD_NAME_MAXLEN` is 15 (configure does not know the platform), so a long thread name is truncated, not refused.
- **The description** (`user/describe/posix/python3.cmd`): a POSIX program with the toolchain set; the network only with a
  `+net HOST:PORT` word, and the roots (`/system/tls/roots`) with it. `ssl.create_default_context()` finds the roots with no `cafile` (OpenSSL's `OPENSSLDIR` is `/system/tls`). `PYTHON*` variables decide nothing. A script imports from the standard library and its own directory (the current directory is readable), not from the current directory under `-c`, `-m` or a program read from standard input. Python is not mediated: a script can turn verification off (`ssl.CERT_NONE`), unlike curl's refused `-k`.
- **The volume.** `tools/run-disk.sh` takes several `EXTRA_ROOT` directories separated by colons and `DISK_SIZE` (default 64M, in
  the disk's key): curl, wget and Python do not fit in 64M, so `make run` with Python is
  `EXTRA_ROOT=build/rootfs-apps:build/rootfs-python DISK_SIZE=192M`, which also holds NetSurf (9d: `bin/netsurf` 12.5 MB and
  `usr/share/netsurf` in `build/rootfs-apps`, about 12 MiB in all, beside Python's 52 MB). `make test-python` makes its own 256M disk.
- **Tests** (`make test-python`, `tests/shell/python.script`): `-V`, `-c`, exit statuses, `tests/python/smoke.py` (flags,
  `sqlite3`, `ssl`, `asyncio`, a thread, `fork`), `grants` with and without `+net`, an `asyncio` HTTPS client
  (`tests/python/https_client.py`) against `tools/test-httpd`'s TLS server with the test PKI's root, the refusal of an expired and a
  wrong-name certificate with 0 request bytes at the server. `make test-python-regr` (`tests/shell/python-regr.script`):
  `tests/python/regr.py`, CPython's regrtest over the CI subset `tests/python/regr-modules.txt`; `make test-python-long` (by hand,
  not in `make test`): the modules too slow for a CI job under TCG, `tests/python/regr-long.txt` (`test_queue`, `test_threading`;
  ADR-0320 item 9). Each step makes its own volume.
- **Known limits.** `test_threading` (CPython's own) first failed with `OSError: Out of memory`: that was `os.fork` (no copy-on-write: a child copies the whole writable space, and each `python3` stack is committed in full: 11 children at once on 512 MiB with an 8 MiB stack, 15 with 2 MiB), not thread creation; it passes since the stack is 2 MiB, with the cases that start another interpreter left out, in the long list run by hand (ADR-0320 open questions; `test_thread` and `test_threading_local` in the CI subset, `test_queue` in the long list). The libposix table of 256 mappings (shared with the kernel's limit) ends a recursion deeper than about 33,000 frames with `MemoryError`, and, since each 1 MiB obmalloc arena is one mapping, a program holding about 250 MiB of small objects too (from the count; not measured); threads do not use it up (4000 threads, flat). A program that Python starts (`subprocess`, `os.system`) has no connection to the random service: another
  `python3` fails at start ("failed to get random numbers"), as for a program started by `dash` (9a). No `socketpair`, `AF_UNIX`,
  `bind`/`listen`/`accept` (a client only), `posix_spawn`; the interpreter's process has 2 MiB of stack, committed in full (no demand paging), and a `fork` child copies it. The shell prints a `hint` about `/dev/urandom` and `/system/tls/roots` after a script that touches them
  without the grants (harmless). Python's `ssl` defaults refuse a certificate without an Authority Key Identifier
  (`VERIFY_X509_STRICT`): the test client clears that flag alone, because the test PKI has none.

## NetSurf's text (stage 9, 9f-text)

[ADR-0326](adr/0326-netsurf-text-through-zoraya-text.md) (accepted; it supersedes ADR-0318's fonts item) replaces NetSurf's
internal bitmap font with `zoraya-text`: Russian and Ukrainian pages are readable, in the desktop's fonts, at the window's scale.

- **The library.** `crates/zoraya-fontset` (`no_std`, no `unsafe`): four faces, each a fallback list of the system's fonts (sans:
  Inter Regular then JetBrains Mono; sans bold: Inter Medium, Inter Regular, JetBrains Mono Bold; monospace and monospace bold
  from JetBrains Mono, then Inter); NetSurf's serif, cursive and fantasy take sans, weight 600 and up the bold face, italic is
  upright. Width, position and split keep NetSurf's meaning; U+200B to U+200F draw nothing; bytes that are not UTF-8 count one
  U+FFFD each. It reads the fonts only as `/system/fonts/NAME` for its four names (INV-0326-1).
- **Bounds a page cannot raise** (T3, T4 of the 9f-text review). A page chooses the size, up to `zoraya-text`'s 512 pixels,
  where one glyph holds about 100 KB of coverage. Glyphs above `CACHE_MAX_PX` (128 pixels) are not kept: each face has
  `LARGE_SLOTS` (2) slots for them. Below that, a face's cache that passes `CACHE_BYTES` (4 MiB of coverage) is emptied and
  starts again. The four faces' caches therefore stay under about 4 × (4 MiB + one 128-pixel glyph) plus 8 large glyphs, about
  17 MiB, whatever a page draws (`the_glyph_caches_stay_bounded_whatever_the_size`). Width shapes the text once, with no
  memory per character. Position and split shape only up to the first boundary past `x` (split also up to the next space), so
  NetSurf's layout, which calls `split` on the rest of a box for every line, is linear in the text: 20,000 Cyrillic words
  break into lines with about twice their characters shaped (`breaking_a_long_paragraph_into_lines_is_linear`), where shaping
  the whole rest on every call was quadratic (1.1 s for 300 lines on the host).
- **The C side.** `libc.a` exports `zoraya_text_init`, `_width`, `_position`, `_split`, `_metrics`, `_glyphs` and
  `zoraya_window_scale` (`crates/zoraya-posix/src/ffi/text.rs`, ADR-0326's block; `ports/netsurf/zoraya_text.h`).
  `ports/netsurf/font_zoraya.c` is NetSurf's font library (`NETSURF_FB_FONTLIB := zoraya`, `-DFB_USE_ZORAYA`, patch
  `0008`); glyphs are plotted with `nsfb_plot_glyph8`, which clips. At start it sets NetSurf's dpi to 90 times the window's
  scale, so a page in points or CSS pixels is twice the pixels at scale 2 and the same in logical pixels; the toolbar keeps its
  pixel size (its field's text converts at the same dpi).
- **The grant and the image.** A POSIX program whose description has `window` gets `/system/fonts` read only, "with the
  window" (`zoraya_shell::grants::with_window`), shown by `grants` and as a pill; a line's own `+r /system/fonts` and a room
  grant also give it, as they give any path (INV-0326-1's check covers only the implicit grant); `tools/run-disk.sh` copies `fonts/` into the
  disk's `/system/fonts` (in the disk's key: a person's disk from before 9f-text is kept as `.old` and a new one made).
- **Tests.** `cargo test -p zoraya-fontset` (the paths read, every Russian and Ukrainian letter a non-blank glyph that is not
  the box at the sizes of scale 1 and 2, widths, position, split, any bytes); `cargo test -p zoraya-shell` (the grant);
  `make test-netsurf`: the page `/ns/cyrillic` of `tools/test-httpd` at scale 1 (`tests/gui/netsurf.script`) and in a second
  boot at scale 2 (`tests/gui/netsurf-text.script`), with the runner's `ink` step (at least N pixels not the background) and a
  line of ten і that ends where a line of ten ж goes on (`docs/netsurf/netsurf-cyrillic.png`, `netsurf-cyrillic-2.png`).
- **Limits.** No italic face; no kerning (Inter's is in GPOS, ADR-0162); the toolbar is not scaled; at scale 3 the dpi is
  clamped to NetSurf's 250, so the page is 2.78 times, not 3 (the review's T8); the time text measuring
  takes per page is not measured (ADR-0318's 5% trigger); the native GUI programs still embed their fonts (ADR-0240).
  Whether `CLAUDE.md`'s licence rule names OFL-1.1 for unmodified font files is proposed ADR-0327.

## The network on the desktop, and when it fails (stage 8, N6)

`--gui --net` boots one machine with the desktop and the network (proposed ADR-0295, ADR-0296 and ADR-0297 on top of
ADR-0221 items 5, 6 and 8).

- **A shared page** (ADR-0221 item 8, the recommended option, as TASK-26 decided). With the GUI the four pages are the disk (31),
  the GPU (23), the keyboard (15) and the tablet (7); the network device is on transport 16, in page 2 with the GPU. Three
  calls: `device_transport(page, index)` (needs `MAP` on the page) gives the `Device` of one transport, made at boot, with
  no mapping and a handle without `MAP`; `device_read(device, offset, width)` (`READ`) and `device_write(device, offset, value,
  width)` (`WRITE`) make one access of 1, 2 or 4 bytes to a transport device (narrower than 4 only in the configuration, from
  0x100). The offset must be a multiple of the width with the access inside the transport's block (at most 0x200 bytes; a
  transport whose block would overlap the one before it is left out at boot); a page device has no register calls, a
  transport device has no mapping and no transports. Pins, quarantine, reset and the interrupt are per transport. The first
  write makes a process the transport's *holder* (kept in its address space, ended when its last handle that can write the
  transport is closed or moved, or with its exit): one holder at a time, none while the page is mapped, and the page is neither mapped nor released
  while a transport has a holder or a live pin, nor a transport released while its page is mapped (INV-0295-3). `init` maps each page once to read the identifiers (`Page::One` goes to its
  driver whole, as before); a `Page::Several` is never given: each transport is split with `device_transport`, identified by
  `device_read`, and given to its driver, which opens it with `zoraya_user::mmio::Window::open` (a mapping, or calls if the
  kernel answers `device_map` with `AccessDenied`): the four drivers use one `Registers` type. `tools/run-qemu.sh` accepts
  `--net` with `--gui`; `make test-gui` runs `tests/gui/net.script` (`net:auto:1`, `net:tcg:1` on CI): the terminal's
  `net status` shows the lease.
- **The restart reaches the shell** (ADR-0221 items 5 and 6). `init` had restarted `ethd` and `netd` together, at most three
  times, since N2. It now also gives the console shell (which is the first terminal window's shell) a *services channel*
  (slot 13) and sends it `NetworkRestarted` with a new root connection after a restart, `NetworkDown` after the third. The
  shell reads the channel before a line that needs the network runs; a closed root is `zosh: fetch: the network is not
  available (restarting)`, and without the `(restarting)` once `init` said so. Running programs see `PEER_CLOSED` on their
  streams and are not told; with the services channel and no root at all a line is refused too. The test
  (`tests/shell/net-fetch.script`, `make test-net-fetch`) uses a test-only request `Crash` that exists only in its own
  image: `build/zoraya-kernel-net-test.elf` embeds a ramdisk built with the `zoraya-user` feature `net-test` (target
  directories `target/user-net-test` and `target/net-test`); every other kernel's `netd` answers `Crash` as an unknown
  ordinal. In the test image `netd` honours it only if `init` started it with an event handle at slot 8 (when the volume
  holds `/etc/zoraya/net-crash`) and only on `init`'s control connection: `net crash` makes the shell ask `init` on the
  services channel, and `init` sends it. Three `net crash` with a fetch after each work; the fourth leaves the network
  down.
- **`+net HOST[:PORT]`** in the shell (ADR-0224 item 1, ADR-0297): `any`, a host name, `*.suffix`, an address or
  `ADDRESS/BITS`; a `url` operand that came from a value designates inside a word of the line; `grants` prints the
  `net:` lines.
- **Measured** (QEMU 11.1.2, HVF, one run, 2026-11-12). A register read by call: 1.39 to 1.42 microseconds (`t_devices`, 2,000 reads of the magic value, three
  boots), most of it QEMU's trap on the device access and the system call. `make bench-net` (5 runs, `fetch --sum`): 16 MiB in
  231 ms median (216 to 243 ms; the hashing takes 16 ms of it), 72 MB/s, as N3's 232 ms; one byte, connect to end of file, 27.2 ms
  median (14.4 to 27.3 ms). TCG not run locally (`FULL=1 make bench-net` runs it). The restart's duration (the plan asks for it) is not measured: the test waits 5 s after each `net crash`, and a fetch
  that ran with 3 s was once too early for the new lease.
- **Not built:** the rooms' `+net` and `+listen` words and the creator's-scope check, the pills and the hover card for a
  network rule, the `REPORT` message and the hint, the panel's and the broker's root connection and services channel (a
  shell started by the panel has no network), `make bench-net`, a `libposix` client's `ECONNRESET` after a kill, the
  network account flood and the fuzz target of the gate's second stage (`tools/untrusted-gate.txt`, still `pending`).

## Known limitations

- **The gate's tests (Q7, ADR-0298, proposed).** Every row of `tools/untrusted-gate.txt` now names a step, and the refusal of
  `room new --untrusted` is still in place. The tests play an untrusted room's client against the production service (an `fsd`
  account in `t_storage`, a compositor record with `untrusted` in `t_gui`, a `netd` account in `t_netd`) and run the system image's
  own programs in trusted rooms with limits (`tests/shell/disk-filler.script`, `hostile.script`, `tests/gui/spinners.script`); the
  keeper's restart is `make test-keeper` (a test-image-only `room crash-keeper`). Fuzzing is `make test-fuzz` and `make
  test-fuzz-net` (`tools/run-fuzz.sh`, nightly toolchain, 20 s locally, 600 s in CI). Not built: an untrusted room's `/tmp`, the broker
  and `tmpd` in the room's job and the untrusted line reader, so INV-0265-4, INV-0268-3 and the `/tmp` half of INV-0265-3 are not
  `checked`. INV-0264-3 is `checked` by `tests/gui/spinners.script` (`latency 30 +50 tcg +450`, accepted by the architect
  2026-11-13, ADR-0298 item 4): the median key to a terminal in `home` with two rooms at `--cpu 25%` and two spinners each is at most 50 ms
  over the same run's median with none under HVF (a local run; the runner reads the screen every 10 ms there, `LATENCY_POLL`), and at most
  450 ms under TCG (CI; the runner must read every 150 ms there, so a time is a screendump plus whole polls). The runner learns the
  accelerator from `tools/run-qemu.sh --print-accel`. Measured under HVF (M-series Mac, 512 MiB guest, eight passing runs, 30 keys each; a screendump
  read alone costs 4 to 7 ms): the median key to screendump 24.0 to 24.9 ms with nothing spinning, 22.8 to 23.1 ms with the spinners; over a
  screendump read 18.0 to 20.6 ms and 18.8 to 19.0 ms: the difference is -1.7 to +0.9 ms against the 50 ms bound (no defect). The 10 ms read makes a
  time a screendump plus at most 10 ms, so a difference of medians of 30 keys resolves a few ms. The first measurement also
  failed once because 30 backspaces were sent at once and 8 were lost, so the erase now paces them like the typing.
- **Masked interrupts.** Until stage 8's Q2, system calls ran with interrupts masked and some scaled with what they touch
  (under HVF on 128 MiB: `memory_create` of all memory about 9 ms, closing that object about 4 ms, closing a chain of
  20,000 channel ends about 11 ms). `memory_create`, `memory_map`, `memory_unmap`, `snapshot_process`, `spawn`'s copy,
  a chain's close and the teardown of an address space or a memory object are now in steps of 64 (ADR-0270). Still
  masked for their size: `dma_pin` (drivers only) and a device reset (up to 100 ms a transport). `schedule()` still
  drops a dead thread and its small remainder. A step point under a spinlock would unmask interrupts under it; only the
  test kernel checks that none is held.
- **Signal table.** `zoraya-events` keeps its peak size. A burst of channels leaves that memory in the kernel heap.
- **Handles in messages.** A protocol decoder checks how many handles a message has. A server must then check their
  types with `handle_type` (ADR-0072).
- **Snapshot stall.** `snapshot_process` now copies in steps of 64 pages with interrupts on between them (Q2); the
  figures here are of the copy as a whole: 0.4 ms for a 2.2 MiB process under HVF,
  about 16 ms for 9.2 MiB under TCG, more on a busy host ("Stage 6 kernel objects (as built)"). A mapped object that
  another process also maps may change between steps. Suspends do not nest, and a resume of a
  sub-job runs it while its parent stays suspended (ADR-0130 item 7). A stopped thread blocked in the kernel finishes
  its call before it parks. The stream ring is 64 KiB per stream, committed, with no quota (ADR-0056).
- **No untrusted code.** Nothing limits what a process may cost the system: no job has a quota. So no untrusted code
  runs in Zoraya until [ADR-0056](adr/0056-job-memory-accounting.md) is accepted and implemented. At stage 4 every
  process is trusted.
- **Console input.** Input is a fixed 256-byte ring (ADR-0060). A burst overruns it while the shell waits for the CPU,
  and the shell reports how many bytes went. Any holder of the console's `READ` right can take input from the shell;
  children get `WRITE` only.

- **Shell (TASK-18 Part 2).** A builtin or function that starts a pipeline runs to its end before the
  programs start: its output is held, up to the 1 MiB of a `$(...)`, and then fed (ADR-0152 rule 5 allows it to be
  written as it is made). Every byte of output is copied through `zosh` (ADR-0152's open question on the relay's
  cost). Keys typed to a program are cooked by `zosh`: cursor keys are dropped, and the line is sent at Enter. A test
  program that writes with `console_write` itself still reaches the serial console, not the window. `settings` publishes
  a new `/config/settings` (ADR-0157 item 4); `/config/history` is rewritten in place after every line. Descriptions are compiled into `zosh` as well as kept in each program and the
  ramdisk: no call reads a ramdisk entry (ADR-0156's open questions). A job brought to the front with `fg` after
  `&` or `bg` has its input at its end: only a job stopped in front gets the keys again. A line whose job Ctrl-Z
  stopped counts as interrupted (`$status`, and `try` catches it). Output written to a redirected file that fails
  is dropped without a message. A program's start-up record holds at most 64 entries (`native::MAX_ENTRIES`): a path
  covered with the same rights by another entry takes none, but a line naming more distinct paths than that is refused.
  A POSIX program has no terminal from `zosh` (ADR-0153): `isatty` is false. Native `cp`
  moves a file in one message, so it cannot copy a large program. Under the test runner, `run-qemu.sh --reboot` lets a reset start the guest
  again; without it QEMU ends on a reset (`-no-reboot`).
- **GUI.** The font is ASCII; everything else draws a box. One keyboard layout (US), one display, no
  resizing, no window decorations beyond a frame, no clipboard. F12 is reserved by the compositor. The compositor keeps
  a copy of every surface (up to 32 clients since 9e, ADR-0323, each up to the display's size), bounded only by those limits until
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
- **Threads.** No priorities, and kernel paths are not preempted. A process has up to 128 threads and the system 1,000 user
  threads (stage 9b, "Threads in a process"). `sleep_until(Infinite)`
  never returns. Sleepers share a deadline queue of 1,024 entries (stage 9b), so a full
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
  255 live processes, which it holds suspended (59 frames each with `hello`), and runs one at a time: a running `hello`
  takes a 1 MiB heap step, which 255 at once would not find in 128 MiB.
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
- **Ports.** toybox has no mount table, file notification or path-based extended attributes on Zoraya (its
  portability patch fails them); no T1 applet uses them. tcc's host check uses a tcc compiled for the host from the same source, not the Zoraya binary (ADR-0143). Programs are
  installed with their debug information.
- **CI.** The CI workflow (`.github/workflows/test.yml`) runs on Linux with TCG, as small parallel jobs (ADR-0213's
  Decision): one build job (`make fetch-ports`, `make ci-build`) hands `build/` and `target/` to the others, and every
  step of `make test` is a job of its own, `test-qemu` one a machine configuration and `test-posix` one a script, W8
  one a build. `make check-ci-jobs` fails if the workflow leaves one out. CI has no HVF, so `auto` is TCG there;
  `make test-posix-long` and `make measure-w4` run only where someone runs them.
  What a run contains is `tools/ci-plan.sh`'s plan: a merge candidate (tag `candidate/NAME`, a pull request) runs the steps its
  change touches (`tools/test-map.txt`) plus a smoke set; a push to `main` whose changes are all under `docs/` (except the docs a
  test or build reads, listed in `tools/plan-lib.sh`'s `docs_only`, and the `libc.a` surface lists and blocks) runs only the
  document checks (`doc_checks`: `adr-check`, `check-host-paths`, `check-untrusted-gate`, `check-quota-tests`,
  `check-tls-roots`), no build and no machine; every other push to `main`, the nightly run and `candidate-full/` run the full
  set (`tools/ci-tests.txt`). `make check-ci-plan` tests the plan. The whole-suite TCG steps have `QEMU_SLOW_TIMEOUT` (420 s).
  The jobs run in an image (`.github/ci/Dockerfile`, built by `ci-image.yml`, tagged by `tools/ci-image-tag.sh`) so
  that no job needs Ubuntu's mirrors; without the image they install their tools with `tools/ci-apt-install.sh`
  (retries, then other mirrors). Details: `docs/tasks/TASK-26-report.md`, "CI image".
- **Stage 6 acceptance (stage 6e).** One CPU: `-j` gives no speed. Every `exec` and `posix_spawn` reads the whole
  program file, debug information included (ADR-0212). A process ended by a signal reports no statistics. W9's
  interruption point is wall-clock on the host, so it differs with the accelerator (ADR-0110). SQLite's run-time
  extensions are compiled out (`-DSQLITE_OMIT_LOAD_EXTENSION`, ADR-0210: no `dlopen`). T2's applets are not built, so
  GNU hello's `configure` does not get far (`build/w4-report.md`). `getrusage` gives zeros and `clock` elapsed time;
  `fsync` makes nothing durable beyond `O_SYNC` (ADR-0210).
- **`libposix` (stage 6c).** A file read or write through a bad pointer faults the process instead of `EFAULT` (streams give
  `EFAULT`); listings have no `.` or `..` (`d_ino` is the object's id, as `st_ino`); `O_TRUNC` of an existing non-empty file is a
  second request; `st_blocks` comes from the size; times before 1970 cannot be set (ADR-0180).
- **Processes and the terminal (stage 6d).** A blocked signal whose default terminates still ends the process
  (ADR-0106 item 2); the mask itself is carried across `exec` and `posix_spawn` (ADR-0190's Decision). A stopped
  child sends no `SIGCHLD` (ADR-0190 item 19). `pselect` reports a terminal ready at once (`poll_readable` is deferred, ADR-0105). Non-canonical
  mode exists but nothing sets it; no `TOSTOP`, no `SIGTTOU`. The shell's fds 0 to 2 are three descriptions (ADR-0190
  item 7). A FIFO fd passed by `spawn` is not counted as a side by `tmpd` (item 12), and a FIFO opened after its
  writers left gets a fresh pair (ADR-0107 item 15). `execl` takes six arguments after `arg0` (item 13). `fork`
  copies eagerly with interrupts masked (ADR-0102). Nothing bounds a fork bomb but the kernel's 255 processes and
  memory (ADR-0056). The broker trusts what a process says of its own pid (ADR-0104, "Trust at stage 6"). A POSIX
  session and `zosh` do not share the console; from `zosh` a POSIX program has no terminal. Dates are the
  root volume's (ADR-0191), not a clock's. The GUI terminal holds up to 4 KiB of keys a slow `ttyd` or `zosh` has not
  read yet (TASK-24 7a, which fixed the loss of a burst's tail under TCG); past that, keys are dropped and counted.
- **A shared virtio page** (ADR-0295, N6). The register calls give each driver of a shared page only its own transport; a
  mapping of the page and a transport's holder exclude each other, but a process that maps the page (only `init`, once, to read
  the identifiers) sees every transport while it does. A page device's `dma_release_quarantine` resets every transport of the
  page, and holds the page's lock across each transport's reset (up to 100 ms), so another transport's calls on the page
  wait behind it. A pin through a transport is allowed while the page is mapped or another process holds the transport (a
  pin starts nothing); a live pin refuses the mapping and the releases. Reading a transport needs
  no hold, so another holder of a transport handle can read its registers (a read restarts nothing). A register read or write on
  a shared page costs a system call (1.4 microseconds under HVF).
- **The network on the desktop** (N6). A shell started by the panel has no network connection and no services channel (only the
  console shell, which is also the first terminal window's shell, has them), so the network restart reaches no other launcher; a
  window cannot be given `+net` words and no pill shows one (the rooms' words are not built).

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
| 7 (done) | design system, text rendering, window management, keyboard layouts, scaling, toolkit, chooser, panel, file manager, editor, settings, clipboard, list settings, rooms (ADR-0160 to ADR-0172) | a person opens a file from the file manager in the editor through the chooser, in a window the compositor framed, with Ukrainian and Czech names on screen, at scale 1 and 2 (`make test-gui`) |
| 8 | networking: the virtio-net driver, `netd` on smoltcp, the scoped network capability and `+net` grants, name resolution, POSIX sockets, the real clock, the random service, TLS, `net.*` settings, tests without the Internet (ADR-0220 to ADR-0230; the network on the desktop waits for ADR-0221 item 8) | `+net test.zoraya:443 curl -O https://test.zoraya/w12.bin` from `zosh` fetches the file from the runner's server over HTTPS with a validated certificate, and `fetch http://other.example/` without a grant prints the `+net` hint (`make test-net`) |
| Q (quotas) | budgets on jobs (memory, processes), the CPU ceiling, accounts in `fsd`, the broker, `netd` and the compositor, directory limits in `fsd`, the room keeper, the room line's limit words, `top`, bounded kernel work, untrusted rooms enabled (ADR-0260 to ADR-0270) | `room new agent-1 --untrusted +w ~/sandbox/agent-1` creates a room in which `:(){ :|:& };:`, a memory hog, 64 spinners and `dd` into `~/sandbox/agent-1` each stop at the room's limits while a `home` terminal stays responsive (`make test-quotas`, the gate of ADR-0268) |

## Where things are

```
CLAUDE.md                 rules for agents
.claude/agents/           the test-runner and mutant-triage subagents
LICENSE                   BSD-2-Clause, except tools/posix-surface/patches/ (its README.md), ports/patches/ and ports/musl/zoraya/ (ports/README.md)
Makefile                  build, run, run-gui, debug, test-unit, test-qemu, test-boot, test-files, test-libposix, test-posix, test-posix-gui, test-tour-gui, test-crash, test-gui, test, adr-check,
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
  zoraya-resolve/         the one path resolver (ADR-0107 items 6 and 7, ADR-0153 rule 6): libposix's path, the native
                          runtime's and zosh's
  zoraya-commands/        the file commands' logic over a Files trait: the shell's builtins now, the programs later
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
  zoraya-virtio-net/, zoraya-proto-nic/  the network driver's logic and the frame protocol with ARP (ADR-0221)
  zoraya-net/, zoraya-proto-net/  the network service's logic over smoltcp, and its protocol (ADR-0221, ADR-0229)
  zoraya-rooms/, zoraya-proto-rooms/  the room keeper's decisions over a kernel trait, and its protocol (ADR-0269, ADR-0293)
  zoraya-proto-display/, zoraya-proto-input/, zoraya-proto-gui/, zoraya-proto-term/  the GUI's protocols
  zoraya-proto-posix/, zoraya-proto-tty/  the broker's and the terminal's protocols (ADR-0104, ADR-0105)
  zoraya-broker/          the broker's logic: pids, groups, kill, the session file
  zoraya-tty/             the terminal's logic: line discipline, connections, foreground
  zoraya-keymap/          key codes to text, the US layout (ADR-0092)
  zoraya-tokens/          the design tokens from tokens.txt, read at compile time (ADR-0161)
  zoraya-text/            TrueType reader, fixed-point rasteriser, glyph cache, simple shaper (ADR-0162)
  zoraya-fonts/           the system fonts of fonts/ as bytes (ADR-0162, ADR-0240)
  zoraya-fontset/         the system's fonts for NetSurf: faces from /system/fonts, width, position, split, glyphs (ADR-0326)
  zoraya-paint/           the compositor's drawing in integers: corners, shadows, blur, wallpaper, the material
  zoraya-term/            the terminal: VT-subset parser, grid, scrollback, keys to bytes, rendering and the exact
                          reader of its cells (ADR-0096, ADR-0162)
  zoraya-compositor/      the compositor's loop over a platform trait (ADR-0093, ADR-0094); windows, frames,
                          gestures, chords, the switcher, the bar (ADR-0160, ADR-0164; window.rs, draw.rs)
  zoraya-posix/           the Rust half of libposix (ADR-0100): resolver, fd table, errno, the file functions; ffi.rs
                          is the C boundary in libc.a; tests/kernel is a fake kernel over the real fs-service
user/                     the programs in the ramdisk, on zoraya-rt: init, zosh, blkd, fsd, gpud, inputd, ethd, netd, net, compd,
                          term, posixrun (the launcher of C programs, stage 6c), broker and ttyd (the POSIX
                          session, stage 6d), test programs
  src/stack.rs            starts the driver and the service
  src/gui.rs              scans device pages, starts and restarts the GUI (init and t_gui)
  src/command.rs          a described program's start: its arguments through zoraya-command (INV-0155-2)
  describe/               command descriptions (ADR-0156): builtins/, commands/, demos/, topics/, system/;
                          build.rs refuses a bad one, describe! puts one in its program's ELF
rootfs/                   the first contents of build/disk.img
fonts/                    the system fonts, unmodified, with their licences (docs/dependencies.md, Fonts)
tests/shell/              scripts the runner plays on the serial line
tests/gui/                the GUI script the runner plays over QMP
tests/posix-core/         stage 6c's C criterion program and the script make test-libposix plays
tests/posix/              the POSIX session: its root (/etc/zoraya/session, /etc/passwd, /home, /tests/wN.sh,
                          /sets/t1p), trees.sh, t_posix.c, one runner script per workload, and
                          the scripts make test-posix, make test-posix-gui and make test-tour-gui play
tools/
  run-qemu.sh             the only place that knows the QEMU command line
  dump-dtb-fixtures.sh    regenerates the zoraya-dtb fixtures
  test-runner/            zoraya-test-runner: runs the test kernel, checks the protocol, plays serial and GUI
                          scripts (serial text with SGR sequences dropped); zoraya-crash-test
  test-summary.sh         make test's summary, with the logs in build/logs/
  test-dns/               zoraya-test-dns: make test-net-lookup's DNS server on 127.0.0.1 (late or no answers)
                          and the check of the step's capture
  mutants/                zoraya-mutants and its mutation lists (*.toml)
  adr-check/              zoraya-adr-check: ADR-0001 invariants
  mkramdisk/              zoraya-mkramdisk: packs user programs, and their descriptions as NAME.cmd
  zoraya-mkfs/            creates a filesystem image from a host directory
  zoraya-fsck/            checks (and on request repairs) a filesystem image
  posix-surface/          TASK-02/02b measurement pipeline (unchanged)
  zcc                     the C compiler driver for aarch64-unknown-zoraya (ADR-0109)
  fetch-ports.sh          downloads and verifies the ports' archives (make fetch-ports)
  zoraya-ar/              the archiver: GNU ar with a symbol index
  posix-surface-check/    libc.a against the tier list, the svc scan, the hiding of other names, the call log's
                          wrappers (calllog)
  posix-stats/            the stage 6 table and the W4 report from the runner's logs, and the call log's union
                          against the tier list (--calls)
  zoraya-elf-check/       zoraya-elf over programs on the host (INV-0109-1)
  footprint/              a process's memory for the watchdog (macOS's physical footprint), and memhog
ports/                    stage 6b: sources.lock, the build of the sysroot and the ports, patches (ports/README.md)
fuzz/                     cargo-fuzz targets for zoraya-fs and the shell's parsers (nightly; outside the workspace)
build/                    kernel ELFs, disk images, logs/ and mutants/; ports/, sysroot/, rootfs-posix/ (not committed)
```

`libposix`'s file, descriptor, pipe and memory half exists (stage 6c); its process half and the broker arrive in 6d.
