# ADR-0060: Console input as raw bytes

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0006, ADR-0022, ADR-0040, ADR-0043
- **Evidence:** CP-14, CP-25, DM-069, DM-070, DM-071

## Context
The console only writes. A person at the serial line needs a shell, so bytes must reach user space. DM-069 records what a kernel terminal layer became, and CP-25 keeps terminal policy out of the native kernel. Nothing is measured here; the buffer size is an assumption.

## Decision drivers
1. No terminal policy in the kernel: no line discipline, echo, editing or signal generation (DM-069, DM-070, CP-25).
2. An interrupt handler neither allocates nor fails (ADR-0022, CP-14).
3. One wait for input and children (ADR-0006).
4. Input is untrusted bytes, and escape sequences mean nothing to the kernel (DM-071).

## Considered options

### Option A: A line discipline in the kernel (Linux TTY)
- **Description:** The kernel buffers lines, echoes, edits, and turns Ctrl-C into a signal to the foreground group.
- **Evidence for:** Every program gets editing for free, it works while a program is hung, and POSIX expects it.
- **Evidence against:** DM-069 and DM-070: a large layer with its own state, and signal generation tied to process groups.
- **Cost:** *(estimate)* Thousands of lines, permanently.
- **Precedent:** Unix.

### Option B: Kernel editing with a raw mode (Plan 9)
- **Description:** The kernel edits lines, and a control file switches to raw bytes, as Plan 9's `cons` and `consctl` do `[outside evidence: Plan 9 cons(3), 9p.io/magic/man2html/3/cons]`.
- **Evidence for:** Simple programs read lines with no library, and a small policy stays in one place.
- **Evidence against:** Two modes, and the kernel still knows Ctrl-U and backspace. Stage 3 has one reader, which wants raw bytes.
- **Cost:** *(estimate)* 150 lines plus a mode.
- **Precedent:** Plan 9.

### Option C: Raw bytes, a fixed ring, a signal
- **Description:** The receive interrupt fills a fixed ring. `console_read` takes bytes and never blocks. `READABLE` is a level signal.
- **Evidence for:** Drivers 1 to 4. Zircon reads its debug log with a read right and a should-wait error when empty, which is the same shape `[outside evidence: fuchsia.dev zx_debuglog_read, for output only]`.
- **Evidence against:** Every program that reads input needs its own editor. Bytes are lost if the reader is slow.
- **Cost:** *(estimate)* 120 lines.
- **Precedent:** Zircon's call shape, for a different object.

### Option D: A terminal service with the UART in user space
- **Description:** A process owns the UART and serves clients, as CP-25 foresees.
- **Evidence for:** No console in the kernel, eventually.
- **Evidence against:** It needs channels, device objects and a mapping call that do not exist at stage 3. The kernel must keep the UART for its own reports (ADR-0024).
- **Cost:** *(estimate)* Stage 4's objects first.
- **Precedent:** QNX's and Fuchsia's user-space drivers.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Ring.** 256 bytes `[assumption]`, a static array in a host-tested `no_std` crate. The interrupt handler drains the UART FIFO into it. A byte that does not fit is dropped and counted, and nothing allocates.
- **`console_read(console, buffer)`** needs `READ`, never blocks, and returns the bytes taken: 0 if there are none. It takes at most the ring's size, however long the buffer, and the buffer's bytes up to that size are overwritten even if fewer are taken, because the kernel touches them first so that a bad address loses no input.
- **`console_dropped(console)`** needs `READ`, and returns the bytes dropped since boot.
- **Signal.** The console object changes from `NotWaitable` to waitable, with `READABLE` asserted exactly while the ring holds a byte. Rights: `COMMON`, `READ`, `WRITE`, `WAIT`.
- **Two holders.** One ring exists for the machine, so readers share it: each byte goes to whichever call takes it first, and nothing arbitrates. A parent must not list `READ` for a child that should not read. The shell lists `WRITE` only.
- **Output is unchanged.** Writes stay synchronous and polled.
- **Kernel knows no byte.** Ctrl-C is a byte like any other, so a burst that overruns the ring can lose it. A person presses it again.

**Tensions with accepted ADRs.** *ADR-0040:* its table lists the console's rights and calls, and this changes them, so accepting this ADR replaces that row (ADR-0001 rule 1). *ADR-0006 rule 1* lists terminals as byte streams, with signals of their own, and rule 6 has a non-blocking read return `WouldBlock`. This console is neither: `READABLE` alone, and 0 when empty, because `WouldBlock` is not in `Status` at this stage. A terminal service at a later stage must take the rule's form.

## Consequences
- **Positive.** The kernel holds no terminal state. Input goes through the one wait.
- **Negative.** A byte received while the reader sleeps past 256 bytes is lost, with a count. A hung foreground program cannot stop the shell, but a program holding `READ` can steal input.
- **Follow-up.** A terminal service replaces this reader at a later stage.

## Invariants
- **INV-0060-1.** The ring never allocates, keeps byte order, and counts every dropped byte. *Check:* host tests in its crate, which does not link `alloc`.
- **INV-0060-2.** `READABLE` is asserted exactly while the ring is non-empty. *Check:* in-QEMU test feeding the ring.
- **INV-0060-3.** `console_read` never blocks and needs `READ`. *Check:* in-QEMU test on an empty ring and with a handle lacking `READ`.
- **INV-0060-4.** A burst larger than the ring loses bytes, counts them, and crashes nothing. *Check:* scripted serial test.

## Open questions
- Whether 256 bytes suits pasted input: nothing measured.

## Revisit triggers
- A terminal service at a later stage.
- Real hardware with no FIFO.

## Architect's decision
