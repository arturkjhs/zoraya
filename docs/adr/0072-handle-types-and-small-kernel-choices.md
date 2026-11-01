# ADR-0072: Handle types in protocols, and small choices stage 4 made

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0050, ADR-0051, ADR-0052, ADR-0054, ADR-0070
- **Evidence:** CP-11, DM-053

## Context
Building ADR-0050 to ADR-0055 met points where the accepted text is silent or asks for something the kernel cannot do. ADR-0052 says handles are checked "in declared order, with their object types", but no call reports a handle's type, and a total decoder runs on bytes and a count. The TASK-12 review also listed choices made with no ADR: what `dma_unpin` does to its handle, whether `memory_unmap` can cut a mapping, how many objects a line has, whether a size may be a part of a page, and how the time limit of ADR-0070 adds up. They depend on each other only in that each was taken while building one stage, and each is small, so they are one record. Nothing here is measured.

## Decision drivers
1. A decoder stays total and needs no system call (ADR-0052).
2. An untyped surface is the failure DM-053 records, and CP-11 asks for a type at every boundary.
3. The smallest call set that the accepted ADRs need.

## Considered options

### Option A: No check, as `ioctl` passes a descriptor (Linux)
- **Description:** A server uses a handle and sees what happens.
- **Evidence for:** Nothing to build, and the kernel refuses a wrong type at first use with `BadHandle`.
- **Evidence against:** The refusal comes late, perhaps after an effect (DM-053's shape).
- **Cost:** None.
- **Precedent:** Unix.

### Option B: A call that reports a handle's type, used by servers
- **Description:** `handle_type(handle)`, and a server checks each handle after the decoder, before any effect. Zircon has `zx_object_get_info` for this `[outside evidence]`.
- **Evidence for:** Meets ADR-0052's sentence.
- **Evidence against:** One more call, which is only used on a path that a wrong handle fails anyway.
- **Cost:** *(estimate)* 40 lines and a table of types.
- **Precedent:** Fuchsia.

### Option C: Check the count at the decoder and the type at first use, before any effect
- **Description:** The decoder checks how many handles came. Each method's first act with a handle is a call that needs that type, so a wrong one gives `BadHandle` before an effect.
- **Evidence for:** Drivers 1 and 3. No new call. Holds for what TASK-13 builds: `attach` maps, a token is compared, a `server_end` is subscribed.
- **Evidence against:** A rule for server authors, which review must keep, and a change to one sentence of ADR-0052.
- **Cost:** A sentence.
- **Precedent:** Option A, with an order added.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision. If the architect prefers Option B, it is cheap to add.

Accepting this ADR also records these, as built:

- **`dma_unpin` ends the pin and closes its handle.** A handle to an ended pin would have no use. This closes backlog W7.
- **`memory_unmap` ends one whole mapping.** The range must be exactly one mapping's, or it is `InvalidArgument`. Cutting a mapping would need a split record for what ADR-0051 does not ask.
- **One interrupt object per line.** A second is `BadState`, as for a device's one mapping.
- **`memory_create` takes whole pages.** Anything else is `InvalidArgument`, because rounding up would commit what was not asked for.
- **ADR-0070's 100 ms is per transport,** so up to 800 ms for a page of eight.

**Tensions with accepted ADRs.** *ADR-0052, Handles:* "with their object types checked" is replaced by Option C.

### Effect of the vision
For target 2 a hostile client is the normal case, and Option C holds only while each server keeps the rule. Option B can be added later with no change to the wire.

### Deferred to stage 6
Nothing here depends on POSIX. ADR-0055 holds the one requirement on stage 4.

## Consequences
- **Positive.** The choices are on record, and ADR-0052's text matches the code.
- **Negative.** A rule that a compiler does not check.
- **Follow-up.** TASK-13's service follows the rule.

## Invariants
- **INV-0072-1.** A message with the wrong number of handles is refused by its decoder. *Check:* `make test-unit`.
- **INV-0072-2.** `memory_unmap` of anything but a whole mapping changes nothing. *Check:* the in-QEMU test of `memory_unmap` with a wrong length and a wrong address.

## Open questions
- Whether a service's authors keep the rule: TASK-13's review.

## Revisit triggers
- A method that acts before it first uses a handle.

## Architect's decision
