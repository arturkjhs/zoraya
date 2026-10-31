# ADR-0030: Threads and scheduling

- **Status:** proposed
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0020, ADR-0023, ADR-0024
- **Evidence:** CP-18, CP-20, DM-067, DM-068, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#hubris, alternatives-postmortem.md#l4-family-and-sel4

## Context
Stage 2 needs more than one flow of control. ADR-0023 rules out a periodic tick, and ADR-0020 reserves a window for thread stacks. Which scheduling policy suits Zoraya is an open, contested question (CP-20, DM-068), and there are no workloads to measure yet. So this stage needs the smallest mechanism that does not close that question.

## Decision drivers
1. No idle wakeups and no periodic tick (CP-20, DM-067).
2. The policy must be replaceable, since DM-068 is contested.
3. Lock scope must be small and written down (CP-18).
4. Each kernel stack must fault on overflow (ADR-0020).
5. One CPU at this stage, with nothing that stops SMP later.

## Considered options

### Option A: Fair scheduler with weights (Linux baseline)
- **Description:** A virtual-runtime or deadline-based scheduler with priorities and load tracking, and kernel preemption.
- **Evidence for:** Handles mixed loads well, and is widely used [outside evidence: Linux `Documentation/scheduler/sched-eevdf.rst`].
- **Evidence against:** DM-068 records decades of churn in this area. No workload here can tune it.
- **Cost:** High (estimate: 1,000 lines or more), and preemptible kernel code demands a much stricter locking discipline.
- **Precedent:** Linux.

### Option B: Static priorities with a fixed task set
- **Description:** Hubris fixes its tasks and priorities at build time (alternatives-postmortem.md#hubris). seL4 uses fixed priorities with round robin inside each (alternatives-postmortem.md#l4-family-and-sel4) [outside evidence: seL4 reference manual].
- **Evidence for:** Simple and predictable.
- **Evidence against:** A fixed set cannot serve a dynamic process model. Priorities need someone to assign them, and nothing does yet.
- **Cost:** Low (estimate: 200 lines) in the seL4 form.
- **Precedent:** Hubris, seL4.

### Option C: Round robin, non-preemptible kernel, slice by deadline
- **Description:** One run queue in FIFO order. A running user thread gets a 10 ms slice, armed as a one-shot deadline only when another thread is waiting to run. The kernel runs to a blocking point: a slice that ends in the kernel takes effect on return to EL0.
- **Evidence for:** Meets drivers 1 and 3 at once: with interrupts masked in the kernel, no kernel lock is ever contended on one CPU. The policy is one small module (driver 2).
- **Evidence against:** A long kernel path delays everything; no priorities, so no latency guarantee. Zircon has a preemptible kernel and a fair scheduler (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon scheduler docs], and SMP will need more than this.
- **Cost:** Lowest (estimate: 300 lines).
- **Precedent:** Early Unix kernels were non-preemptible [outside evidence: Lions' Commentary on UNIX 6th Edition].

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Thread.** A kernel object with a saved context, a state (`Ready`, `Running`, `Blocked`, `Dead`), a kernel stack, and an optional owning process.
- **Stacks.** 64 KiB each in the stack window of ADR-0020, separated by one unmapped guard page, backed by frames. Creation fails with an error if memory runs out.
- **Context switch.** Called from kernel code at a call boundary, so it saves x19 to x30, SP, d8 to d15 and `FPCR`/`FPSR`. All other state, including a user thread's whole register set, is already in the exception frame on its stack, so FP state is eager and there is no lazy trap. It also switches the user TLS register `TPIDR_EL0`, the stack limit and the address space.
- **Interrupts.** Kernel code runs with IRQ masked, except the idle thread and a kernel thread that unmasks them to wait. A timer callback only marks the need to reschedule.
- **Preemption.** On return to EL0 only, and at explicit `yield` or blocking points. The boot thread and other kernel threads are cooperative.
- **Slice.** 10 ms `[assumption]`, armed only when a thread is waiting to run, so a lone thread takes no timer interrupts, as CP-20 asks.
- **Idle.** One thread that sleeps with `wfi`, with interrupts masked around its check of the run queue so a wakeup cannot be lost.
- **Blocking.** `sleep_until(Deadline)` and waiting for a thread or process to end. A thread blocks with the deadline queue of ADR-0023 waking it.
- **Dead threads.** A thread cannot free the stack it runs on, so another thread reaps it.

## Consequences
- **Positive.** Small enough to review line by line. A lone thread is never interrupted.
- **Negative.** No priorities. Kernel paths are not preempted, so a long one delays everyone. It does not scale to SMP as it stands.
- **Follow-up.** Priorities or fair sharing when a workload shows the need; per-CPU queues with SMP.

## Invariants
- **INV-0030-1.** With one runnable thread and no pending deadline, the timer is off. *Check:* in-QEMU test reading the timer state from a lone thread.
- **INV-0030-2.** Each thread stack has an unmapped page below it, and an overflow is reported, not silent. *Check:* in-QEMU test of a table walk, and an expected-failure overflow on a thread stack.
- **INV-0030-3.** Two CPU-bound user threads both make progress. *Check:* in-QEMU test with two spinning user programs.

## Open questions
- The slice length, and whether it should be per thread.
- When to make the kernel preemptible, if ever.

## Revisit triggers
- A measured latency problem caused by a kernel path or by the lack of priorities.
- SMP.

## Architect's decision
