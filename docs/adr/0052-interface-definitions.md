# ADR-0052: Interface definitions and wire format

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0012, ADR-0044, ADR-0050
- **Evidence:** DM-023, CP-11, CP-13, DM-053, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, stage-4/sources.md#interface-definitions

## Context
Services need a way to state their protocols. DM-053 is what untyped interfaces become, and CP-11 forbids an untyped `invoke(handle, op, bytes)` unless an IDL types the bytes. ADR-0044 already generates the system call ABI from one Rust declaration, and left open whether IPC should share it. Decoders of hostile input follow ADR-0012's discipline.

## Decision drivers
1. Every message from a peer is untrusted and decoded totally (ADR-0012 rule 4).
2. No untyped escape hatch (DM-053).
3. A protocol can gain methods without breaking old peers (CP-13).
4. Few moving parts: no external dependency in `no_std` code without a reason, and a Rust-only system at this stage.
5. Other languages must remain possible: the wire must be describable without Rust.

## Considered options

### Option A: `ioctl`-style codes and hand-written structs (Linux baseline)
- **Description:** A number and a buffer per operation, with each side coding its own.
- **Evidence for:** No tooling.
- **Evidence against:** DM-053. Its real strength is one entry point and no version negotiation, and that is what makes it unreviewable.
- **Cost:** Every interface pays in review.
- **Precedent:** Linux `ioctl`.

### Option B: A separate IDL and compiler (FIDL)
- **Description:** A language for protocols, compiled to bindings per language.
- **Evidence for:** Language-neutral, canonical encoding, required validation (`stage-4/sources.md#interface-definitions`), in production (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** A compiler, versioning rules and a runtime to build (CP-11's cost) for a system with two protocols.
- **Cost:** *(estimate)* Several thousand lines.
- **Precedent:** FIDL.

### Option C: An existing format (Cap'n Proto)
- **Description:** Adopt a schema language and its encoding.
- **Evidence for:** Mature; a capability table in the design; reading needs no decoding pass (`stage-4/sources.md#interface-definitions`).
- **Evidence against:** Relative pointers with lazy validation, which needs a traversal budget and depth limit (`stage-4/sources.md#interface-definitions`), against driver 1's all-at-once decoder. An external generator, and a `no_std` dependency to justify.
- **Cost:** *(estimate)* An audit of a large dependency.
- **Precedent:** Its own project.

### Option D: Rust declarations as the schema, flat fixed layouts
- **Description:** A `protocol!` macro, in the style of ADR-0044's `syscalls!`, declares each method's fields, handles and reply. It generates encoders, total decoders, a server trait and client stubs.
- **Evidence for:** One declaration, type-checked on both sides. Host-testable. Drivers 1, 2 and 4.
- **Evidence against:** Rust only. A macro is a poor IDL if protocols get complicated.
- **Cost:** *(estimate)* 300 lines of macro.
- **Precedent:** ADR-0044.

## Recommendation
Recommended: **Option D**, with the wire format specified independently of the macro. It is a recommendation, not a decision.

- **Header.** 16 bytes, little-endian: magic-and-version `u32`, ordinal `u32`, transaction id `u32`, body size `u32`. A transaction id of 0 means no reply is expected. No pointers, as in FIDL's encoded form.
- **Body.** Fixed fields in order, with natural alignment and zero padding that is checked, then bounded variable parts in declared order: byte arrays, strings, or sequences of declared records, each with a declared maximum and decoded whole by generated code. No variable part is opaque when its contents have structure, so `rename`'s two names, `IsLink`'s path and target (ADR-0013 rule 7) and `read_directory`'s entries are all typed. Enumerations are range-checked. Strings are UTF-8, and names satisfy ADR-0013.
- **Handles** are out of band, in the message's handle list, in declared order, with their object types checked. A message with extra or missing handles is refused and its handles closed.
- **Decoding.** Whole message, once, before any effect; total; `#![forbid(unsafe_code)]`. Reference: `zoraya-abi`'s `SpawnRequest::decode`.
- **Evolution.** A new method is a new ordinal. An unknown ordinal gets one generic error reply, whose layout is the same for every protocol and holds a status; any attached handles are closed, and the connection stays. With a transaction id of 0, no reply. A changed method is a new ordinal too: no optional fields and no tables. The magic-and-version changes only for a new header.
- **One crate per protocol**, so a client links only what it speaks.
- **Syscalls stay apart:** ADR-0044's declaration describes registers, this one bytes and handles. The answer to its open question is two macros of one style.

### Effect of the vision
Target 3 favours Option B, and so do language runtimes under target 2 if they speak protocols directly; through `libposix` they do not. Moving later costs a compiler, but the wire is specified without Rust, so a generator for another language can target the same bytes, and nothing on the wire changes. For target 2 a hostile client is the normal case, and total decoding is what is wanted. Networking does not change it: this format is local, and a network protocol has its own.

### Deferred to stage 6
How C programs reach services through `libposix`. A new method is a new ordinal, so stage 6 adds what it needs without changing a stage 4 method. ADR-0055 holds the one requirement on stage 4: its design must not preclude an open file description with a shared position (DM-023).

## Consequences
- **Positive.** No tool to build. A protocol is a page of Rust. Decoders are host-fuzzed like the ramdisk's.
- **Negative.** New fields mean new ordinals, so old methods linger. No language but Rust gets bindings for free.
- **Follow-up.** A wire-format document kept next to the macro.

## Invariants
- **INV-0052-1.** Every decoder is total: no input panics or reads out of bounds. *Check:* `#![forbid(unsafe_code)]`, and host tests with truncations and random mutations for every message.
- **INV-0052-2.** Encoding then decoding gives the same value, and any nonzero padding or trailing byte is refused. *Check:* generated round-trip test per message, and a mutation test.
- **INV-0052-3.** No method's body has a free-form "operation" field. *Check:* the macro accepts only declared kinds, which a lint over its table confirms; whether a byte array carries operations in disguise is left to review.
- **INV-0052-4.** An unknown ordinal gets `Unsupported`, and the connection survives. *Check:* in-QEMU test against a service.

## Open questions
- When a second language needs bindings.

## Revisit triggers
- A protocol the macro cannot express.
- A non-Rust client that must speak a protocol directly.

## Architect's decision
