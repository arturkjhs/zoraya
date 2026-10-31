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
- **External crates:** none ([`dependencies.md`](dependencies.md)).
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

**Filesystem** ([ADR-0010](adr/0010-filesystem-client-guarantees.md) to
[ADR-0015](adr/0015-block-device-contract.md)). Accepted, not yet built; the reasons are in
[`adr/filesystem-batch-selection.md`](adr/filesystem-batch-selection.md).
- **Guarantees.** After a crash a volume holds a prefix of the completion order. Requests are atomic, durability comes
  from `sync`, and a storage error stops the volume's writes for good.
- **Mechanism.** A copy-on-write tree with dual root records. Mount picks the newest valid root, with no replay.
- **Integrity.** CRC32C of every block in its parent pointer, a total decoder, and one service per volume.
- **Names.** UTF-8 compared byte for byte, no hard links, no `..` in the server, symbolic links as inert objects.
- **Metadata.** `mtime`, `ctime` and an advisory mode; times strictly increase on a volume.
- **Device.** The virtio contract as written: no write is atomic, and flush is the only barrier.

## Boot (stage 0, as built)

How the kernel starts. The code is in `kernel/src/arch/aarch64/boot.rs`, `kernel/src/dtb.rs` and `kernel/linker.ld`.

- **Loading.**
  - `tools/run-qemu.sh` passes the kernel ELF with `-kernel`. QEMU treats an ELF as a bare-metal image: it loads the
    segments at their link addresses and starts CPU 0 at the ELF entry.
  - The image is linked at `0x40200000`, which is RAM base plus 2 MiB. QEMU puts the device tree blob at the start
    of RAM (`0x40000000`; 1 MiB on QEMU 11.1.2), but only if it fits below the image. Otherwise QEMU silently omits
    the blob and the kernel can only report its absence through the fallback UART. `linker.ld` asserts the 2 MiB gap.
  - The MMU is off at stage 0, so all addresses are physical.
- **Device tree address.** The kernel accepts an address in `x0` if it lies in RAM below the image (Linux boot
  protocol style). Otherwise it reads the blob at the start of RAM.
  - Under QEMU 11.1.2, `x0` is `0` for an ELF, under both HVF and TCG. The blob is therefore always found at the
    start of RAM. Every boot banner prints this.
  - The blob is validated by `zoraya-dtb` before use.
- **Exception level.** The kernel starts at EL1, or at EL2 with `-machine virt,virtualization=on`, which works under both TCG and, on the M4 with QEMU 11.1.2, HVF. At EL2 it
  writes a minimal configuration and `eret`s to EL1h; the configuration is documented in the header of `boot.rs`:
  - EL1 is AArch64 with no stage-2 translation, and nothing stage 0 does is trapped. SVE and SME stay trapped, and
    `MDCR_EL2` (debug and PMU traps) is left at its reset value; stage 1 must set it before using either;
  - `SCTLR_EL1` is set before the `eret`;
  - EL1 may use the physical timer and counter;
  - FP/SIMD is not trapped;
  - the GIC system-register interface is enabled for EL1 if present.

  The tests check that the kernel runs at EL1 after boot in both cases.
- **Secondary CPUs.** With PSCI emulated, QEMU starts only CPU 0 and keeps the others powered off until `CPU_ON`. As
  a safeguard, boot code also parks (`wfe`) any CPU whose MPIDR affinity is not 0.
- **Before Rust.** The boot code then:
  - sets `SCTLR_EL1` to a known value (MMU and caches off);
  - enables FP/SIMD (the target lets the compiler use NEON);
  - installs a catch-all vector table that turns any exception into a panic;
  - sets up a 64 KiB stack and clears `.bss`.
- **Console.** The PL011 is the node named by `/chosen/stdout-path`, or failing that the first enabled `arm,pl011`
  node. `0x09000000` is used only to print a device tree error before halting.
- **Banner.** It shows the version (`Cargo.toml`) and git commit (embedded by `kernel/build.rs`), the entry and
  current exception level, the boot CPU, the CPU count, the interrupt controller, PSCI and the memory map.
- **Shutdown.** PSCI `SYSTEM_OFF`, through the conduit (`hvc` or `smc`) named by the device tree's PSCI node.
- **Panic.** The handler prints `ZORAYA-PANIC at file:line:col: message` on one line, then halts.

### Tests

`make test` runs:
- `cargo fmt --check`;
- clippy with warnings as errors (host crates, kernel, test kernel);
- the host tests;
- `make adr-check`;
- the in-QEMU tests in six machine configurations:

| Label | Accelerator | Machine | Checks beyond the test list |
|---|---|---|---|
| auto-default | HVF on a Mac, else TCG | default | entry at EL1 |
| auto-smp4-2g | HVF on a Mac, else TCG | 4 CPUs, 2 GiB | `cpus: 4`, 2048 MiB region |
| auto-el2 | HVF on a Mac, else TCG | `virtualization=on` | entry at EL2 |
| tcg-default | TCG | default | GICv2 |
| tcg-el2 | TCG | `virtualization=on` | entry at EL2, PSCI via `smc` |
| tcg-smp4-gicv3 | TCG | 4 CPUs, 2 GiB, `gic-version=3` | `cpus: 4`, GICv3 |

How the in-QEMU tests run and report:
- The test kernel (feature `qemu-test`) prints the `ZORAYA-TEST` protocol, described in `kernel/src/testing.rs`. A
  `run <name>` line precedes each test, so a test that hangs is named in the timeout report.
- A panicking test is reported as that test's failure, and the run continues on a fresh stack.
- Two harness self-tests fail on purpose: one panic, and one undefined instruction caught by the vectors. The runner
  requires both to be reported as panics.
- `tools/test-runner` applies a timeout, and fails on a missing `begin` or `end` line, a count mismatch, a panic
  outside a test, a kernel that does not power off, or a non-zero QEMU status. The guest's exit status is not used as
  a signal.

`zoraya-dtb` is tested against device trees dumped from QEMU (`crates/zoraya-dtb/tests/fixtures/`, with the exact
commands) and against malformed input: every truncation, bad header fields, structure and string errors, and 30,000
deterministic random mutations.

### Known limitations of stage 0

- **No MMU, interrupts or timer.** Memory is Device-nGnRnE, so the code avoids unaligned accesses and atomic
  read-modify-write.
- **QEMU `virt` assumptions.** The RAM base `0x40000000` (where the device tree and image are found) and the fallback
  UART address are QEMU `virt` facts.
- **UART set-up.** The PL011 baud rate is not programmed; QEMU ignores it.
- **No stack guard.** The 64 KiB boot stack sits directly above `.bss`, and with the MMU off there is no guard page.
  The review measured a high-water mark of about 27 KiB (normal kernel) and 30 KiB (test kernel). A canary at the
  bottom of the stack is checked at shutdown (warning) and by the last in-QEMU test.
- **Exception recovery in tests.** A test that fails through the catch-all vector is recovered without `eret`, so the
  remaining tests run in exception context. That is harmless at stage 0, and must change when the vectors become real.
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
Makefile                  build, run, debug, test-unit, test-qemu, test, adr-check
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
  linker.ld               image layout (physical, 0x40200000)
  build.rs                linker script, git commit
  src/arch/aarch64/       entry, EL2 -> EL1, vectors, system registers
  src/dtb.rs              locating the device tree blob
  src/console.rs, pl011.rs  UART console
  src/psci.rs             SYSTEM_OFF
  src/panic.rs            panic handler
  src/testing.rs          in-QEMU tests (feature qemu-test)
crates/
  zoraya-dtb/             device tree parser and platform extraction; no_std, host-tested, fixtures from QEMU
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
