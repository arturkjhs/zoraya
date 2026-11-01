# ADR-0044: The dispatcher and the ABI table

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0032, ADR-0035
- **Evidence:** CP-11, DM-053, DM-120

## Context
ADR-0032's open question: `SYSCALLS` is written by hand, apart from the dispatcher, so lints over it prove little. Either the dispatcher is built from the table, or a test must show that the two match. Stage 3 also adds some twenty calls and a user library that must agree with both.

## Decision drivers
1. The table the lints read must be the one the kernel runs (INV-0032-1).
2. A mismatch in arity or type between kernel, table and user wrapper must not compile.
3. Argument decoding is in one place and strict (CP-11): DM-120 is one multiplexed call with accreted operations, DM-053 an untyped one.

## Considered options

### Option A: A hand-written `switch` and a unistd table (Linux)
- **Description:** A table of function pointers and a separately kept library.
- **Evidence for:** The simplest thing, and it works at scale with discipline.
- **Evidence against:** Nothing connects it to the lints (driver 1).
- **Cost:** A test to compare them, which needs its own reflection.
- **Precedent:** Linux.

### Option B: A test comparing a hand-written dispatcher with the table
- **Description:** Keep both; a test calls each number and checks that it is handled.
- **Evidence for:** No macro.
- **Evidence against:** It proves a number is handled, not that its arguments are read as the table says (driver 2).
- **Cost:** Low, with a weak guarantee.
- **Precedent:** None better in the files.

### Option C: One macro declaration in `zoraya-abi` generates everything
- **Description:** A single `syscalls!` list of typed signatures generates the `Syscall` enum, `SYSCALLS`, a `Handlers` trait the kernel must implement, a `dispatch` function that decodes registers by type, and typed user-side stubs.
- **Evidence for:** All three views come from one list (drivers 1 and 2). Decoding is host-tested. No build tool is needed.
- **Evidence against:** A macro to read. Generating the ABI this way is a small IDL (CP-11's cost).
- **Cost:** *(estimate)* 100 lines of `macro_rules!`.
- **Precedent:** Zircon generates its syscall bindings from one definition `[outside evidence: Fuchsia kazoo tool]`.

## Recommendation
Recommended: **Option C**. It is a recommendation, not a decision.

- **Types.** Each parameter is a type with an `Arg` impl giving its `Param` kind, its register count, and its strict decoder: a value too wide for its type is refused, never truncated. `SYSCALLS[..].params` comes from these, so the lints run over the same list the kernel decodes.
- **Kernel.** `syscall.rs` implements `Handlers`. A missing or differently typed call does not compile.
- **Lints.** INV-0035-1 and its successor, INV-0004-1 and INV-0006-2 stay unit tests in `zoraya-abi`, over `SYSCALLS`, which is now the source of truth.
- **User library.** `zoraya-rt` wraps the generated stubs. It has no system call numbers of its own.

## Consequences
- **Positive.** ADR-0032's open question closes. INV-0032-1 becomes true by construction for numbers and arguments.
- **Negative.** Adding a parameter kind means an `Arg` impl and a judgement in the lint, which is the intent.
- **Follow-up.** None.

## Invariants
- **INV-0044-1.** `SYSCALLS`, `Syscall`, the kernel's `Handlers` and the user stubs come from one declaration. *Check:* the kernel builds only if it implements the generated trait, and a `zoraya-abi` test sends every number through `dispatch` and asserts it reaches the handler of the same name.
- **INV-0044-2.** A register value too wide for its parameter type is refused, never truncated. *Check:* `zoraya-abi` unit test for every parameter type.

## Open questions
- Whether the same declaration should describe IPC (CP-11's open question). TASK-10 decides.

## Revisit triggers
- A call that the macro cannot express.

## Architect's decision
