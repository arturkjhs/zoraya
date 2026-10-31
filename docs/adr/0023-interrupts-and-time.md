# ADR-0023: Interrupts and time

- **Status:** accepted
- **Date:** 2026-10-31
- **Depends on:** ADR-0001, ADR-0006
- **Evidence:** CP-20, DM-065, DM-067, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#l4-family-and-sel4

## Context
ADR-0006 makes every blocking call take a `Deadline`, a monotonic instant or `Infinite`. So the kernel needs a monotonic clock and "interrupt me at D". CP-20 asks for no periodic tick (DM-067) and for monotonic-only deadline types (DM-065).

Measured facts (TASK-04, `docs/design.md`): HVF on the M4 cannot start with GICv2 and the default `virt` machine under HVF has GICv3; TCG defaults to GICv2 and offers GICv3 with `gic-version=3`. So both versions are reachable, and only one under HVF.

## Decision drivers
1. Work under HVF and TCG with the configurations `make test-qemu` already uses.
2. Deadlines never fire early, and the idle kernel is not woken for nothing (CP-20, DM-067).
3. Monotonic time cannot go backwards and is not adjusted (DM-065).
4. Time arithmetic must be host-testable, with no overflow over any realistic uptime.
5. The stage-2 scheduler needs a periodic preemption source, but only while something runs.

## Considered options

### Option A: Periodic tick on the physical timer, GICv3 only
- **Description:** A fixed HZ tick drives time and timeouts, in the manner of a classic Linux kernel, with the EL1 physical timer. Deadlines are checked at each tick. Only GICv3 is supported.
- **Evidence for:** Simple and well understood; one GIC driver. Linux now ships a tickless mode only as an option over this structure (DM-067).
- **Evidence against:** Timeout precision is limited to the tick, and an idle machine wakes up for nothing (DM-067, CP-20). Dropping GICv2 loses `tcg-default`, an existing test configuration. Under a hypervisor the physical timer is not the one the hardware virtualises for the guest [outside evidence: Arm ARM DDI 0487, "The Generic Timer in AArch64 state"; QEMU `target/arm/hvf/hvf.c`].
- **Cost:** Low (estimate: 150 lines).
- **Precedent:** Linux with `HZ`. seL4's non-MCS configuration uses a periodic tick (alternatives-postmortem.md#l4-family-and-sel4) [outside evidence: seL4 reference manual].

### Option B: One-shot deadlines on the virtual timer, GICv2 and GICv3
- **Description:** The comparator is always programmed for the earliest pending deadline. Periodic work is a client that re-arms its own deadline. `CNTV_*` at both accelerators.
- **Evidence for:** Matches ADR-0006 and CP-20 exactly. The virtual timer is the one HVF virtualises in hardware and the one Linux guests use, so it is the best-tested path under QEMU HVF [outside evidence: QEMU `target/arm/hvf/hvf.c`]. With `CNTVOFF_EL2 = 0`, set by the boot code at EL2, virtual and physical counts agree. Zircon is tickless in this way (alternatives-postmortem.md#fuchsia--zircon-incl-starnix) [outside evidence: Zircon kernel `timer` sources].
- **Evidence against:** Two GIC backends. A queue of deadlines is needed instead of a counter. Tick accounting has to be derived, which DM-067 lists as a real cost.
- **Cost:** Moderate (estimate: 500 lines including both GIC backends).
- **Precedent:** Zircon; seL4 MCS (alternatives-postmortem.md#l4-family-and-sel4) [outside evidence: seL4 reference manual].

### Option C: A periodic tick plus one-shot precision timers
- **Description:** Both: a tick for accounting and scheduling, and a one-shot comparator for precise deadlines, like Linux `hrtimers` under `CONFIG_HZ`.
- **Evidence for:** Keeps tick-based accounting simple.
- **Evidence against:** Two mechanisms, and still the idle wakeups CP-20 rules out (DM-067).
- **Cost:** Highest (estimate: Option B plus the tick path).
- **Precedent:** Linux `hrtimers` [outside evidence: Linux `Documentation/timers/hrtimers.rst`].

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **GIC.** v2 and v3, chosen at boot from the device tree. Any other controller stops the boot with a message. GICv3 runs with system registers and affinity routing. No LPIs or ITS at stage 1. Tested: GICv3 under HVF and TCG, GICv2 under TCG only.
- **Interrupt numbers** come from the device tree: the `arm,armv8-timer` node's virtual timer entry and the UART's `interrupts`.
- **Clock.** `Instant` is a `u64` count of nanoseconds since the kernel's clock start. Source: `CNTVCT_EL0` minus its value at start, converted with `CNTFRQ_EL0` by 128-bit arithmetic, rounded down. Guarantees on one CPU: never decreases; never adjusted; rate is that of the counter; no meaning across boots. `u64` nanoseconds last 584 years. The clock says nothing about wall time (CP-20). Whether it counts during host-side VM pauses is what the counter does.
- **Deadlines.** `Deadline` is `At(Instant)` or `Infinite`, as ADR-0006 rule 4 fixes. Conversion to counter ticks rounds up, so a deadline fires no earlier than requested. `timer::arm(instant, callback)` inserts into a bounded queue ordered by time, with cancellation by id, and programs `CNTV_CVAL_EL0` for the earliest entry. A queue that is full is an error for the caller.
- **Scheduler tick.** Stage 2 re-arms a deadline per time slice, only while a thread runs.

## Consequences
- **Positive.** No idle wakeups. ADR-0006's `Deadline` has a direct implementation. Both GICs that QEMU offers on the test machines work.
- **Negative.** Two GIC backends to keep alive. GICv2 is never exercised under HVF. The deadline queue is bounded, so a future large number of timers needs a different structure.
- **Follow-up.** Per-CPU timers and redistributor lookup for all CPUs at SMP. Interrupt priorities when a use appears.

## Invariants
- **INV-0023-1.** A deadline fires no earlier than requested. *Check:* host test of the round-trip conversion in `zoraya-time`, and an in-QEMU one-shot test.
- **INV-0023-2.** `Instant` never decreases on one CPU. *Check:* in-QEMU test reading the clock in a loop across timer interrupts.
- **INV-0023-3.** The kernel programs no periodic hardware tick: the comparator is only ever set for the earliest queued deadline, and an empty queue leaves the timer disabled. *Check:* in-QEMU test that the timer is disabled when the queue is empty.
- **INV-0023-4.** Interrupt numbers come from the device tree. *Check:* `zoraya-dtb` host tests against the QEMU fixtures, and the in-QEMU delivery tests under GICv2 and GICv3.

## Open questions
- Whether suspended time counts (CP-20): there is no suspend at stage 1.
- How much a host-loaded HVF vCPU delays a timer interrupt. The in-QEMU tolerance is stated in the test and has to hold up in practice.

## Revisit triggers
- A timer queue bound that a real workload reaches.
- Measured deadline lateness under HVF that breaks an ADR-0006 use.
- A need for GICv4 or message-signalled interrupts (virtio-pci, stage 4 or later).

## Architect's decision

Accepted as recommended, 2026-10-31. Decided by an agent acting as architect, on the project owner's explicit instruction to take this decision in the owner's place. No human specialist reviewed it; the independent review is in `docs/tasks/TASK-05-review.md`.
