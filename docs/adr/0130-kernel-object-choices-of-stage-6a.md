# ADR-0130: Kernel object choices the stage 6 ADRs leave open

- **Status:** accepted
- **Date:** 2026-11-02
- **Depends on:** ADR-0040, ADR-0042, ADR-0044, ADR-0057, ADR-0101, ADR-0102, ADR-0105, ADR-0108, ADR-0109
- **Evidence:** CP-11, alternatives-postmortem.md#fuchsia--zircon-incl-starnix

## Context

TASK-16 built the kernel side of stage 6a: byte streams (ADR-0101), `snapshot_process` (ADR-0102), `job_suspend`
and `job_resume` (ADR-0105 item 8), `image_create` (ADR-0108) and the startup block's program header table (ADR-0109
item 5). Writing the code met points where the accepted ADRs are silent or say less than the code must decide. The
task's rule: write a proposed ADR and implement the least committing option. Each item below is implemented as the
Recommendation says, and the tests named under Invariants cover it.

What is measured: nothing here is chosen on a measurement; the costs of stage 6a are in `docs/design.md`, "Stage 6
kernel objects (as built)".

The items:

1. **The snapshot buffer's layout.** ADR-0102 lists the fields (magic `ZSN1`, header size, flags, `job`, `group`,
   `self_slot`, move count, reserved zero, entries `{slot, handle}`) but not their offsets, whether `self_slot` may
   be absent, or what a slot named twice means. It also needs a parameter kind for the ABI table (ADR-0044).
2. **A duplicable handle that is also moved.** ADR-0102 item 3 duplicates every duplicable slot, item 4 moves a
   listed handle "out of the caller's table into the child's at `slot`". A listed handle of a duplicable type would
   then be in the child twice: duplicated at its own slot and moved to the listed one.
3. **Empty slots in the child.** Items 3 and 4 give filled slots the parent's generations. They do not say what an
   empty slot of the child is at. A stale handle value in the copied memory names nothing in the parent; at
   generation 0 in the child, a handle the child later makes there could take that value.
4. **A job outside the caller's tree.** Item 1 says the job "must be in the caller's job tree" without a status.
5. **`TPIDR_EL0`.** Item 5 says "the caller's registers as saved at the `svc`". The thread pointer is not in the
   exception frame; the kernel keeps it in the thread's context.
6. **Out of address space identifiers.** Item 6 lists ASIDs among the causes of `NoMemory` or `LimitReached`, not
   which.
7. **What `job_suspend` and `job_resume` mean in detail.** ADR-0105 item 8 says they "stop and continue every
   thread in the job tree and assert `SUSPENDED` on the job and every job below it". It leaves open: a suspend of a
   suspended job; a resume of a job that is not suspended; a resume of a job whose ancestor stays suspended; a job
   or process that joins a suspended tree; the process's own `SUSPENDED` signal, which `start_suspended` uses
   (ADR-0003); and when a thread blocked in the kernel stops.
8. **`object_get_signals`.** ADR-0006 and ADR-0104 item 11 name the call; no ADR gives its right or what it does on
   an object with no signals.
9. **The startup block's layout.** ADR-0109 item 5 appends the program headers after the vectors, "versioned by the
   header size", but ADR-0042's block has no header size and no count of headers, and the table needs 8-byte
   alignment for `Elf64_Phdr`'s fields.
10. **Zero-length transfers on a stream.** ADR-0101 says what a write of at most `STREAM_ATOMIC` bytes does with less
    than `STREAM_ATOMIC` free; by that letter a write of 0 bytes with 100 free would be `WouldBlock`.
11. **The name of a process from a memory image.** A ramdisk image has a name; one from `image_create` does not.
12. **Where a stream's bytes live.** ADR-0101 says "committed from frames"; it does not say how the kernel reaches
    them.

## Decision drivers

1. **Least committing** (TASK-16's rule): a choice the next stages can change without breaking a program.
2. **Refuse what is not understood** (CP-11): unknown flags, nonzero reserved fields and ambiguous requests fail.
3. **Keep the accepted ADRs' guarantees**: every handle value in the copied memory names in the child what it named in
   the parent (INV-0102-7), and nothing visible changes on a refused call (INV-0102-2).
4. **Follow the kernel's existing conventions** where an ADR is silent: rights are checked before the kind, ASID
   exhaustion is `NoMemory` for `spawn`, a flat buffer has its header size at offset 4.

## Considered options

### Option A: Do what Linux does (baseline)

- **Description:** `fork` duplicates every descriptor, a stopped process is stopped by a signal state per process
  with nesting counts, `SIGCONT` resumes whatever was stopped, the auxiliary vector carries `AT_PHDR` to the
  program headers in the mapped file.
- **Evidence for:** What the ported programs expect at their level; `libposix` hides the difference.
- **Evidence against:** The kernel has no signal state and no file mapping; ADR-0102 and ADR-0105 already chose
  jobs and move lists. None of these points is about Linux's behaviour; they are about the native objects.
- **Cost:** Not applicable to this kernel.
- **Precedent:** Linux.

### Option B: The least committing choice at each point (this implementation)

- **Description:** As the Recommendation lists.
- **Evidence for:** Drivers 1 to 4.
- **Evidence against:** Some choices are refusals (a slot named twice, the self slot among the moves) that a later
  caller might want; refusing now and allowing later breaks nothing.
- **Cost:** As built.
- **Precedent:** ADR-0016 did the same for the filesystem.

### Option C: Zircon's shapes (non-Unix precedent)

- **Description:** A process builder and `zx_task_suspend` tokens: each suspend returns a token, and the task runs
  again when every token is closed (`alternatives-postmortem.md#fuchsia--zircon-incl-starnix`, and
  `[outside evidence: fuchsia.dev zx_task_suspend, not fetched]`).
- **Evidence for:** Suspends nest without a shared flag; a holder that dies resumes what it stopped.
- **Evidence against:** ADR-0105 chose `job_suspend`/`job_resume` with a right, not tokens; a token would be a new
  object type and an ABI change the accepted ADR did not make.
- **Cost:** *(estimate)* A new object type and its lifetime rules.
- **Precedent:** Zircon.

## Recommendation

### Decision

Accepted 2026-11-03, with one change: **`dma_pin` refuses with `AccessDenied` in a job with the `posix` bit**, so a
POSIX process makes no pin and none can sit unseen in a message queued on its own channel when it snapshots (the open
question from the TASK-16 review, R3). INV-0130-3 states it. A pin made elsewhere can still be handed to a POSIX
process, in a table or a message; one in its table refuses the snapshot as INV-0102-5 says, and a POSIX process has
no reason to hold one.

The original recommendation follows, accepted as written.

**Recommendation (not a decision): Option B**, as follows.

1. **Snapshot buffer.** Little-endian; magic at 0, header size (48) at 4, flags at 8 (`START_SUSPENDED` only),
   reserved zero at 12, `job` at 16, `group` at 24 (0 for none), `self_slot` at 32 (0 for none), move count at 36,
   reserved zero (64 bits) at 40; then 16-byte entries `{slot u32, reserved u32 zero, handle u64}`; nothing may
   follow. Refused: a slot out of range, a slot named twice, a handle named twice, and `self_slot` among the moves.
   In the ABI table the call takes the `SpawnArgs` kind, renamed in its comment to "a flat buffer of ADR-0042",
   rather than a new kind: the lints treat both buffers alike.
2. **A moved handle is not also duplicated**: its own slot is empty in the child unless something else lands there.
   "Moves out of the caller's table" reads as one place in the child.
3. **Every slot of the child starts at the parent's generation**, filled or not, up to the larger of the parent's
   table and the highest slot the call names; so a stale value in the copied memory stays stale in the child.
4. **A job outside the caller's tree is `AccessDenied`.** A group outside the job is `InvalidArgument`, as in `spawn`.
   As everywhere in the kernel, a missing right is reported before a wrong kind.
5. **`TPIDR_EL0` is copied**: it is the calling thread's register, and musl keeps its thread pointer there.
6. **ASID exhaustion is `NoMemory`**, as `spawn` reports it today.
7. **`job_suspend`/`job_resume`.** One flag per job, set or cleared over the whole tree under the tree's lock.
   Suspending a suspended job and resuming a running one are `Ok` and change nothing. A resume of a sub-job runs its
   processes even while an ancestor stays suspended (no nesting count). A job created under a suspended job, and a
   process that joins one (by `spawn` or `snapshot_process`), start stopped. A stopped process's thread parks on its
   next way to EL0: a thread blocked in the kernel finishes its call first, and runs no user instruction until the
   resume. An exit request still ends a stopped process. The process's `SUSPENDED` signal stays `start_suspended`'s;
   a stop shows only as the job's `SUSPENDED`, which is what ADR-0006's `WUNTRACED` mapping reads.
8. **`object_get_signals` needs `WAIT`**, the right that subscribing needs, and refuses an object with no signals
   with `BadHandle`, as `port_subscribe` does. It returns only the bits the type defines.
9. **Startup block.** `argc` u32 at 0, `envc` u32 at 4, header size (16) at 8, the number of program headers at 12;
   the strings from 16; zeros to a multiple of 8; then the whole table. A block that would exceed
   `STARTUP_BLOCK_MAX` refuses the `spawn` with `LimitReached` (ADR-0109 item 5).
10. **A transfer of zero bytes does nothing and returns 0**, except that a write with no read end is still
    `PeerClosed`. A read with zero room returns 0, which a caller cannot tell from end of file; `libposix` never asks
    for zero bytes.
11. **A process from a memory image is named by its `argv[0]`** if that is UTF-8 of 1 to 32 bytes, and `image`
    otherwise. The name only appears in the kernel's fault reports.
12. **A stream's ring is a memory object** of 64 KiB, never mapped and never named by a handle, reached through the
    direct map a page at a time under the stream's lock; the user copy goes straight between user memory and those
    frames, so a transfer allocates nothing (INV-0101-4).

## Consequences

- **Positive.** Every point has one stated behaviour and a test; the accepted ADRs' guarantees hold.
- **Negative.** Suspends do not nest; a resume of a sub-job overrides its ancestor's suspend. A zero-byte read looks
  like end of file.
- **Follow-up.** If `libposix` (stage 6d) needs nesting stops, a superseding ADR with counts or tokens (Option C).

## Invariants

- **INV-0130-1.** A refused `snapshot_process` changes nothing the caller can see, and a moved handle is in exactly
  one place in the child. *Check:* `t_snapshot`'s move-list and table tests.
- **INV-0130-2.** A process in a suspended job, including one that joins it, runs no user instruction until
  `job_resume`. *Check:* `t_suspend`.
- **INV-0130-3.** `dma_pin` fails with `AccessDenied` in any job with the `posix` bit. *Check:* `t_snapshot`, which
  calls it from its `posix` job with a device handle and expects the error.

## Open questions

- Whether stage 6d's job control needs nested suspends.
- Answered by the architect's decision: a pin moved into a message queued on the caller's own channel was not seen
  by INV-0102-5's check (TASK-16 review, R3); `dma_pin` now refuses in a `posix` job (INV-0130-3).

## Revisit triggers

- A `libposix` path that stops a group from two places at once.
- A program that reads its own program headers some other way than `AT_PHDR`.

## Architect's decision

### Decision

Accepted 2026-11-03, with the change above: `dma_pin` refuses in `posix` jobs, with a rule (INV-0130-3) and a test.
Decided by the architect; recorded by an agent at the architect's request.
