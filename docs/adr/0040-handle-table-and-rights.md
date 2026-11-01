# ADR-0040: Handle table and rights

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0006, ADR-0032
- **Evidence:** CP-01, CP-02, DM-002, DM-003, DM-072, GR-001, GR-003, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#keykos--eros--capros-and-coyotos, alternatives-postmortem.md#l4-family-and-sel4

## Context
ADR-0032 fixes `Handle` as a `u32` with 0 never valid and left rights to stage 3. CP-02 asks for typed handles whose rights only decrease. ADR-0003 needs a child to find a handle at the slot its parent asked for (INV-0003-6). ADR-0003 rule 3 makes `Port` non-duplicable. Nothing here is measured: the sizes below are assumptions.

## Decision drivers
1. A stale handle must fail, never reach whatever now occupies the slot. That is the handle form of DM-003's reuse race.
2. Rights only decrease (CP-02); GR-003 records NT's `DuplicateHandle` sometimes raising access, which this must rule out.
3. One object, one right per power: DM-072 is one descriptor granting two unrelated powers.
4. Slots requested at `spawn` are the slots observed (INV-0003-6), so a child must be able to compute a startup handle's value from its slot.
5. A hostile process must not be able to grow kernel memory without limit through its table (CP-01 bounds authority, not memory; a limit does).

## Considered options

### Option A: Lowest free integer, no rights (Linux descriptors)
- **Description:** A per-process array of object pointers; the lowest free index is reused at once. Access mode lives on the open object.
- **Evidence for:** Simple and compact. Works for fifty years (GR-001). Slots are trivially requested.
- **Evidence against:** Reuse by design, so a double close or a stale number hits an unrelated object, the shape of DM-003. No rights mask (GR-001). Implicit inheritance grew around it (DM-002).
- **Cost:** The smallest table, and a permanent class of bug.
- **Precedent:** Unix, Linux. Linux added `pidfd` and close-on-exec later as retrofits (DM-002, DM-003).

### Option B: Slot index plus generation, with a rights mask
- **Description:** `raw = generation << 16 | slot`. A slot's generation rises on every close. A slot at the highest generation is retired and never reused. A startup handle has generation 0, so its value equals its slot. Each entry holds object, rights and a unique id.
- **Evidence for:** Stale handles fail by construction (driver 1). Value equals slot at startup (driver 4). Rights are a mask fixed in the handle, as in Zircon (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** A retired slot is lost for the process's life. 16 bits of slot limit the table.
- **Cost:** *(estimate)* 150 lines in a host-tested crate. Lookup is an array index.
- **Precedent:** Zircon handles carry rights and reduce on duplicate. Generation counters are common in slot maps.

### Option C: Random 32-bit handle values, hashed (Zircon style)
- **Description:** Values are randomised per process and looked up in a table.
- **Evidence for:** Guessing a live handle is hard, so stale use mostly fails. Zircon ships this (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** Failure of a stale handle is probabilistic, not a rule a test can state. INV-0003-6 then needs a separate startup-handle table.
- **Cost:** *(estimate)* A hash table and a random source in the kernel.
- **Precedent:** Zircon.

### Option D: Kernel-managed capability space (seL4 CNodes, KeyKOS nodes)
- **Description:** Capabilities live in tree-structured nodes the process builds from typed memory.
- **Evidence for:** No kernel allocation on the path; authority over memory is itself a capability (`alternatives-postmortem.md#l4-family-and-sel4`, `alternatives-postmortem.md#keykos--eros--capros-and-coyotos`).
- **Evidence against:** Large user-visible machinery that no measured program needs, and one that the POSIX personality would have to hide.
- **Cost:** *(estimate)* The largest kernel and library cost here.
- **Precedent:** seL4, KeyKOS.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Representation.** `Handle` stays a `u32`: bits 0 to 15 the slot, bits 16 to 31 the generation. Slot 0 does not exist, so 0 stays invalid. `MAX_HANDLES` is 1024 slots `[assumption]`.
- **Allocation.** Slots never used first, then next-fit from a cursor, so a closed slot is not reused soon. `spawn` installs at the requested slot, with generation 0, in a table that is otherwise empty.
- **Exhaustion.** A full table gives `LimitReached`, and the call has no other effect. A slot whose generation reaches 65535 is retired: a stale handle can then never match.
- **Rights.** `DUPLICATE`, `TRANSFER`, `WAIT`, `SIGNAL`, `CREATE_PROCESS`, `KILL`, `RESUME`, `READ`, `WRITE`, `LOOKUP`.

| Object | Rights it can carry | Calls |
|---|---|---|
| Process | `DUPLICATE`, `TRANSFER`, `WAIT`, `SIGNAL`, `RESUME` | `port_subscribe` and exit record: `WAIT`; `process_signal`: `SIGNAL`; `process_resume`: `RESUME` |
| Job | `DUPLICATE`, `TRANSFER`, `WAIT`, `SIGNAL`, `CREATE_PROCESS`, `KILL` | `spawn`, `job_create`: `CREATE_PROCESS`; `job_signal`: `SIGNAL`; `job_kill`: `KILL` |
| Port | `TRANSFER`, `WAIT`, `WRITE` | `port_wait`: `WAIT`; `port_subscribe`, `port_unsubscribe`, `port_queue`: `WRITE` |
| Console | `DUPLICATE`, `TRANSFER`, `WRITE` | `console_write`: `WRITE` |
| Directory | `DUPLICATE`, `TRANSFER`, `LOOKUP` | `directory_lookup`: `LOOKUP` |
| Image | `DUPLICATE`, `TRANSFER`, `READ` | `spawn`: `READ` |

*Partly replaced by ADR-0060 and ADR-0062 (architect's decision, 2026-11-01):* the console also has `READ` and `WAIT`, with `console_read` and `console_dropped` needing `READ`, and it is waitable. There is a new right `POWER` and a new object type `Power`, whose call `power_off` needs it. ADR-0061 adds `directory_entry`, which needs `LOOKUP`.

- **Duplication.** `handle_duplicate(handle, rights)` needs `DUPLICATE`, and `rights` must be a subset of what the handle holds, or it fails with `AccessDenied`. It never silently narrows a request: asking for more is an error, so a bug shows. Unknown bits are `InvalidArgument`.
- **Transfer.** Only as part of `spawn` at this stage. A move needs `TRANSFER`, a duplicate needs `DUPLICATE`, and rights passed are a subset, with the same error. Moving a handle ends its subscriptions, as closing does: a subscription belongs to the handle in its process.
- **Closing.** `handle_close` ends the handle at once and removes its subscriptions. No call fails on an open handle's behalf later: there is no revocation.
- **Port.** A `Port` handle never has `DUPLICATE` (ADR-0003 rule 3), so there is at most one handle to a port and closing it closes the port.

## Consequences
- **Positive.** Stale handles fail deterministically. Every right is checked in one place per call. Startup slots need no second table.
- **Negative.** A table of 1024 is a real limit on a server. A long-lived process retires a slot after 65,535 uses of it; next-fit spreads that across the table.
- **Follow-up.** Revocation (CP-02's open question). Rights for channels and memory objects at stage 4. Per-type duplicability for `snapshot_process` at stage 6.

## Invariants
- **INV-0040-1.** A closed handle's value never names an object again in that process. *Check:* `cargo test -p zoraya-handle`, including a slot driven through every generation to retirement.
- **INV-0040-2.** No operation yields a handle with a right its source lacked. *Check:* `zoraya-handle` host tests over all rights masks, and an in-QEMU test that every call refuses a handle lacking its right.
- **INV-0040-3.** A handle table never holds more than `MAX_HANDLES` entries, and a full table gives `LimitReached`, never a panic. *Check:* a host test, and an in-QEMU test that exhausts a table.

## Open questions
- Whether 1024 is enough for a filesystem service with many clients: stage 4 measures.
- Revocation, handle-level or object-level (CP-02).

## Revisit triggers
- A service that needs more than 1024 handles.
- A need to reduce rights on a live handle.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
