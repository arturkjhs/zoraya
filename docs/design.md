# Zoraya design summary

What a new agent needs in order to understand the system in ten minutes. This page states what has been decided and
what exists. The decisions themselves, with their evidence and rejected options, are the ADRs in [`adr/`](adr/index.md).
The rules for working here are in [`../CLAUDE.md`](../CLAUDE.md).

## Vision (architect)

TODO(architect): what is new in Zoraya, in the architect's words.

## Platform

- **Architecture:** aarch64 (ARMv8-A).
- **Machine:** QEMU `virt`. Under HVF on an Apple Silicon Mac (the development host, an M4) the CPU is `host`. Under
  TCG elsewhere it is `cortex-a72`. Real hardware is out of scope.
- **Language:** Rust, `no_std`, target `aarch64-unknown-none`. Stable toolchain 1.99.1, pinned in
  `rust-toolchain.toml`. No nightly features are used.
- **External crates:** none ([`dependencies.md`](dependencies.md)). The kernel uses `core` and `alloc`.
- **Native interfaces are Zoraya's own design.** POSIX exists only as a compatibility personality on top of them
  (ADR-0002).

## Architecture as decided

ADR-0001 to ADR-0006 were accepted as recommended on 2026-10-25. [ADR-0001](adr/0001-decision-process.md) sets the process:
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
- **Exit records.** A process object carries `ExitRecord { code, reason, detail }`.
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

**Proposed, not yet decided.** Stage 1 is built on ADR-0020 (address space), ADR-0021 (physical memory), ADR-0022
(heap), ADR-0023 (interrupts and time) and ADR-0024 (kernel fault policy). They are `proposed` until the architect
reviews the branch. The code follows their recommendations, and the sections below describe it.

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
- **Exception level.** The kernel starts at EL1, or at EL2 with `-machine virt,virtualization=on`, which works under both TCG and, on the M4 with QEMU 11.1.2, HVF. At EL2 it
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
- **EL0 vectors** exist and panic with a message saying they are not wired up until stage 2.
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
  rather than spin. `sync.rs` says what changes for SMP. Lock order: heap, then kernel tables, then frames.

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

## Tests

`make test` runs:
- `cargo fmt --check`;
- clippy with warnings as errors (host crates, kernel, test kernel);
- the host tests;
- `make adr-check`;
- the in-QEMU tests in seven machine configurations;
- `make test-boot`, which builds the normal kernel and boots it under each accelerator. It must print its heap and
  timer checks, not panic, and power off.

| Label | Accelerator | Machine | Checks beyond the test list |
|---|---|---|---|
| auto-default | HVF on a Mac, else TCG | default | entry at EL1 |
| auto-smp4-2g | HVF on a Mac, else TCG | 4 CPUs, 2 GiB | `cpus: 4`, 2048 MiB region |
| auto-el2 | HVF on a Mac, else TCG | `virtualization=on` | entry at EL2 |
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
- Tests inspect a fault with `arch::catch`, which runs a function and returns the fault that ended it. That path
  exists only in the test kernel: the normal kernel cannot resume a fault.
- Seven tests fail on purpose (`--expect-panic`): the harness self-tests, and the fault reports for `brk`, an unmapped
  access, an unexpected interrupt, an EL0 exception and a stack overflow. `--expect-line` checks what the reports say.
- `tools/test-runner` applies a timeout, and fails on a missing `begin` or `end` line, a count mismatch, a panic
  outside a test, a kernel that does not power off, or a non-zero QEMU status. The guest's exit status is not used as
  a signal.

The timer test allows a shortfall of 20 % of the ticks over 200 ms, because the host can deschedule the vCPU (HVF) or
the TCG thread, and ticks then arrive late. Too many ticks always fails: no deadline fires early.

Host tests: `zoraya-pmm`, `zoraya-heap`, `zoraya-pagetable` and `zoraya-time` each have a model test with a seeded
generator, so there is no property-testing dependency. `zoraya-pagetable` decodes descriptors bit by bit with a walker
that shares no code with the crate.

`zoraya-dtb` is tested against device trees dumped from QEMU (`crates/zoraya-dtb/tests/fixtures/`, with the exact
commands) and against malformed input: every truncation, bad header fields, structure and string errors, and 30,000
deterministic random mutations.

## Known limitations

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
- **Interrupts.** One priority, no nesting. FIQ and SError reports are untested, since nothing raises them.
  Interrupt handlers must not panic in a test that expects recovery: the interrupt would stay active.
- **Emergency stack.** A second overflow, on the emergency stack, halts silently. The deepest use measured is the
  stack overflow report: 5,552 of 32,768 bytes.
- **Faults while a lock is held.** Recovery in the test kernel does not release locks, so a test that panics while
  holding one makes later tests fail. The normal kernel halts on panic.
- **Single core.** Only the boot CPU runs. The GIC, timer and lock are set up for it alone. Before SMP: on GICv2,
  `gic::acknowledge` must keep the source bits of an SGI for the EOI and mask them from the ID.
- **HVF limits.** HVF does not support GICv2 (`gic-version=2` fails to start). EL2 entry under HVF
  (`virtualization=on`) needs HVF nested virtualisation: an M3 or later and a recent macOS. It works on the M4 with
  QEMU 11.1.2 and is part of `make test`, so `make test` as written fails on an M1 or M2 Mac (`auto-el2`).
- **CI not run.** The CI workflow (`.github/workflows/test.yml`) runs `make test` on Linux with TCG, and has not run
  yet.

## Roadmap

| Stage | Content | Done when |
|---|---|---|
| **0 (current)** | Boot, UART output, panic handler, device tree parsing, test harness | `make test` passes |
| 1 | Exception vectors, GIC, timer, MMU (kernel in upper half), physical frame allocator, kernel heap | timer ticks; `Box`/`Vec` work in the kernel |
| 2 | Threads, context switch, scheduler, EL0, first native syscalls, ELF loader, initial ramdisk | a user process prints through a native syscall |
| 3 | Handles, ports and process objects per ADR-0003/0004/0006; native `spawn` | a process spawns a child and waits on its exit via a port |
| 4 | virtio-blk, filesystem service, `mkfs` on host | files survive reboot |
| 5 | virtio-gpu, virtio-input, compositor, terminal as GUI client | screenshot shows a terminal window |
| 6 | `libposix` T1 per ADR-0005, port of tcc, make, dash | `tcc hello.c && ./a.out` inside Zoraya |

## Where things are

```
CLAUDE.md                 rules for agents
Makefile                  build, run, debug, test-unit, test-qemu, test-boot, test, adr-check
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
  src/psci.rs             SYSTEM_OFF
  src/panic.rs            panic handler
  src/testing.rs          in-QEMU tests (feature qemu-test)
crates/                   no_std, host-tested
  zoraya-dtb/             device tree parser and platform extraction; fixtures from QEMU
  zoraya-pmm/             physical frame allocator and range sets
  zoraya-heap/            kernel heap logic
  zoraya-pagetable/       translation table building and editing
  zoraya-time/            clock arithmetic and the deadline queue
tools/
  run-qemu.sh             the only place that knows the QEMU command line
  dump-dtb-fixtures.sh    regenerates the zoraya-dtb fixtures
  test-runner/            zoraya-test-runner: runs the test kernel, checks the protocol
  adr-check/              zoraya-adr-check: ADR-0001 invariants
  posix-surface/          TASK-02/02b measurement pipeline (unchanged)
build/                    kernel ELFs copied by make (not committed)
```

The ADRs mention an `abi` crate, `libposix`, the broker and services. None of these exists yet; they arrive in
stages 2 to 6.
