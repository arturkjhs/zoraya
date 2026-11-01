# ADR-0057: A 64-bit handle with no retired slots

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0032, ADR-0040, ADR-0042, ADR-0044, ADR-0050, ADR-0052, ADR-0054, ADR-0055
- **Evidence:** CP-13, DM-003, DM-023, GR-001, alternatives-postmortem.md#hubris, stage-4/sources.md#handles

## Context
ADR-0040 makes `Handle` a `u32` of 16 bits of slot and 16 of generation, and retires a slot whose generation reaches 65535. With 1024 slots, a process can pass about 67 million handles through its table before the table is gone. Stage 3's programs never come close. ADR-0055's service does: every `open` passes one handle, and a message of 64 handles passes 64 whether or not it is then refused. The third review of TASK-10 (T5) found that one client could end a volume's table in about a million messages, by arithmetic and not by measurement, with few handles open at any moment. ADR-0040 names the cost for "a long-lived process" and its revisit triggers name a service that needs more handles. The handle is ABI, so widening it costs least now (CP-13). Measured: nothing on this point.

## Decision drivers
1. A closed handle's value must not name a new object (INV-0040-1): DM-003 and GR-001 state the reuse race, and DM-023 records what reused numbers cost.
2. A long-lived process must not wear out its table.
3. Early ABI shapes are permanent (CP-13). No code outside this repository depends on the handle yet.
4. A handle fits one register (ADR-0032) and is checked in constant time.

## Considered options

### Option A: Keep ADR-0040, and defend the service
- **Description:** A read flag refuses a message by its handle count before any handle is installed. The service watches the table's retired count and reports to its supervisor while room remains, and `init` restarts it at a planned moment.
- **Evidence for:** No ABI change, and it is the cheapest now. Refusing before installing is useful in its own right, because it costs a hostile message nothing.
- **Evidence against:** It treats the symptom. Every `open` still burns a value, so a service must restart on a schedule, and ADR-0054 says running clients are not recovered from a restart. Every long-lived process needs the same care, and driver 2 is not met.
- **Cost:** *(estimate)* 100 lines, then a restart path that clients feel.
- **Precedent:** None in the files.

### Option B: 64 bits, 48 of generation, no retirement
- **Description:** `Handle` is a `u64`: bits 0 to 15 the slot, bits 16 to 63 the generation. A slot's generation rises on each close and wraps. No slot retires.
- **Evidence for:** Drivers 2 to 4, and driver 1 within the system's working life, not absolutely (INV-0057-1). At one close per microsecond on one slot, about the speed of `port_wait` under HVF (875 ns), a value returns after about 8.9 years, and next-fit (ADR-0040) spreads closes over the table, so about 9,000 years for a full cycle of 1024 slots `[arithmetic]`. The slot field is already 16 bits, so `MAX_HANDLES` can rise later with no ABI change.
- **Evidence against:** Handles in structures double in size: `spawn`'s buffer and messages. Rights stay in the table, so nothing else is gained from the extra bits. A value can return in principle, after the times above. A variant keeps retirement at the last 48-bit generation, which keeps INV-0040-1 word for word, at the price of a rule and a state that would never run. The architect asked for no retirement, so wrapping it is.
- **Cost:** *(estimate)* A day's change across the crates below.
- **Precedent:** Generation-tagged ids in Hubris (`alternatives-postmortem.md#hubris`).

### Option C: 32 bits, redivided as 11 of slot and 21 of generation
- **Description:** Stay a `u32` and give the generation the 5 bits that slots 1 to 1024 do not use. Retirement stays.
- **Evidence for:** No change to any structure, and a small change to code. It gives 32 times the lifetime.
- **Evidence against:** About 2.1 billion closes in all, which a busy service spends in hours to days `[estimate]`, so driver 2 is not met, only delayed. It also fixes `MAX_HANDLES` at 1024 in the ABI, which ADR-0055's limit may need raised.
- **Cost:** *(estimate)* An hour.
- **Precedent:** None in the files.

### Option D: Small integers reused at once (Linux file descriptors)
- **Description:** The lowest free number, with no generation.
- **Evidence for:** Simple, and `dup2` and POSIX depend on the shape. Nothing can wear out.
- **Evidence against:** Against driver 1: a stale number names a new object, which is DM-023's failure for closed and reused descriptors.
- **Cost:** Low in code, high in bugs of that kind.
- **Precedent:** Unix.

### Option E: 32-bit values the kernel may reuse (Zircon)
- **Description:** A handle is any 32-bit number, the kernel may reuse the value of a closed handle, and a job policy detects bad use (`stage-4/sources.md#handles`).
- **Evidence for:** Nothing wears out, in production, with a detection tool.
- **Evidence against:** Driver 1 is given up as a guarantee, and the page warns about racy close and use. ADR-0040 chose otherwise.
- **Cost:** *(estimate)* An hour, and a policy.
- **Precedent:** Fuchsia.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision. Option A's refusal before installing is worth having on its own, but it does not belong here.

- **Representation.** This replaces ADR-0040's Representation: `Handle` is a `u64`, bits 0 to 15 the slot and bits 16 to 63 the generation. Slot 0 does not exist, so 0 stays invalid. `MAX_HANDLES` stays 1024.
- **Exhaustion.** This replaces the second sentence of ADR-0040's Exhaustion: no slot is retired. A generation wraps, in the times above.
- **ADR-0042.** This replaces its versioning rule once: the magic and the width change together, before any code outside this repository exists, and no older size is accepted. The rule holds again from `ZSP2` on.
- **ADR-0032.** Its `Handle` becomes a `u64`, under its revisit trigger.
- **ADR-0040's absolute wording.** This also replaces its driver 1, "A stale handle must fail, never reach whatever now occupies the slot", and its Consequences sentence "Stale handles fail deterministically". Both now hold within INV-0057-1's bound, not absolutely. ADR-0040 carries a note saying so.
- **Everything else in ADR-0040 stands,** except its Consequences sentence on retiring a slot, which goes with the rule. Rights, allocation, a full table giving `LimitReached`, and `spawn` installing with generation 0.
- **Registers.** A handle is one register. Since every bit pattern is now a generation, a width check has nothing left to refuse. The table's lookup stays the check.
- **On acceptance,** INV-0057-1 replaces INV-0040-1, and its row is marked as replaced.

### Effect of the vision
None on the choice, for any target. Target 2 is what makes a table worn out on purpose an attack, and this removes it as such. Threads and networking are unaffected. Changing later would break every program, which is the reason to do this before stage 4 adds formats that hold handles.

### Deferred to stage 6
Nothing here depends on POSIX. A file descriptor stays a `libposix` number that maps to a handle, so `dup2`'s demands do not reach the handle. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** A process's table does not wear out. ADR-0055's tension with ADR-0040 goes, and a service needs no scheduled restart. `MAX_HANDLES` can rise with no ABI change.
- **Negative.** Larger `spawn` buffers, messages and table entries. Stage 3's binaries and their tests must change together, once.
- **`zoraya-abi`.** `Handle` wraps a `u64`, and `new`, `raw`, `from_parts` and `generation` change type. `CONSOLE`, `JOB` and `DIRECTORY` keep their values. The `Handle` argument decoder stops using the 32-bit width check, and INV-0044-2's test changes for that parameter type. In `spawn.rs` the job, image, group and entry handle fields become 8 bytes, so `HEADER_SIZE`, `ENTRY_SIZE`, the field offsets and the alignment change, and the magic becomes `ZSP2` (ADR-0042). The encoder and the decoder tests follow.
- **`zoraya-handle`.** `Slot.generation` becomes a `u64` and wraps. `is_retired`, `retired()` and the rule in `is_free` go. The retirement tests, `a_slot_is_retired_at_its_last_generation` and the one that drives a slot to retirement, are replaced by tests of INV-0057-1 and -2.
- **Kernel.** Generated handlers follow the type. `kernel/src/object.rs` and `syscall.rs` change wherever a handle is stored or logged as a `u32`. The per-process table grows by 6 bytes a slot at most, charged to nothing until ADR-0056.
- **`zoraya-rt`.** Stubs are generated, so they follow. The hand-written `handle()` helper, which decodes every returned handle, changes.
- **Stage 3 tests.** `make test` must pass unchanged in meaning. `user/src/bin/t_handles.rs`, `t_spawn.rs`, `fuzz.rs`, `badptr.rs` and `child.rs`, whose `Handle::new` takes a `u32`, need review for values written as `u32`. `kernel/src/testing.rs` and `crates/zoraya-abi/src/tests.rs` likewise. `docs/design.md`'s handle section changes.
- **Other ADRs.** ADR-0050's handle lists and ADR-0052's out-of-band list hold 8-byte values. ADR-0042's format version rises.
- **Follow-up.** Done before stage 4 adds any format holding a handle.

## Invariants
- **INV-0057-1.** A slot's generation does not repeat within the system's working life, so within that life a closed handle's value does not name an object again. The bound: a repeat needs 2^48 closes of one slot, about 2.8 × 10^14. At 875 ns a close, the `port_wait` median under HVF, used here as an upper bound on the rate because a close is also a system call, that is about 7.8 years of one process doing nothing but closing one slot with a table otherwise full, and about 8,000 years when next-fit spreads closes over 1024 slots. A process's table dies with it, so the count starts again. This is an arithmetic bound, not an absolute rule: a process that runs longer than that, doing only that, would see a repeat. *Check:* `zoraya-handle` host tests, including a slot set near the 48-bit limit by direct access and driven past it, showing that generations rise by one until the wrap.
- **INV-0057-2.** A slot is reusable at every generation: no table loses a slot by use. *Check:* the same test, asserting that the table's free count is unchanged after the wrap.
- **INV-0057-3.** Handle values in the `spawn` buffer and the syscall ABI have one width, 64 bits. *Check:* `zoraya-abi` round-trip test with a generation above 32 bits, through `Encoder`, `SpawnRequest::decode` and the `Arg` encoding.

## Open questions
- Whether `MAX_HANDLES` should rise for services, now that the ABI allows it: measure at stage 4.

## Revisit triggers
- A handle stored in a structure where 8 bytes matters, measured.
- A machine fast enough to make the wrap time a concern.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect, on the condition of 0 errors in the changed lines of the sixth review, which `docs/research/stage-4/review-notes.md`, section "Sixth review, repeated", records.
