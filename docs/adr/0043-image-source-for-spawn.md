# ADR-0043: Where `spawn` gets its image

- **Status:** proposed
- **Date:** 2026-11-01
- **Depends on:** ADR-0002, ADR-0003, ADR-0033
- **Evidence:** CP-01, DM-009, GR-005, alternatives-postmortem.md#fuchsia--zircon-incl-starnix, alternatives-postmortem.md#plan-9--inferno

## Context
ADR-0003's `spawn` takes an image handle. At stage 3 there is no filesystem service, only the embedded ramdisk (ADR-0033). The parent needs an image handle, and ADR-0002 forbids a global namespace.

## Decision drivers
1. No global name: a process reaches only what it was given (CP-01, DM-009).
2. Stage 4's directory handles should replace this without changing `spawn`.
3. Lookup takes a relative name (INV-0002-1's `RelativePath`).

## Considered options

### Option A: A path string in `spawn` (Linux `execve`)
- **Description:** `spawn` names a program by absolute path, resolved by the kernel.
- **Evidence for:** Simplest for callers.
- **Evidence against:** A global namespace in the kernel, against ADR-0002 and DM-009.
- **Cost:** Permanent.
- **Precedent:** Unix.

### Option B: A ramdisk directory handle, with lookup by relative name
- **Description:** The first process receives a directory handle for the ramdisk. `directory_lookup(directory, name)` returns an image handle, and `spawn` takes that.
- **Evidence for:** Authority is a handle, and passing the directory or one image limits what a child can start. It is the shape of a launcher handing out handles (GR-005, `alternatives-postmortem.md#fuchsia--zircon-incl-starnix`) and of per-process name spaces (`alternatives-postmortem.md#plan-9--inferno`).
- **Evidence against:** One call that stage 4's service will replace with messages.
- **Cost:** *(estimate)* 30 lines.
- **Precedent:** Fuchsia launcher; Plan 9.

### Option C: One image handle per program, given at start
- **Description:** The first process gets a handle per ramdisk file, at known slots.
- **Evidence for:** No lookup call at all.
- **Evidence against:** The slot list becomes a global numbering of programs, and 1024 slots bound the ramdisk.
- **Cost:** A table of slots to keep in step with the build.
- **Precedent:** None in the files.

## Recommendation
Recommended: **Option B**. It is a recommendation, not a decision.

- **`directory_lookup(directory, name)`** needs `LOOKUP`, and returns an image handle with `READ`, `DUPLICATE` and `TRANSFER`. A missing name is `NotFound`. The name is a `RelativePath`: UTF-8, not empty, no leading `/`. The ramdisk is flat, so a `/` anywhere simply finds nothing.
- **Startup convention.** A process started by the kernel gets the console at slot 1, a job handle at slot 2 and the directory at slot 3. The convention is `zoraya-abi` constants, not a kernel rule for children: a child has what its parent listed (INV-0003-1).
- **Rights of the first handles.** All the rights their type can carry.

## Consequences
- **Positive.** `spawn` does not change at stage 4: only where an image handle comes from.
- **Negative.** `directory_lookup` and the `Directory` object are stage-3 stand-ins.
- **Follow-up.** The filesystem service gives directory handles over IPC.

## Invariants
- **INV-0043-1.** A process with no directory or image handle cannot start a program. *Check:* in-QEMU test: a child with an empty handle list has no valid handle to name one.

## Open questions
- Whether an image handle should stay valid as a read-only memory object at stage 4.

## Revisit triggers
- Stage 4's directory protocol.

## Architect's decision
