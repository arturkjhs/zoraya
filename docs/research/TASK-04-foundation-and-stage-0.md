# TASK-04: Project foundation and kernel stage 0

## Context

Zoraya OS is a new operating system: Rust `no_std` kernel, target aarch64, running under QEMU `virt` on an Apple Silicon Mac (M4). Native interfaces are our own design; POSIX exists only as a compatibility personality.

The research phase (TASK-01, TASK-02, TASK-02b) and the first architecture decisions (TASK-03) are done. Read before starting:
- `docs/adr/index.md` and ADR-0001 in full (how decisions bind you);
- the **Recommendation**, **Invariants** and **Open questions** sections of ADR-0002 to ADR-0006;
- `docs/adr/invariants.md`;
- `docs/research/candidate-principles.md` (summary level).

This task builds the foundation every later task stands on: the rules for agents, the design summary, the repository layout, the build and test harness, and a kernel that boots, prints, and reports test results. It writes **no** process, memory-management, IPC or POSIX code.

## Step 1: Record the architect's decisions in the ADR files

The architect (the human owner of this repository) accepted ADR-0001 to ADR-0006 as recommended, without changes, on 2026-10-25, and marked this in `docs/adr/index.md` (commit `8c4ee71`). The ADR files themselves still say `proposed`.

For each of ADR-0001 to ADR-0006, exactly as ADR-0001 prescribes for an accepted ADR:
- set `Status:` to `accepted`;
- fill the "Architect's decision" section with: `Accepted as recommended, 2026-10-25. Recorded by an agent from the architect's decision in docs/adr/index.md (commit 8c4ee71).`
- change nothing else in the ADR.

Update `invariants.md` if it carries status information. Commit this step on its own.

This transcription is the only status change you may make. You never set or change an ADR status on your own judgement.

## Step 2: `CLAUDE.md` (rules for every agent)

Create `CLAUDE.md` at the repository root. Short, imperative, no prose about the project's philosophy. It must contain at least these rules:

**Decisions**
- Accepted ADRs in `docs/adr/` are binding. Before changing an area, read the ADRs that govern it (`docs/adr/index.md`).
- Any deviation from an accepted ADR, or any new architectural decision, requires a new ADR with status `proposed`. Never change design silently. Never set an ADR status; only the architect does.
- Choices that are architectural (language features or toolchain channel, external dependencies in the kernel, memory layout conventions, ABI shapes) are decisions. Small implementation choices are not. When unsure, write a short proposed ADR and continue with the least committing option.

**Git**
- Use the repository's configured git identity. If none is configured, stop and ask. Never pass `-c user.name` / `-c user.email` and never invent an identity.
- Work on the branch named in the task. Never merge, never push, never rewrite published history, unless the task explicitly says so.

**Environment**
- Never create, delete or reconfigure virtual machines, accounts, or system-wide settings on the host. If a required environment is missing, stop and report.
- Do not modify `docs/research/` (historical record) or a task file in `docs/tasks/`.

**Work discipline**
- A stage is not done until `make test` passes from a clean checkout. Run it; do not infer it.
- Never claim a test passes, a number, or a behaviour without having run the command that shows it in this session.
- Logic that does not need hardware lives in a separate `no_std` crate with host tests (`cargo test`). The kernel crate contains only what needs the machine.
- Every `unsafe` block has a `// SAFETY:` comment stating the invariant it relies on.
- Do not start the next stage without the architect's go-ahead.

**Conventions**
- Repository language: English (code, comments, docs, commit messages).
- Crate names use the prefix `zoraya-`.
- Task specifications live in `docs/tasks/`.

Add any rule the ADRs or this task imply that is missing above, and keep the file under about 120 lines.

## Step 3: `docs/design.md` (design summary)

A short document (target: 2–4 pages) that a new agent reads to understand the system in ten minutes:
1. **Vision.** Leave a section headed `## Vision (architect)` containing only the line `TODO(architect): what is new in Zoraya, in the architect's words.` Do not write this section yourself.
2. **Platform.** aarch64, QEMU `virt`, Rust `no_std`, host is Apple Silicon; real hardware out of scope.
3. **Architecture as decided.** One paragraph per accepted ADR (0002–0006), stating the decision and linking the ADR. Facts only from the ADRs; no new decisions.
4. **Roadmap.** The stages below, with the current one marked:

| Stage | Content | Done when |
|---|---|---|
| 0 | Boot, UART output, panic handler, device tree parsing, test harness | `make test` passes |
| 1 | Exception vectors, GIC, timer, MMU (kernel in upper half), physical frame allocator, kernel heap | timer ticks; `Box`/`Vec` work in the kernel |
| 2 | Threads, context switch, scheduler, EL0, first native syscalls, ELF loader, initial ramdisk | a user process prints through a native syscall |
| 3 | Handles, ports and process objects per ADR-0003/0004/0006; native `spawn` | a process spawns a child and waits on its exit via a port |
| 4 | virtio-blk, filesystem service, `mkfs` on host | files survive reboot |
| 5 | virtio-gpu, virtio-input, compositor, terminal as GUI client | screenshot shows a terminal window |
| 6 | `libposix` T1 per ADR-0005, port of tcc, make, dash | `tcc hello.c && ./a.out` inside Zoraya |

5. **Where things are.** Repository map (Step 4).

## Step 4: Repository layout and toolchain

Create a Cargo workspace:

```
zoraya/
├── CLAUDE.md
├── Makefile
├── rust-toolchain.toml
├── .cargo/config.toml
├── docs/ (adr/, research/, tasks/, design.md, dependencies.md)
├── kernel/                 # zoraya-kernel: boot, arch, machine-dependent code only
├── crates/
│   └── zoraya-dtb/         # device tree parser, no_std, host-tested
├── tools/
│   ├── run-qemu.sh         # one place that knows the QEMU command line
│   └── posix-surface/      # existing research pipeline, unchanged
└── tests/                  # host-side runner for in-QEMU tests, if not inside tools/
```

- **Toolchain.** Prefer stable Rust. Pin the exact version in `rust-toolchain.toml` with the `aarch64-unknown-none` target and the components you need. If you find that stage 0 or the planned stages genuinely require nightly features, do not switch: write a proposed ADR-0007 with the concrete features and alternatives, and continue on stable for this task.
- **Dependencies.** Create `docs/dependencies.md`. Every external crate used by the kernel or a `no_std` crate is listed there with: name, exact version, license, what it is used for, why it is not written in-house. Only permissively licensed (MIT/Apache-2.0/BSD/ISC/Zlib) `no_std` crates. Prefer writing small things yourself; a device tree parser is small enough to write.
- **Move the task files.** Copy `TASK-01` to `TASK-04` specs into `docs/tasks/` if they are not already there (the architect may place this file there before you start).

## Step 5: Kernel stage 0

### Behaviour

1. Boot on `qemu-system-aarch64 -machine virt`, CPU `host` with `-accel hvf` on macOS, and CPU `cortex-a72` (or newer) with TCG elsewhere. One script, `tools/run-qemu.sh`, owns the command line and chooses the accelerator; nothing else invokes QEMU directly.
2. Handle entry at EL2 or EL1. If entered at EL2, drop to EL1 with a documented minimal EL2 configuration. Park all secondary cores (or confirm and document that only one core starts, depending on how the kernel is loaded).
3. Set up a stack, clear `.bss`, enter Rust.
4. Obtain the device tree blob address (verify and document how it is passed for the loading method you choose) and parse it with `zoraya-dtb` to find: the PL011 UART base, memory regions, CPU count, and the interrupt controller node. The UART address must come from the device tree, not a constant. If the device tree is missing or unparsable, fall back to a single documented constant only to print the error, then halt.
5. Print a boot banner, the kernel version (from `Cargo.toml` and the git commit, embedded at build time), the exception level at entry, and the parsed memory map.
6. A panic handler that prints the panic message and location over UART and then halts.
7. Shut down through PSCI `SYSTEM_OFF` (method from the device tree's PSCI node).

### Test harness

Two kinds of tests, one entry point.

**Host tests:** `cargo test` on the host for every crate under `crates/`. `zoraya-dtb` must be tested against real device trees dumped from QEMU (`-machine virt,dumpdtb=...`) for at least: default `virt`, `virt` with 4 CPUs and 2 GiB, `virt` with `gic-version=3`, `virt` with `virtualization=on`. Commit the dumped blobs as test fixtures with the exact QEMU command that produced each. Also test malformed input (truncated blob, wrong magic, bad offsets): the parser must return an error, never panic or read out of bounds.

**In-QEMU tests:** a test build of the kernel runs a list of tests and reports them over UART in a fixed, machine-readable format, for example:

```
ZORAYA-TEST begin <count>
ZORAYA-TEST ok <name>
ZORAYA-TEST fail <name> <message>
ZORAYA-TEST end pass=<n> fail=<m>
```

A host-side runner (script or small Rust binary) starts QEMU through `tools/run-qemu.sh`, applies a timeout, parses the output, and exits non-zero on any failure, panic, missing `end` line, or timeout. A panic during a test must be reported as a failure of that test, not as a hang. Do not rely on a QEMU exit code from the guest as the only signal; if you use semihosting for a clean exit code, verify it works under both HVF and TCG and document the result.

Stage-0 in-QEMU tests must include at least: the device tree is found and parsed; the UART base from the device tree matches the one the kernel is printing through; memory regions are non-empty and inside the physical address space; the exception level after boot is EL1; a deliberate panic in a dedicated test is caught and reported by the runner as an expected failure (a test of the harness itself).

**Make targets:**
- `make build` — builds the kernel;
- `make run` — boots it interactively;
- `make test-unit` — host tests;
- `make test-qemu` — in-QEMU tests;
- `make test` — all of the above plus `cargo fmt --check` and `cargo clippy` with warnings as errors;
- `make debug` — boots under TCG waiting for a debugger, with a one-line note on how to attach `lldb` or `gdb-multiarch`.

`make test` must pass from a clean clone on the architect's Mac, and must not need network access after the first toolchain install.

### ADR-0001 invariants

ADR-0001 says invariants become checks after acceptance. Implement now only the invariants that can already be checked at stage 0 (those about the ADR files themselves, from ADR-0001), as part of `make test`. In `docs/adr/invariants.md`, add a status for every invariant: `checked` (with the command), or `not yet applicable (stage N)`. Do not implement checks for code that does not exist yet.

### Continuous integration (optional)

If it is cheap, add a GitHub Actions workflow that runs `make test` on Linux with TCG. Keep it under 10 minutes per run. Do not enable any other workflow or service.

## Process

1. Read the context files.
2. Step 1 (ADR status transcription), commit.
3. Step 4 skeleton and toolchain, commit.
4. `zoraya-dtb` with host tests and fixtures, commit.
5. Kernel boot path, UART, panic, PSCI shutdown, commit.
6. In-QEMU test harness and runner, Make targets, ADR-0001 checks, commit.
7. Steps 2 and 3 (`CLAUDE.md`, `design.md`) — written last, so they describe what actually exists. Commit.
8. Independent review by a subagent that did not write the code: it runs `make test` from a fresh clone, reads every `unsafe` block and its `SAFETY` comment, checks the boot path against the ARM architecture requirements it relies on, checks that the device tree parser cannot read out of bounds on malformed input, and checks `CLAUDE.md` and `design.md` against the ADRs. Record findings in `docs/tasks/TASK-04-review.md` and fix them.
9. Final report: what was built, the exact `make test` output, what is documented as a known limitation, any proposed ADRs written.

Work on the branch `kernel/stage-0`, branched from `main`. Commit after each step. Do not merge or push.

## Definition of done

- ADR-0001 to ADR-0006 show `accepted` with the architect's decision recorded.
- `CLAUDE.md`, `docs/design.md` (with the Vision section left to the architect), `docs/dependencies.md` exist.
- The kernel boots under both HVF and TCG, prints the banner and memory map from the device tree, and shuts down via PSCI.
- `make test` passes from a clean clone and covers host tests, in-QEMU tests, formatting, lints and the ADR-0001 checks.
- The harness reports a panicking test as a failure without hanging.
- Independent review done, recorded, and findings addressed.

## Out of scope

- Exception vectors beyond what boot requires, interrupts, timer, MMU, memory allocation (stage 1).
- Any process, IPC, syscall or POSIX code.
- Changing any ADR beyond Step 1, or `docs/research/`.
- Creating or reconfiguring VMs, accounts, or host settings.
