# ADR-0042: Passing structures to system calls

- **Status:** accepted
- **Date:** 2026-11-01
- **Depends on:** ADR-0003, ADR-0004, ADR-0031, ADR-0032
- **Evidence:** CP-11, CP-13, DM-008, DM-055, DM-160, X-8, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context
ADR-0032 left this open: `spawn` has more parameters than registers. ADR-0031 reaches user memory only through a checked copy. CP-11 asks for sized structs, rejected unknown flags, and a single copy-in.

## Decision drivers
1. Copy in once and work on the private copy: DM-160's double fetch cannot exist then.
2. Unknown flags and nonzero reserved fields are refused (DM-055).
3. Argv and environment are vectors, not a string (DM-008, INV-0003-3).
4. The decoder is total, runs on the host, and has no `unsafe`.
5. A later version can add fields (CP-11, CP-13).

## Considered options

### Option A: Nested user pointers (Linux `execve`, `clone3`)
- **Description:** A struct of pointers to vectors of pointers to strings, each read by the kernel as it goes.
- **Evidence for:** Familiar, and the caller builds nothing special. `clone3` shows size-based versioning works.
- **Evidence against:** Many separate fetches over memory another thread can change, the setting of DM-160. Each level needs its own limit and fault path. It cannot be decoded on the host.
- **Cost:** *(estimate)* Small to write, large to review.
- **Precedent:** Linux.

### Option B: One flat, self-contained buffer
- **Description:** The caller encodes everything into one byte buffer with a sized header, and passes a pointer and length. The kernel copies it once, then a total decoder in `zoraya-abi` validates it. No pointer inside.
- **Evidence for:** One fetch by construction (driver 1). Decoding is ordinary host-tested code (driver 4), the approach of ADR-0012's decoders. Zircon's channel messages show a bounded copy works (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`).
- **Evidence against:** A copy of everything, which matters for huge argv. The caller needs an encoder.
- **Cost:** *(estimate)* 200 lines in `zoraya-abi`, shared by both sides.
- **Precedent:** Zircon messages; IDL wire formats generally.

### Option C: A builder made of many calls (ADR-0003 Option D)
- **Description:** Create an empty process, then set each attribute with a call.
- **Evidence for:** Needs no struct at all, and each step is small.
- **Evidence against:** ADR-0003 rejected this as ABI: it exposes more surface and half-built processes.
- **Cost:** Many calls per spawn.
- **Precedent:** Zircon's process builder.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **Call.** `spawn(buffer, length)` returns a process handle. `length` is at most `MAX_SPAWN_SIZE`, 64 KiB `[assumption]`.
- **Layout.** All integers little-endian. Header: magic `ZSP1`, header size, flags (`START_SUSPENDED`), job, image, group (0 for none), `argc`, `envc`, handle count, a reserved zero, then one disposition byte per request kind. Then handle entries `{slot, handle, rights, mode}`. Then `argc + envc` strings, each a `u32` length and its bytes. Nothing may follow.
- **Versioning.** The header size says which version. This version accepts exactly its own size; a later one accepts older sizes and fills defaults. Unknown flags, a nonzero reserved field, and unknown modes or dispositions are `InvalidArgument`.
- **Validation.** `SpawnRequest::decode` checks everything before anything happens, and iteration after it cannot fail. `spawn` then checks the handles, with no effect on the parent until all pass.
- **Strings.** Byte strings: any bytes, with their length. UTF-8 is not required, since ADR-0002 leaves X-8 open, and DM-008 is why these stay vectors.
- **The child's view.** The kernel writes the argument and environment vectors into a read-only block at the fixed address `STARTUP_BLOCK`, in the same string format, because a process starts with zeroed registers (ADR-0031's test relies on that). The block is always mapped, even when both are empty.

## Consequences
- **Positive.** No pointer chasing, so one fault path and one copy.
- **Negative.** 64 KiB bounds argv plus environment plus handle list. Linux's limit is far larger; a toolchain may reach it.
- **Follow-up.** The same convention for future struct-taking calls.

## Invariants
- **INV-0042-1.** The kernel reads a `spawn` buffer from user memory exactly once. *Check:* `SpawnRequest::decode` takes `&[u8]`, so it cannot fetch, and `spawn` holds the only read of its buffer.
- **INV-0042-2.** `SpawnRequest::decode` is total: no input panics, and any input it accepts iterates without failure. *Check:* `cargo test -p zoraya-abi` with truncations and random mutations of a valid buffer.
- **INV-0042-3.** A refused `spawn` leaves the caller's handle table unchanged. *Check:* in-QEMU test where the last entry of a list is invalid and the moved handles are still usable.

## Open questions
- Whether 64 KiB is enough: measure argv at stage 6 with the toolchain's real command lines.
- Whether the limit should rise if the copy cost is acceptable (ADR-0031's open question).

## Revisit triggers
- A real command line that does not fit.

## Architect's decision

Accepted as recommended, 2026-11-01. Decided by the architect.
