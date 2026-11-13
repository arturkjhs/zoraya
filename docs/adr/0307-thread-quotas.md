# ADR-0307: Thread quotas: a `threads` figure of its own, derived from the processes figure

- **Status:** accepted
- **Date:** 2026-11-12
- **Depends on:** ADR-0046, ADR-0260, ADR-0261, ADR-0262, ADR-0263, ADR-0264, ADR-0266, ADR-0267, ADR-0270, ADR-0285, ADR-0300, ADR-0301
- **Evidence:** CP-07, CP-14, DM-030, DM-004

## Context

ADR-0260's table has a row "Threads: live threads, the kernel, the job: later: one thread per process today", and two
open questions: "Whether threads, when added, should count against processes (one address space identifier each is not
needed) or have their own field: decide with the threads ADR" (ADR-0260, open questions) and "`--procs` may want a
sibling" (ADR-0266, what is not built). ADR-0261 point 5 already charges every kernel object, "thread with its 64 KiB
stack" included, to memory; the thread record and stack are paid per thread today in `Thread::_paid`.

What a thread costs that memory does not bound: a thread stack is 64 KiB of kernel memory (16 frames) plus the user stack
the program maps; at 512 MiB of room memory that is up to 8,000 threads. Things that grow with the number of threads, not
with bytes: the run queue and the parked list walks (ADR-0285 point 1 caps a pick at 16 parks and a release at 64, so
time is bounded but a long list lengthens a period), the deadline queue (one entry per thread that sleeps with a
deadline, ADR-0301 item 5), the waiter list of a futex (at most `MAX_THREADS` per process), `job_kill`'s wake of one blocked thread per process, which wakes the others in steps of 64 (ADR-0300 item 6) and the handle table (two slots per thread in the library). Memory
overcommit's lesson (DM-030) is that a resource that can be asked for without a count is eventually asked for without
limit; CP-14 wants the refusal at the request; CP-07 wants the room to contain it.

DM-004 says each older process feature needed a rule for threads. For limits, Linux counts threads and processes in one
number (`RLIMIT_NPROC`, `pids.max`).

## Decision drivers

1. A room cannot create threads without a count that its launcher set (CP-07, CP-14), with the refusal at `thread_create`.
2. The processes figure keeps its meaning: address space identifiers (255) and fork bombs (ADR-0261 point 1).
3. No new word for the person to learn until it is needed (ADR-0266).
4. The records stay compatible: `job_set_limits` and `job_get_usage` have fixed layouts that old callers use
   (`zoraya-abi/src/quota.rs`).
5. The bounds of the kernel's queues (deadline queue 1024, `MAX_THREADS_SYSTEM`) hold whatever the limits say.

## Considered options

### Option A: A `threads` field of its own, counting every live thread, derived by the keeper from `procs` (recommended)

- **Description:** As recommended below.
- **Evidence for:** Drivers 1 to 5. A bomb of threads and a bomb of processes are different shapes and one number cannot
  cap both without capping the useful case (32 processes of 3 threads each).
- **Evidence against:** A fourth `Counters` entry in `zoraya-charge` (`Field::Threads`), a longer usage record, and one
  more derived number the keeper must set in every room.
- **Cost:** *(estimate)* 120 lines in the kernel and the crates, 60 in the keeper.
- **Precedent:** Linux's `RLIMIT_NPROC` counts both in one (below); Windows job objects have an active-process limit only
  `[outside evidence: Windows job objects]`.

### Option B: Count a thread as a process in the `processes` figure

- **Description:** `thread_create` charges `Processes` by one.
- **Evidence for:** No new field; the same Linux behaviour as one `pids.max`.
- **Evidence against:** The 255 identifiers are a different resource: with `--procs 32` a 32-thread program uses the room's
  processes; a `fork` (ADR-0102) would then fail with a room full of idle threads; the number the person set in
  ADR-0266 changes meaning.
- **Cost:** *(estimate)* 20 lines.
- **Precedent:** Linux `pids.max`.

### Option C: No thread limit; memory bounds them

- **Description:** A thread is 66 KiB or more, so `--mem` bounds the count.
- **Evidence for:** Nothing to add (ADR-0261 point 5 charges the stack).
- **Evidence against:** The deadline queue and the walks above scale with the count, not the bytes; `--mem 512M` allows
  8,000 threads, over the queue's 1024.
- **Cost:** *(estimate)* none; an unbounded queue.
- **Precedent:** Early Unix with `RLIMIT_NPROC` unset.

## Recommendation

**Recommendation (not a decision): Option A.**

1. **The field.** `Field::Threads` in `zoraya-charge` (index 3), `QuotaField::Threads = 3` (the value register of
   `OverQuota`), counted for every live thread of every process of the job's subtree, **including each process's first
   thread**. `spawn` and `snapshot_process` charge one thread together with the process, after `Processes` and before
   memory, so the field that refuses is the first that would be exceeded in the order processes, threads, memory.
2. **The records.** `JobLimits.threads: Option<u32>` is stored at bytes 16 to 20 of the 24-byte limits record, 0 meaning no
   limit (the word is today "reserved, must be zero", so an old caller that writes zeros means "no thread limit", and the
   decoder accepts it); bytes 20 to 24 stay reserved. **`job_set_limits` replaces the whole record**: today all limits are set together or none
   (`Job::set_limits`, memory, processes and CPU), and `threads` joins them, so a call that leaves bytes 16 to 20 zero removes an earlier thread limit of that job (an old
   caller does this by not knowing the word), and the limits of the ancestors still apply through the walk, so what is
   removed is only the job's own ceiling. Callers that must keep it read the record back and write it whole. The usage record grows from 120 to 152 bytes: `limit`, `used`, `peak`
   and `refused` (four `u64`) for threads after the CPU words. The kernel writes what fits; a decoder reads missing
   words as zero and no limit (ADR-0285 point 4's rule).
3. **The call.** `job_set_limits` checks `threads` as ADR-0261 point 2 checks the others (not above the parent's
   effective limit; lowering below use allowed; needs `LIMIT`). `thread_create` returns `OverQuota` with
   `QuotaField::Threads` in the value register when the walk refuses.
4. **Numbers** `[assumption, all]`. The room keeper derives `threads = 3 × procs` for every room that has `procs`:
   32 gives 96. `init` sets the session job to 600 (three times its 200 processes) and the keeper sets the untrusted rooms'
   job to 288 (three times its 96). These per-job figures bound the session and the untrusted rooms only; the 55 service
   processes of ADR-0266 point 4 and the trusted jobs outside the session have no thread figure, and only the kernel's
   `MAX_THREADS_SYSTEM` (1000) bounds them. The deadline queue is therefore sized from that system figure, not from the
   session's 600: `QUEUE_SIZE >= MAX_THREADS_SYSTEM + kernel timers + RESERVED` (ADR-0301 item 5). A trusted room and `home`
   have no figure of their own. No new word in the room's line yet: `room` shows the derived figure in `grants` (one
   more number after `--procs`) and `--threads N` is the later sibling that ADR-0266 left open.
5. **The signal.** A refusal asserts `OVER_QUOTA` on the limited job as for any field (ADR-0263 point 3); the keeper's
   `Refused { field }` gains `Threads`, and `top` shows the figure as `thr` (ADR-0267, ADR-0294) as a follow-up.
6. **POSIX** (rows added to ADR-0263 point 4's table): threads refused: `pthread_create`, `thrd_create` return `EAGAIN`
   (POSIX's code for "insufficient resources or a system limit on the number of threads"); `fork` refused for threads:
   `EAGAIN` as for processes.
7. **What a thread does not cost twice.** The stack, TLS and `struct pthread` are one memory object charged to memory
   (ADR-0303 item 3); the kernel stack and record are charged at `thread_create` (ADR-0300 item 2). A thread that ends
   refunds the kernel stack, the record and the figure **in `thread_exit`** (ADR-0300 item 3), not when its handle is
   closed or the process ends; the user mapping is refunded by the joiner or by the thread itself. The thread object that
   remains costs a few words and is refunded when its handle is closed, which the thread does itself (ADR-0302 item 4).

**Tensions with accepted ADRs.** *ADR-0260's* row "Threads | later" is filled. *ADR-0261 point 1* (`JobLimits` has memory and
processes, CPU in ADR-0264) gains a field. *ADR-0266 point 1* (five words) stays five. *ADR-0263 point 4's* table gains a row.
*ADR-0285 point 4's* usage record grows by the same rule.

## Consequences

- **Positive.** A room's thread bomb is refused at the request; the queues stay bounded; old binaries and records keep
  working.
- **Negative.** One more number in `grants` and the usage record; a derived figure that is a guess until measured.
- **Follow-up.** `zoraya-charge`, `zoraya-abi` `quota.rs`, `job_set_limits` and `job_get_usage`, the keeper's derivation,
  `init`'s session figure, `docs/design.md`'s Quotas section.

## Invariants

- **INV-0307-1.** A job with `threads = N` runs at most N live threads in its subtree: the N+1th `thread_create`, `spawn` or
  `snapshot_process` returns `OverQuota` with `Threads`, and changes nothing. *Check:* `cargo test -p zoraya-charge` (the
  walk with a fourth field) and an in-QEMU test at N = 3.
- **INV-0307-2.** The limits record with zero in bytes 16 to 24 means no thread limit, and a usage buffer of 88 or 120 bytes is
  still accepted. *Check:* `cargo test -p zoraya-abi` (the decoders over the old sizes).
- **INV-0307-3.** The sum of the thread ceilings the keeper and `init` set never exceeds 600 under the session, and a room's is
  three times its processes figure. *Check:* a host test in `zoraya-rooms` over the derivation.
- **INV-0307-4.** A thread that ends refunds the figure, at its `thread_exit`: usage after 1000 create and join rounds, and
  after 2,000 detached ones, equals before. *Check:* an
  in-QEMU test with `job_get_usage`.
- **INV-0307-5.** `pthread_create` returns `EAGAIN` for `OverQuota(Threads)` and for `OverQuota(Memory)`. *Check:* INV-0100-4's
  table test and an in-QEMU `libposix` test in a limited job.

## Open questions

- Whether 3 per process is right: measure CPython's `threading` tests and the stress test of ADR-0309.
- `--threads` as a typed word.
- Whether kernel threads and services should count (ADR-0264 point 7: never capped).

## Revisit triggers

- A real program that needs more than three threads per process on average in an untrusted room.
- SMP, where thread counts per CPU may matter.

## Architect's decision

Accepted 2026-11-13. Decided by the architect; recorded by an agent at the architect's request.
