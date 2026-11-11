# ADR-0291: Accounts in the file service: the request that carves one, the derived bounds, and what a bound does

- **Status:** proposed
- **Date:** 2026-11-11
- **Depends on:** ADR-0055, ADR-0056, ADR-0100, ADR-0262, ADR-0263, ADR-0265, ADR-0269
- **Evidence:** CP-01, CP-14, quotas/survey.md#the-service-pays-problem

## Context
ADR-0262 item 2 gives `fsd` accounts: `clone(rights, server_end, budget)` makes a new account carved from the caller's,
with the table's bounds (32 open, 4 buffers, 1 held reply per untrusted account; four of them). Building it in Q3
(TASK-26) met questions the ADR leaves open. (1) `Clone` has one field and every client sends it; adding a field
changes the wire for all of them. (2) The table gives three bounds per account but `clone` carries one budget.
(3) What "1 held reply" does when a second reply cannot be sent. (4) Who pays for a FIFO's service-held stream ends.
(5) What happens to an account's carve when the account it was carved from ends first. (6) Which `sync`-like
requests ADR-0265 item 9 folds.

## Decision drivers
0. No client holds a service's memory without a bound (CP-14), and a service knows a client only by its connection tree (CP-01); the choice among a static partition and a donated quota is ADR-0262's (`quotas/survey.md#the-service-pays-problem`).
1. A change to an accepted wire shape costs every client (ADR-0262 driver 3).
2. Every count and length a client sends is checked; every per-client resource has a bound (fsd is a trust boundary).
3. The sum of bounds never passes the service's (INV-0262-3), at every moment.
4. The simplest rule that keeps a service usable for the session, and reversible.

## Considered options

### Option A: a new request `CloneAccount` (ordinal 40), derived bounds, close at the held bound
- **Description:** `CloneAccount { rights, budget }` is `Clone` plus a budget; `Clone` is unchanged. The account's
  other bounds follow from the budget: buffers = budget / 8, held replies = budget / 32, each at least 1 (4 and 1 for
  the untrusted 32). A budget of 0, or over 32 when carved from the session's account, is `InvalidArgument`; one the
  caller's account cannot spare (open, buffers or held), or a fifth account in the untrusted column, is
  `TooManyOpen`. Everything is carved: the caller's limits fall by the new account's, and rise again when the
  account's last connection closes. Accounts the dead account carved answer to its parent from then on; if the
  session's account itself ends first, its carve is not returned (nothing can use it) and a new session account is
  made with what the live ones leave. A reply that cannot be sent because the client's queue is full (`Full`), when
  the account already holds its bound of replies, is dropped with its connection: the client sees the channel close.
  A reply refused for the machine's memory (`NoMemory`) is not the client's doing: it is held and retried as before
  accounts, bounded by the connections (review Q3, finding 3).
  FIFO stream ends, which exist only in the temporary directory service, are the service's, not an account's, and are
  bounded by the global `MAX_OPEN`; a service with FIFOs refuses `CloneAccount` (`InvalidArgument`), since a deferred
  FIFO answer is charged without a check. An account with no connection whose sub-accounts live stays with the limits
  it has left until the last of them goes, so the sub-accounts stay in its column and the session cannot carve a fifth
  top account from the room's budget (review Q3, finding 1); the session's account is kept in the same way, so the
  sums stay exact after it ends. `Sync` and a durable write (`DURABLE`, a write and a `sync`, ADR-0010 rule 4) from an
  untrusted account are folded into the next commit: the write is made, the reply `Done { count }` waits for it
  (review Q3, finding 4: a one-byte `O_SYNC` loop is the same stall as a `sync` loop). Each refusal at an account's
  bound is counted for its launcher (`Service::account_refusals`).
- **Evidence for:** No change for existing clients; one number for the launcher to choose; the sums are exact, so
  INV-0262-3 is an equality the model test checks after every step.
- **Evidence against:** One more request; a client at the held bound loses a connection instead of getting a status
  (a reply that was already made has no room to carry one).
- **Cost:** *(measured)* about 300 lines in `zoraya-fs-service`, 20 in the protocol and client.

### Option B: change `Clone` to carry the budget (0 meaning none)
- **Evidence against:** Every sender changes in one step, and a stale client sends a short message the service now
  refuses. Not taken (driver 1).

### Option C: three explicit numbers in the request
- **Evidence for:** The launcher can choose each bound. **Evidence against:** A launcher can ask for combinations the
  table does not allow, and the service must validate each against the column; nothing in stage 8 needs it.

## Recommendation
**Recommendation (not a decision): Option A.**

## Consequences
- **Positive.** Existing clients unchanged; INV-0262-1 to -3 hold for `fsd` and are checked on the host.
- **Negative.** The session's held-reply bound is `MAX_OPEN` (896), so the service's worst case is dominated by 896
  held replies of up to 64 KiB, about 56 MiB (`zoraya-fs-service`'s `worst_case`); it was the same before accounts.
  Lowering the session's bound (for example to 64, 4 MiB) is a change to what a slow trusted client can do and
  needs the architect.
- **Follow-up.** The launcher of an untrusted room (Q6a) calls `CloneAccount` once per `fsd`; `top` reads the
  account's counts (`Service::account_totals` is the host form).

## Invariants
- **INV-0291-1.** Account limits, summed over the live accounts, equal the service's bounds (`MAX_OPEN`,
  `MAX_BUFFERS`, `MAX_HELD`) after every request. *Check:* `zoraya-fs-service`'s `tests/service.rs`,
  `random_accounts_keep_the_sum_of_bounds_and_free_their_state`.

## Open questions
- How the launcher reads the refusal count (a request, or the supervisor channel of ADR-0269): Q6a. Today it is a
  host getter.
- Sub-accounts are bounded by the handles of their parent, not by a count (worst case 72 B each).
- Whether the held-reply bound of the session should be lowered (see Consequences).
- Who may carve: today any connection with `CLONE`. An untrusted program can only subdivide its own account.

## Revisit triggers
- A second service needs the same request (`netd`, ADR-0262 item 1): move the account table to a shared crate.

## Architect's decision

(Left empty for the architect.)
