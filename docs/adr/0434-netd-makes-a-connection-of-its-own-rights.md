# ADR-0434: `netd` makes a connection of a connection's own rights and scope (plan part F4)

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0220, ADR-0262, ADR-0287, ADR-0310, ADR-0411, ADR-0433
- **Evidence:** CP-05, GR-005

## Context

ADR-0411 item 1 lets a parent give its child "the parent's network connection, shared" (`POSIX_SPAWN_GRANT_NET`). A POSIX
program holds one connection to `netd`, narrowed by the shell to the host and port of its line (ADR-0287 item 2), and the
library does not know that connection's rights or scope. `netd`'s `Narrow` (ADR-0220 item 5) needs the scope and the right
`narrow`, which the shell's narrowed connection for a program does not have. So a program has no way to make a second
connection that is as wide as its own and no wider. Plan part F4 needs one. **Measured:** `cargo test -p zoraya-net`.

## Decision drivers

1. **A grant is never wider than what the giver holds** (CP-05, GR-005).
2. **The library cannot state a scope it does not know**, and must not widen one by guessing.
3. **Small change to the service and no change to what it already answers.**

## Considered options

### Option A: a message `Duplicate` (ordinal 8) that makes a connection like the caller's

One handle (the server end), no fields; the new connection has the caller's rights and encoded scope and joins its account.

### Option B: the program keeps its scope and rights and asks `Narrow` with them

Needs the bootstrap to carry them, a record change and a copy of what the service already knows; and the right `narrow` is
missing in the shell's connection. Rejected.

### Option C: a spawn can only move the connection (`MOVE_NET`)

No change to `netd`; a program with two helpers that both need the network cannot do it. This is what ADR-0411 item 3 does for the
network process; `GRANT_NET` would then be `EACCES` always.

## Recommendation

**Recommendation (not a decision): Option A.** Message 8, `Duplicate` (a request with one handle), answered by `Duplicated` with
a status: `OK` and the new connection attached, or `BUSY` when `netd` has no room for another connection (the bound of
ADR-0222). Any connection may ask: the new one is a copy, so it has no right and no scope the asker lacked, and it joins the same
account, so the budget of ADR-0262 counts it as it counts a `Narrow`. A request without its handle ends the connection, as
`Narrow` does. Nothing else of `netd` changes.

## Consequences

- **Positive.** `GRANT_NET` gives a child a connection as wide as the parent's.
- **Negative.** A program can fill `netd`'s connection table by asking; the bound is `netd`'s (the same as for a shell that narrows
  in a loop) and each connection counts in the program's account.
- **Follow-up.** A per-connection bound on duplicates, if a program abuses it.

## Invariants

- **INV-0434-1.** The connection `Duplicate` makes has exactly the rights and the scope of the connection that asked, and the
  request is refused only by `BUSY` at the table's bound. *Check:* `cargo test -p zoraya-net` (`duplicate_gives_the_same_rights_and_scope_and_nothing_more`).

## Open questions

- Whether `Duplicate` should be limited per connection as the random service's `Share` is (ADR-0435).

## Revisit triggers

- A program that duplicates its connection in a loop.

## Architect's decision

(Left empty for the human: accepted / changed / rejected, date, notes.)
