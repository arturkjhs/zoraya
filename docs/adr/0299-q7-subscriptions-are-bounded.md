# ADR-0299: Q7, subscriptions are bounded per port and per source

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0006, ADR-0041, ADR-0261, ADR-0270
- **Evidence:** CP-14

## Context
The review of the system calls after stage 8 (`docs/tasks/TASK-26-syscall-review.md`, error 2; CP-14 on the all-or-nothing bound) found that `port_subscribe` has no bound: a port takes any
number of subscriptions (any `u64` key), and a source any number of subscribers; each costs 128 bytes of the caller's
charge, so a room's memory limit is the only bound, 170,000 to 700,000 subscriptions (estimate). Under `EVENTS` (a
spinlock, IRQ masked) the sorted insert moves the port's whole list, `Events::set` visits every subscription of a source
whose bits rise (also in another process's call, as when `fsd` answers on a channel end the room subscribed to), and
closing a port or freeing a source frees them all. INV-0270-1 (2 ms masked) does not hold for them.

## Decision drivers
1. INV-0270-1 for a program that may pass any arguments (ADR-0270 driver 1).
2. `set` runs in interrupt handlers and in other processes' calls: it cannot pause in steps.
3. No service needs more than one subscription per handle it serves, and a handle table holds 1024 handles.
4. The change to a program that subscribes less must be nil (ADR-0270 driver 4).

## Considered options

### Option A: walk the lists in steps
- **Evidence against:** driver 2: `set` runs with `EVENTS` held, from handlers; the ready list and the lists of a source change
  under it. A resumable walk needs a second structure that handlers and calls both keep right. **Cost:** *(estimate)* large.

### Option B: a bound on the subscriptions of a port and of a source, refused as `LimitReached`
- **Description:** `zoraya_events::MAX_SUBSCRIPTIONS` = 1024 for a port and for a source. `subscribe` past it fails with
  `Error::TooMany`, which the kernel maps to `Status::LimitReached` (as `Full`, the user packets' bound, already is), the
  `paid` dropped and nothing changed. Each walk of one port's or one source's list is then a fixed amount: at most 1024
  subscriptions. The two bounds do not bound a call that touches both (review of the fixes, error 3): freeing a source with 1024
  subscribers on 1024 ports that each hold 1024 entries removes 1024 entries, each by a binary search and a move of up to
  1023 entries of 16 bytes (16 MiB moved at most). The first version swept every port's list (over 1M entries, 1.94 ms on
  the host, about 10 ms in the kernel); the search takes 1.05 ms on the host (`freeing_a_full_source_on_full_ports_is_bounded`,
  release build, one run), and `t_bound` measures the call at an eighth of the product (128 ports). A handle table of 1024
  also bounds what one process can hold, but not what many processes subscribe to one source.
- **Evidence for:** drivers 1 to 4; the same shape as `MAX_USER_PACKETS`.
- **Evidence against:** a service with more than 1024 clients on one port (none today; the table's own limit is 1024 handles)
  would need a second port.
- **Cost:** 10 lines and tests.

### Option C: do what POSIX does (no bound; `epoll` limits watches by the user's memory: `max_user_watches`)
- **Evidence against:** driver 1; Linux's limit counts a user's memory, as ADR-0261 does, and a masked walk is not a thing
  Linux has to bound (it is preemptible).

## Recommendation
**Recommendation (not a decision): Option B**, with the limit 1024, a number that is `[assumption]` until measured: `t_bound`
measures the last `port_subscribe`, the signal of a source with 1024 subscribers and the close of such a port against the
same with 8, and the call past the limit.

This changes the behaviour of ADR-0006: a port is no longer unbounded in subscriptions. It is a deviation that this ADR
records; nothing else in ADR-0006 or ADR-0041 changes. The work of freeing 1024 subscriptions in one interval is not
counted in units (the `Paid` drops take the charge lock and walk at most 32 ancestors); the time is what `t_bound` checks.

## Consequences
- **Positive.** `port_subscribe`, `set` on a source, `port_close` and `free_source` do a fixed amount of work.
- **Negative.** A new refusal (`LimitReached`) that no program meets today.
- **Exits.** The bounds are per port and per source; an exit closes up to 64 handles per interval (ADR-0270 point 2), so
  64 ports of 1024 subscriptions were about 20 ms masked on the host (review of the fixes, error 4). `object::weight` now counts
  a port's subscriptions, and those of the source of any other object, one unit per 16 (a full port is a whole step), so
  the exit pauses between them; `t_bound` `full-ports` closes 64 ports and 64 jobs, each with 1024 subscriptions.
- **Follow-up.** Count the 1024 as units if a measurement under HVF shows the close near 2 ms.

## Invariants
- **INV-0299-1.** A port holds at most 1024 subscriptions and a source has at most 1024; the next `subscribe` fails with
  `TooMany` (`LimitReached` for a program) and changes nothing. *Check:* `cargo test -p zoraya-events`, and `t_bound` for
  the masked time at the limit.

## Open questions
- Whether 1024 is right for a source: a channel end or a job observed by more than 1024 ports.

## Revisit triggers
- A service that needs more than 1024 subscriptions on one port.

## Architect's decision
(Left empty for the human: accepted / changed / rejected, date, notes.)
