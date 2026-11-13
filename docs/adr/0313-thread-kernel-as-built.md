# ADR-0313: Threads in the kernel as built: the owner is the last to leave, lazy thread lists, parked threads woken by one

- **Status:** proposed
- **Date:** 2026-11-13
- **Depends on:** ADR-0270, ADR-0285, ADR-0300, ADR-0301, ADR-0306, ADR-0307
- **Evidence:** CP-10, CP-14, DM-004, GR-011, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

The kernel half of part 9b (ADR-0300, ADR-0301, ADR-0307, accepted 2026-11-13) was implemented. Five places where the
accepted text leaves a choice, or says a thing the code does differently without changing what it guarantees, are recorded
here instead of changed silently (CLAUDE.md "Decisions"). The thread is a part of the process only in the sense DM-004 warns about
if every feature needs a retrofitted rule; these refinements keep each rule local to one function. Nothing here changes a decision of the architect; each item says
which sentence it refines. Measured on the development host under HVF: `t_thread` (the raw-ABI test of step 1) runs 100,000
futex hand-offs in about 0.3 s and 2,000 create-and-join rounds in about 70 ms.

## Decision drivers

1. Every guarantee of ADR-0300 item 6 (no sibling runs after the record is fixed; the space is freed with no thread running or
   ready) must hold with the least new machinery in the kernel (ADR-0270 bound, one CPU).
2. A process that never makes a second thread pays nothing for the feature (ADR-0261, ADR-0260's per-process figures; the
   existing tests compare usage to the byte).
3. A wake that is not for the waiter (a resume, an exit sweep) must not look like a futex wake or end a sleep (GR-011 point 4:
   spurious wakes are part of the futex contract, so the waiter re-checks).
4. Nothing is asynchronous (CP-10): a thread leaves only at a return to EL0 or in a blocked call. Every limit is refused at the
   request (CP-14).

## Considered options

### Option A: Exactly the accepted text: the owner is the thread that fixed the record, and waits for the count to reach one
- **Description:** The asker owns the teardown and sleeps until every sibling has counted itself out, then tears down.
- **Evidence for:** Matches ADR-0300 item 6 word for word.
- **Evidence against:** The owner needs a second wait (a counter and a wake) with a lock-free handshake and a rule for the case
  that the asker is a different process (`job_kill`), where "the last to leave" is named anyway. Two roles for one job.
- **Cost:** *(estimate)* 80 lines more than Option B and one more sleeping state.
- **Precedent:** Zircon's process teardown waits for its threads to be dead (alternatives-postmortem.md#fuchsia--zircon-incl-starnix).

### Option B: The owner is the thread whose leave empties the list (this ADR)
- **Description:** The record is fixed once (first phase, as accepted). Every thread leaves when it sees the record. The thread
  whose leave leaves the list empty runs the teardown. Nobody waits for a count.
- **Evidence for:** Driver 1. The accepted guarantees hold: after the record is fixed no thread runs a user instruction (each
  leaves at its next return to EL0 or in its blocked call), and the space is freed by the last, with none running or ready
  (a leaver is out of the list and off the CPU before the next thread runs, because it does not yield between its removal and
  `sched::exit_current`, on one CPU). The "one owner" invariant is a pure function of the list (`zoraya_thread::Members::leave`,
  model-checked, INV-0300-6).
- **Evidence against:** The owner may be a thread that did not ask; it uses the record the asker fixed.
- **Cost:** *(estimate)* less than A.
- **Precedent:** the last close of a reference-counted object.

### Option C: Make the thread lists and the futex node part of every process
- **Description:** Reserve 128 entries for the thread list, the futex wait list and the mapping-lock waiters when the process is made.
- **Evidence for:** No lazy path.
- **Evidence against:** About 8 KiB more for every process (255 of them, charged to the creator's job), which moves every
  usage figure of every existing test and the memory of rooms with no thread in them (driver 2).
- **Cost:** *(estimate)* none in code, 2 MiB of kernel memory at the process limit.
- **Precedent:** Linux's `task_struct` per thread, paid by all.

## Recommendation

**Recommendation (not a decision): Option B, with Option A's guarantees, and the five refinements.**

1. **The owner** (refines ADR-0300 item 6, "the owner waits for the count to reach one"). The thread whose leave empties the
   list owns the teardown. `Members::leave` names it once; a process that was not ending and loses its last thread (a `thread_exit`
   of the last one) begins its end in the same leave, as `Exited` with code 0 (ADR-0300 item 3). The first thread to leave an
   ending process wakes its siblings in steps of 64 (`thread::wake_siblings`), and each leaver also wakes the new first thread,
   so a sibling that the index-based steps skipped as the list shortened is still reached.
2. **Lazy lists** (refines ADR-0300 item 1 and ADR-0301 item 7). A process's thread list starts with room for its first thread.
   The first `thread_create` or `futex_wait` of a process makes the futex list and the mapping-lock waiter list, each with
   room for 128, and grows the thread list to 128 (`thread::ensure_multi`), charged once to the process's job and refunded at
   its end. Every later wait and creation allocates nothing, as ADR-0301 item 7 requires. A refused `thread_create` changes nothing
   from its first call on: the lists are made after the last check that can refuse, and kept if a later step fails.
3. **Resume wakes one parked thread** (refines ADR-0300 item 7, "`cont` wakes one parked thread"). A parked thread registers as the
   process's park head; `cont` wakes that thread, and it wakes the other parked threads, in steps, from its own context. The
   same park serves a fork's freeze (`thread::Freeze`, ADR-0306 item 2): the threads other than the caller park at their next
   return to EL0 and run again when the guard drops.
4. **Wakes that are not for the thread are harmless** (refines ADR-0301 item 1, "returns `Ok` with 0 when woken"). A futex waiter
   is woken for the futex only if its node was taken off the list by a waker, which sets the thread's `woken` flag under the
   list's lock first. A wake from anything else (an exit sweep, a resume) finds the flag clear, and the waiter checks, in
   this order, the ending, `thread_interrupt`, its deadline, and then blocks again. `sleep_until` is never woken by anything
   but its deadline, an interrupt or the ending.
5. **A thread that ends refunds at the call** (confirms ADR-0307 item 7, and says which): its `threads` figure, its record and
   its kernel stack go back to the job in `thread_exit` or at its leave (`Paid::release_exited`); the memory itself is freed by the
   reaper at the next switch. The refund is made before the `clear` store and its wake (which may yield, in steps of 64, with
   that many joiners), so a joiner woken by the store always sees the figures refunded.

## Consequences

- **Positive.** No owner handshake; a process pays nothing for threads it never makes; the guarantees of the accepted ADRs hold.
- **Negative.** The first thread-making call of a process allocates 8 KiB once (charged); a process killed while a sibling is
  in a stepped call waits for that call's next step point, as ADR-0300 item 6 already says.
- **Follow-up.** `t_bound`'s extension (ADR-0301 INV-0301-4) measures the sweeps; the fork half of the freeze is exercised by
  `libposix` (step 2 of part 9b).

## Invariants

- **INV-0313-1.** Exactly one thread of a process runs its teardown, and it is the thread whose leave empties the list; a process
  that was not ending and loses its last thread is ended by that leave. *Check:* `cargo test -p zoraya-thread`.
- **INV-0313-2.** A futex waiter that returns for any reason leaves no node on the process's wait list: after any mixture of
  wakes, time-outs and interrupts, a `futex_wake` of the address wakes none. *Check:* `t_thread` in `make test-qemu`.
- **INV-0313-3.** A thread that has stored its `clear` word has already given its `threads` figure and kernel memory back: the job's
  usage read right after `join` and the close of the thread's handle equals its value before the thread was made: the refund precedes the store and the wake of the `clear` word, which may yield with 64 or more waiters. *Check:* `t_thread` in `make test-qemu`.

## Open questions

- Whether the lists should be made at the first `thread_create` only, and `futex_wait` in a process that has never made a
  thread should answer `WouldBlock` or `BadState` (a program that waits on a word with no other thread can only time out).
- Whether the park head should be a list when a process has many parked threads after a long suspend (one wake plus steps is
  128 wakes at most).

## Revisit triggers

- SMP: the owner's guarantee "off the CPU before the next thread runs" needs a count or a fence.
- A measured sweep cost (ADR-0301 INV-0301-4) over ADR-0270's bound.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
