# TASK-08: Kernel stage 2 — threads, scheduler, EL0, first native syscalls, ELF loader, initial ramdisk

The project owner gave the go-ahead for this stage and had no task file for it, so the implementing agent wrote this
one from the roadmap line, in TASK-05's form.

| Stage | Content | Done when |
|---|---|---|
| 2 | Threads, context switch, scheduler, EL0, first native syscalls, ELF loader, initial ramdisk | a user process prints through a native syscall |

Still single-core. Handles beyond what the first syscalls need, ports, process objects for user space and `spawn` are
stage 3.

## Decisions

Proposed ADRs before the code that depends on them. **ADR numbers 0030 to 0039 are reserved for this task**: TASK-07
uses free numbers in 0010 to 0019. At least: threads and scheduling; user address spaces, ASIDs and user memory access;
system call ABI; initial ramdisk and ELF loading; user faults (following ADR-0004).

## Work

- Host-tested crates: `zoraya-abi` (the native ABI, with the checks of INV-0002-1, INV-0004-1 and INV-0006-2),
  `zoraya-elf`, `zoraya-ramdisk`.
- Kernel: thread stacks with guards, context switch, run queue, preemption by deadline, blocking; per-process TTBR0;
  lower-EL vectors, the stack limit across EL0 (ADR-0024's open question); system calls; faults from EL0.
- User: a runtime and test programs built for EL0, packed into the ramdisk by a host tool.

## Tests

In-QEMU, under every `make test-qemu` configuration:
- threads run, yield, sleep to a `Deadline`, and a kernel thread's stack overflow hits its guard;
- preemption: two CPU-bound user processes both progress;
- a user process prints through a syscall and exits with a code the kernel reads;
- isolation: a user program cannot read or write kernel memory or another process, and cannot run a privileged
  instruction; each ends in an `ExitRecord` with reason `Fault`, and the kernel survives;
- a bad pointer or bad handle in a syscall returns a typed error;
- W^X holds in user mappings: an ELF with a writable executable segment is refused;
- registers, FP state and TLS survive preemption.

`make test` from a clean clone must pass, and the normal kernel's boot must show the roadmap criterion.

## Process

ADRs, crates, kernel steps, tests, invariants and docs, then an independent review by a subagent recorded in
`docs/tasks/TASK-08-review.md`, with fixes. Branch `kernel/stage-2` from `main`. Do not merge or push.
