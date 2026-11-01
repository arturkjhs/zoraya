# ADR-0045: Port keys and user packet data as opaque payloads

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0006, ADR-0035
- **Evidence:** CP-01, CP-06, CP-11, DM-023

## Context
INV-0035-1 says the only integer payloads of a native call are `detail` fields and `ExitRecord.code`. ADR-0006 rule 2 gives `port_subscribe` a `key` and user packets their `data`: integers the kernel stores and hands back. Both ADRs are accepted, so the invariant cannot be true of the ABI they require. This is the same conflict ADR-0035 resolved for `code`. Request kinds and `Custom(n)` are not payloads: they are enumerations, which the allow-list treats as flags.

## Decision drivers
1. The point of the rule is that no integer names an object (CP-01, CP-06).
2. A key the kernel never interprets names nothing. A key is how a waiter tells its sources apart, and DM-023 is what happens when that identity is tied to the descriptor instead.
3. The record should stay small.

## Considered options

### Option A: Widen the allow-list by name
- **Description:** A new invariant replaces INV-0035-1, adding `key` and packet `data` by name.
- **Evidence for:** Honest, and the lint stays strict. ADR-0035 did the same.
- **Evidence against:** A third wording of one invariant.
- **Cost:** A line in the lint.
- **Precedent:** Zircon's port keys are opaque 64-bit values `[outside evidence: zx_port_packet_t]`.

### Option B: Keep the rule and make the key a handle
- **Description:** A packet names its source by handle.
- **Evidence for:** No new payload.
- **Evidence against:** A closed and reused handle value confuses a waiter, and ADR-0006 rule 2 fixes `key` as the API. It needs a superseding ADR-0006.
- **Cost:** Larger than the fix.
- **Precedent:** Linux epoll's fd-tied identity (DM-023), as a warning.

### Option C: Pass keys only through a buffer
- **Description:** A key travels in a user-memory structure, outside the lint's reach.
- **Evidence for:** No rule change.
- **Evidence against:** It defeats the lint, and a typed parameter is the point (CP-11).
- **Cost:** A copy per call, and a dishonest check.
- **Precedent:** None.

## Recommendation
Recommended: **Option A**. It is a recommendation, not a decision.

- INV-0045-1 replaces INV-0035-1 when this ADR is accepted, and INV-0035-1's row is marked as replaced.
- `Param` gains `Opaque`, for the two named uses. The lint allows it only for the keys of `port_subscribe`, `port_unsubscribe` and `port_queue`, and `port_queue`'s data.

## Consequences
- **Positive.** The invariant can be true and checked at stage 3.
- **Negative.** One more link in the invariant's history.
- **Follow-up.** None.

## Invariants
- **INV-0045-1.** Every object a native syscall acts on is designated by a `Handle` parameter; every path parameter has the `abi` type `RelativePath`, whose constructor rejects a leading `/`; the only integer payloads are `detail` fields, `ExitRecord.code`, port subscription keys and user packet data, each documented as opaque to the kernel (ADR-0004, ADR-0006). *Check:* a lint over the `abi` crate's syscall signatures that allows parameter types only from an allow-list (`Handle`, `RelativePath`, `Deadline`, flags, sizes, buffers, `detail: u32`, `code: i32`, `Opaque` for `port_subscribe`, `port_unsubscribe` and `port_queue`), plus a unit test that `RelativePath::new("/x")` fails.

## Open questions
- None.

## Revisit triggers
- Another call that needs an integer payload.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
