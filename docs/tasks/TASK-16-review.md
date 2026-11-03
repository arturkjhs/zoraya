# TASK-16 review: stage 6a, kernel objects

An independent review by a subagent that did not write the code (on the main model, as `CLAUDE.md` asks), of
`git diff 9b58034..ac0a1f4`: byte streams (ADR-0101), `snapshot_process` (ADR-0102), `image_create` (ADR-0108),
`job_suspend` and `job_resume` (ADR-0105 item 8), the startup block (ADR-0109 item 5), `object_get_signals`, their
tests and the proposed ADR-0130. The reviewer only read code. Fixes are in `d331580`. Then a check of the changed
lines by a second subagent, whose findings are fixed in `6b5aa97`, and one more check of those lines, fixed in
`b935e76`. Then mutation testing, sorted by the `mutant-triage` subagent, with the scripts as the final judge.

## Findings and dispositions

| Id | Severity | Finding | Disposition |
|---|---|---|---|
| R1 | risk | INV-0102-3's per-refusal check compares free frames, and the kernel heap never gives pages back: a refused call that grows the heap would look like a leak. | Kept as is, stated here. The sweep starts with a warm-up snapshot, which grows what grows once; every refusal in every configuration of two full runs returned every frame and byte of heap. A heap that grew during a refusal would fail the check, which is the conservative side. |
| R2 | risk | `#[personality_only]` was enforced only by `snapshot_process`'s handler: a second tagged call without its own check would pass the abi lint. | Fixed. The dispatcher refuses every tagged call outside a `posix` job, through `zoraya_abi::is_personality_only`, a `match` the `syscalls!` declaration generates. After the second check the handler's own test went, so the gate is in one place and its mutant can be caught. |
| R3 | risk | A pin moved into a message queued on the caller's own channel escapes the pin check, so a caller can snapshot with a live pin. | Recorded as an open question in ADR-0130 (proposed). The child's memory is fresh, so nothing is shared through the pin; a POSIX process holds no device, so it cannot make a pin. |
| R4 | risk | The copies of mapped objects were zeroed, then overwritten, which doubled the write traffic of the masked interval being measured. | Fixed. `MemoryObject::new_for_copy` takes frames unzeroed; every byte is written before the child exists (the check confirmed every page of a recorded mapping is a leaf the walk copies). The 8 MiB case went from about 1.8 ms to about 1.4 ms under HVF; `design.md` has the new figures. |
| R5 | risk | No test moved a duplicable handle, so ADR-0130 item 2 ("moved, not also duplicated") was untested. | Fixed with T3. |
| T1 | test gap | The posix gate's test could not tell the caller's job from the target's. | Fixed: `outer` snapshots into a `posix` job and expects `AccessDenied`. |
| T2 | test gap | No child relied on an inherited disposition. | Fixed: the table test's child relies on `Custom(5) = Deliver` set by the parent before the call. |
| T3 | test gap | A moved duplicable handle in one place. | Fixed: a stream write end moves to slot 700; its old slot is empty in the child and the parent reads what the child wrote through it. |
| T4 | test gap | The table comparison read handle values, not rights. | Fixed: rights are compared slot by slot, the self slot `WAIT` only, moved slots the caller's full rights. |
| T5 | test gap | A process joining a suspended job by snapshot was untested. | Fixed: `a_child_in_a_suspended_job_waits`. |
| T6 | test gap | A thread blocked in the kernel during a suspend, and `start_suspended` with a suspended job. | Fixed: `resume_wakes_only_what_it_stopped` (a sleeper is not woken early by a resume; `process_resume` inside a suspended job waits for `job_resume`). |
| T7 | test gap | `zoraya-elf`'s object-backed host test re-implements the kernel's sequence. | Kept: it is the host half of INV-0108-1's check; `t_image` covers the kernel's path in QEMU. |
| T8 | test gap | Nothing reaches `TooLarge` in `map_startup_block`. | Kept as a defensive check that cannot be reached: vectors are under 64 KiB and the table under 1792 bytes, so the block is under `STARTUP_BLOCK_MAX` by construction. |
| N1 | nit | `MemoryObject::read`'s result ignored in `image_create`. | Fixed: commented and `debug_assert!`ed. |
| N2 | nit | Unchecked addition in `Program::from_table`. | Fixed: `checked_add`. |
| N3 | nit | `argv[0]` becomes the kernel's name for a memory image's process. | Kept: it appears only in the kernel's own fault lines. |
| N4 | nit | A criterion test line that checked nothing. | Removed; the table test covers it. |
| C1 | check 1 | Two gates: neither gate's mutant could be caught. | Fixed in `6b5aa97` (R2's second half). |
| C2 | check 1 | The new tests had no mutants in `tools/mutants`. | Fixed: `K-snapshot-moved-also-duplicated`, `K-resume-wakes-everything`, `K-self-handle-full-rights`; all caught. |
| C3 | check 1 | A fixed 30 ms wait before "now it runs" could fail on a slow host. | Fixed: `t_suspend` polls up to 2 s for every "it runs" check. |
| C4 | check 1 | The dispatcher scanned the whole ABI table per call. | Fixed: a generated `match`. |
| C5 | check 2 | After the polls, the check that the lower job stayed stopped came too soon to mean anything. | Fixed in `b935e76`: a fixed wait first; fixed waits are safe before "nothing happened" checks. |

The reviewer found these sound: lock order for stream ends and job walks (no re-entry), all-or-nothing of
`snapshot_process` against every failure point, generations, the register frame and `TPIDR_EL0`, the Arm ARM sequence
(copy through the direct map, `DC CVAU` per executable page, one `DSB ISH; IC IALLUIS; DSB ISH; ISB`, a fresh ASID
invalidated when last freed, a non-zeroed frame mapped only in an inactive table and written at once), park, stop
and cont (no lost wakeup on one CPU, exit while parked), every new SAFETY comment, and hostile inputs (no panic, no
unbounded masked loop driven by a user value other than the snapshot copy's size, which ADR-0102 item 8 declares).

## The intermittent TCG run

Before `e423bb5`, `t_snapshot`'s frame-exhaustion sweep held and gave back all free memory at every step, writing a
list link into each held frame. Under TCG the test kernel's second run of `t_snapshot` in one boot did not finish in
`make test`'s 180 s, nor in 900 s. At the architect's request it was reproduced afterwards with
`tools/repro/tcg-loop.sh 96c5a79 20 2`, which builds that commit, runs the test kernel under TCG twenty times, two at
a time, and samples a run that does not end through QMP and the gdb stub: **20 of 20** runs were still in the second
sweep after 240 s, each at step 46 to 48 of 333, inside `hold_frames` or `release_frames`, with no loop and no other
ready thread.

The cause was then measured, not guessed:

- Steps over time, read through the gdb stub: about 10 a second in the first run of the sweep, about 0.32 a second
  in the second, so the second sweep takes about 1000 s.
- Counters added to the frame allocator (in a scratch copy of that commit) and read over the gdb stub: about one
  bitmap word scanned per allocation in both runs, but about 35 times fewer allocations a second in the second run.
  The scheduler had nothing else ready. The allocator's algorithm and the scheduler are therefore not the cause.
- QEMU's `info jit`: no growth in translation invalidations or flushes during the slow phase.
- Keeping the held frames' list in a static array instead of in the frames themselves made the second run as fast as
  the first, in two runs (3.3 and 3.2 million allocations in 10 s, against 75 thousand).

So the cost is in touching the held frames. After the first run, the free frames include thousands that held user
code the snapshot children executed; under TCG, accesses to those pages stay slow. That is QEMU's cost, probably its
tracking of writes to pages with translated code; it was not checked against QEMU's sources. It is not a kernel
defect, and nothing in the kernel changes. The sweep since `e423bb5`
writes each held frame once and gives back one frame a step, so it avoids the cost; every TCG configuration of the
full `make test` runs since then passed. A test of elapsed time under TCG would be unreliable, so none was added; the
finding and the command to repeat it are in `docs/backlog.md`.

## Mutation testing

Hand-written list `tools/mutants/kernel-objects.toml`, run with `make mutants` (`test-qemu-one` per mutant), at
`b935e76` and again for the two survivors at `cd9528e`:

- 27 mutants: 26 caught, 1 survived.
- `K-snapshot-no-icache` survives and is equivalent under QEMU: QEMU keeps instruction fetch coherent with data
  writes, so leaving out the instruction cache invalidation changes nothing a test can see. It stays in the list as
  the record of the Arm ARM requirement.
- `K-image-length-unchecked` survived at `b935e76`: every length past the object was tried on a file whose headers
  were refused anyway. `t_image` now tries those lengths on `hello` itself, and the mutant is caught at `cd9528e`.

`make mutants-host CRATE=zoraya-stream` (cargo-mutants 27.1.0), sorted by `mutant-triage`:

| | Mutants | Caught | Missed | Timeouts | Unviable |
|---|---|---|---|---|---|
| Before (`b935e76`) | 93 | 82 | 4 | 3 | 4 |
| After (`cd9528e`) | 93 | 87 | 1 | 1 | 4 |

- The one missed mutant is equivalent: `1 << 0` and `1 >> 0` are both 1.
- Three holes were closed: `is_empty`, `reader_open` and `writer_open` are now asserted after a change, and a
  zero-byte read of an empty ring with a writer is checked. The record test's drain loop now stops on an empty read,
  so a mutant that returns nothing fails by assertion, not by timeout.
- The remaining timeout, `-=` to `/=` in `commit_read`, fails four tests before another hangs, so it is caught in
  substance.

**Memory.** The first cargo-mutants runs on this crate, four jobs at a time, took 63 GB between four test processes and
stopped the host twice. A mutant that never consumes the ring makes a test's drain loop grow a vector without bound.
Since `aed800c`:

- every host test under `crates/` caps its own heap at 512 MiB (`crates/zoraya-test-alloc`);
- mutation runs use at most two workers;
- `tools/mutants/watchdog.sh` kills a process past a memory limit.

`docs/backlog.md` records these guards for every future run. The runaway mutant now aborts in about a second. The two
runs above finished with no watchdog kill.
