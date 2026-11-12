# ADR-0270: Bounded kernel work: long calls proceed in steps with interrupts enabled between them

- **Status:** accepted
- **Date:** 2026-11-10
- **Depends on:** ADR-0030, ADR-0041, ADR-0050, ADR-0051, ADR-0056, ADR-0102, ADR-0261, ADR-0264
- **Evidence:** CP-14, X-6, alternatives-postmortem.md#keykos--eros--capros-and-coyotos, quotas/survey.md#zoraya-today, quotas/survey.md#cpu

## Context
System calls run with interrupts masked, and some do work proportional to their argument (`docs/design.md`, "Known
limitations", measured under HVF on 128 MiB): `memory_create` of all memory about 9 ms and closing that object about
4 ms; closing a chain of 20,000 channel ends about 11 ms; `memory_map`, `dma_pin`, an address space's teardown and a
device reset are "the same kind". `snapshot_process` copies every page with interrupts masked (ADR-0102 item 8:
"ADR-0056 must bound it before untrusted code runs"), and ADR-0050 says of the channel teardown "ADR-0056 is what
bounds a chain for untrusted code". A process's teardown does not even run on its own thread: `schedule()` calls
`reap()`, which drops a dead thread, and with it its process, in whichever thread switches next
(`kernel/src/sched.rs`), so the stall and its CPU time land on another job. With quotas a room can still ask for its
whole `--mem` in one call, or free it in one exit: a limit bounds the size, not the time, and ADR-0264's CPU ceiling
can only act at the next return to EL0 (`quotas/survey.md#zoraya-today`). KeyKOS's own list of warts records the
same failure in a capability system: batched kernel work grew roughly linearly with RAM and could not be kept out of
critical periods, so real-time support was poor (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`).

## Decision drivers
1. No program may hold the CPU in the kernel, or make the kernel hold it on its behalf, for longer than a fixed bound,
   whatever its arguments, or ADR-0264's ceiling and the terminal's latency (INV-0264-3) mean nothing.
2. A call keeps its all-or-nothing result (CP-14, ADR-0051's no-effect failure).
3. The kernel stays non-preemptive (ADR-0030): the change must not add preemption of kernel code in general.
4. The cost on small calls must be nil.
5. Work done for a dead process is bounded like work done by a live one; its memory stays charged until freed.

## Considered options

### Option A: A preemptible kernel (Linux's `PREEMPT`, or voluntary points everywhere)
- **Description:** Kernel code can be preempted outside locks, or calls `cond_resched` at many points
  `[outside evidence: Linux's preemption models, not fetched]`.
- **Evidence for:** Bounds latency everywhere at once.
- **Evidence against:** Driver 3: every kernel path becomes reentrant, a large change for a small kernel.
- **Cost:** *(estimate)* Large; a review of every lock and per-CPU assumption.
- **Precedent:** Linux.

### Option B: Cap the size of one call
- **Description:** Refuse `memory_create` above, say, 16 MiB and `snapshot_process` of a process above a size, in a
  limited job.
- **Evidence for:** Trivial; bounds time by bounding size, for the calls that take a size.
- **Evidence against:** Breaks programs that need large objects; `fork` of a large process becomes impossible, which
  X-6's strict commit already makes expensive; does nothing for teardown, whose size is what the room accumulated.
- **Cost:** *(estimate)* 20 lines.
- **Precedent:** none found as a design.

### Option C: Long work in steps, with interrupts enabled and a preemption point between steps; teardown on a kernel thread
- **Description:** Every call whose work grows with an argument or with what it frees does it in steps of at most 64
  pages (256 KiB) or 64 objects `[assumption]`; between steps it holds no spinlock, unmasks interrupts, and yields if
  its slice has ended. The objects being built are not visible until the last step, so a failure part-way frees what
  was done. Teardown of dead processes and closed objects moves from `reap()` to a kernel thread that does it in the
  same steps, charged to the dead object's job until freed (ADR-0261).
- **Evidence for:** Drivers 1 to 5: the bound is one step's time; the result stays all or nothing; preemption happens
  only at points the call chooses, as blocking calls already do. It avoids KeyKOS's failure by keeping no batch
  longer than a step (`alternatives-postmortem.md#keykos--eros--capros-and-coyotos`). The CPU ceiling's overrun is
  then one step (`quotas/survey.md#cpu`). seL4 makes its long-running operations preemptible at points in the same way
  `[outside evidence: the seL4 manual's preemption points, not found in the text extracted this session]`.
- **Evidence against:** Each long call must be written as a loop with its state out of locks; a call can now be
  interrupted by `job_kill`, which must free the partial work; freed memory returns later than today (after the
  reaper's steps), so a room that exits and restarts may briefly see its own old charge.
- **Cost:** *(estimate)* 150 lines per site, six sites, and the reaper thread (100 lines).
- **Precedent:** voluntary preemption points; KeyKOS as the counter-example.

### Option D: A pool of zeroed frames filled by an idle thread
- **Description:** An idle-priority kernel thread zeroes free frames ahead, so `memory_create` takes zeroed ones.
- **Evidence for:** Zeroing moves off the call's path entirely when the pool is full.
- **Evidence against:** Does nothing for a snapshot's copy, a mapping, or a teardown; under load the pool is empty and
  the call zeroes anyway.
- **Cost:** *(estimate)* 200 lines.
- **Precedent:** pre-zeroed pages are common `[outside evidence: not fetched]`.

## Recommendation
**Recommendation (not a decision): Option C** at every site that scales, and Option D only if a measurement asks for
it.

1. **Sites**, every system call path whose work grows with an argument or with what it frees: `memory_create`
   (zeroing), `spawn` (the image copy and zeroing), `snapshot_process` (the copy), `memory_map` and `memory_unmap`
   (page tables for a range), the close of a memory object (its frames), the teardown of a process (address space,
   page tables, handle table) after its exit or `job_kill`, the teardown of a closed channel's chain (ADR-0050), and
   `dma_pin` (drivers only, trusted, included for the bound).
2. **Step.** At most 64 pages, 64 page-table entries' worth of range, or 64 objects `[assumption]`. Between steps: no
   spinlock held, interrupts unmasked, `sched::preempt_point()` (which switches if the slice ended, ADR-0030).
3. **All or nothing.** The object being built is not in any handle table until the last step; a failure or a
   `job_kill` between steps frees the steps done and refunds their charge (ADR-0261).
4. **Teardown on the reaper.** A kernel thread, `reaper`, takes dead threads, dead processes and closed objects from a
   list that `reap()` (inside `schedule()`, where nothing may yield) only appends to, and frees them in steps with
   preemption points. What it has not yet freed stays charged to its job (ADR-0261 point 9) and is shown as `held`.
   The reaper is a kernel thread, never capped (ADR-0264 point 7), and runs in round robin with user threads, so the
   CPU time of teardown is charged to no job: it is bounded per step and shared like any thread's, and a room that
   exits in a loop makes the reaper work no faster than its own processes can be started again.
5. **The bound.** The longest time with interrupts masked on any system call path, and in `schedule()`, is one step
   plus the fixed work around it; a test measures it (INV-0270-1).

**Tensions with accepted ADRs.** *ADR-0030:* system calls run with IRQ masked and preemption happens on return to EL0
or at explicit yields; this adds explicit yields inside six calls and a kernel thread for teardown. *ADR-0041* and
*ADR-0050:* teardown, specified as running masked "for as long as the chain lasts", runs in steps on the reaper.
*ADR-0051:* `memory_create` and `memory_map` become loops, results unchanged. *ADR-0102 item 8:* fulfilled.

**Examples.** Nothing new is typed. What a person sees is what does not happen: with `agent-1` at `--mem 512M`,

```
agent-1 ~/sandbox/agent-1> ./alloc 500M     # one memory_create of 500 MiB
agent-1 ~/sandbox/agent-1> exit             # and its teardown
```

zeroes and then frees 2,000 steps of 256 KiB each, and a key typed meanwhile in another room's terminal is echoed
within INV-0264-3's bound instead of after the whole zeroing or teardown.

### Effect of the vision
Untrusted code needs it; the general-purpose base gains a terminal that stays responsive while a large build forks
and exits.

## Consequences
- **Positive.** The CPU ceiling and the terminal's latency hold under any argument a limited room may pass, and when it
  exits.
- **Negative.** Six calls become loops; kill during a call has a new path; memory returns a little later.
- **Follow-up.** Q2 in `quotas/plan.md`.

## Invariants
- **INV-0270-1.** No system call, no call of `schedule()` and no timer callback keeps interrupts masked for more than 2 ms
  `[assumption]` under HVF, with any arguments a limited job may pass. *Check:* in-QEMU test with the test kernel's
  masked-time counter over every allocating and freeing system call at the room's whole limit (`memory_create`,
  `memory_map` and `memory_unmap` of its range, closing the object, `snapshot_process` of a 64 MiB process, `spawn` of
  the largest image), the close of a 10,000-endpoint chain, and the exit and the `job_kill` of a process holding the
  room's whole memory.
- **INV-0270-2.** A call killed between steps leaves no object and no charge. *Check:* in-QEMU test: `job_kill` from
  another process during a large `memory_create` and a large `snapshot_process`, then `job_get_usage` and frame
  statistics back to their starting values once the reaper is idle.

## Open questions
- The step size: measure zeroing and copying per page under HVF and TCG.
- *Q2 note (agent, ADR-0001 rule 6):* the steps of 64 are measured by `t_bound`: one step of `memory_create`, `memory_map`,
  `memory_unmap` and a memory object's close takes 2 to 20 us masked under HVF, so the bound has room for steps several
  times larger; Q2 left the 64 as the ADR says. The reaper frees 1024 frames a turn instead of 64: it runs with
  interrupts on, so a turn delays no interrupt, and 64 made it slower than processes can be started (the test that
  starts 255 processes one after another ran out of frames).
- *Q2 note:* `schedule()` still calls `reap()` to drop a dead thread, and with it the small remainder of its process
  (the kernel stack and the record): the frames of a large space or object were the unbounded part and now go to the
  reaper. Moving the thread list too needs a way to wake the reaper from inside `schedule()`; it is not done.
- *Q4 note:* `schedule()` now also charges the run time to the job's ancestors (at most 32 nodes) and parks at most 16
  threads per call, and the timer callback that returns parked threads moves 64 and re-arms itself for the rest, so
  none of the three holds interrupts masked for more than a bounded number of walks. `t_cpu` runs the masked-time and
  units counters over the CPU tests: at most 1051 us and 69 units under HVF.
- *Q4 note (CI):* `job_kill` of a room of many processes exposed a sum the units bound did not cover: the scheduler goes
  from one dying thread to the next with IRQ masked throughout, so the last part of one exit (the address space's
  frames, at most 64 inline, and the previous thread's 16-frame kernel stack freed by `reap`) and the first part of the
  next (handles and mappings, up to a step before the first pause) were one interval: 94 units under TCG, 114 and 212
  on CI, against 80. `exit_current` now starts with `steps::flush()`, so an interval holds one part of one exit: at most
  64 + 16 units (the tail), 71 measured under TCG. The test kernel records where the units of the busiest interval came
  from (`steps::diagnose_units`, printed by `t_cpu` when a bound fails).
- *Q4 note (CI, second):* the flush was not enough: CI then saw 110 and 116 units (2 x 55 and 2 x 58, two exits'
  tails), 94 on `net-tcg`, all from `frames::free`. `steps::pause` unmasked, masked again and then called the
  preemption point; a thread switched away there resumed later inside the interval of the threads that ran meanwhile
  (the scheduler switches with IRQ masked) and went on with its next part with no window. When the slice ended at
  the flush before an exit's tail, that tail, which cannot yield, joined the tail of the exit that switched to it. The
  timing needs an exit as long as the slice, so slow hosts hit it. `pause` is now `steps::step_point`: a window, the
  preemption point, and a second window if the scheduler ran. The test kernel's
  `a_switch_at_a_step_point_starts_a_new_interval` forces that timing with a stand-in for the switch, and
  `frames::free` counts at its caller's line so the diagnosis names the freeing path.
- *Q2 note:* a snapshot copies a mapped object that another process also maps in steps, so a concurrent writer is seen
  part way (a plain `memcpy` of shared memory is no different); the copy of the caller's own pages cannot change meanwhile.
  `snapshot_process` charges the caller's own pages page by page, not before the walk (review Q1, note 14).
- *Q2 note:* a step point with a spinlock held would unmask interrupts under it, and the normal kernel does not count
  locks: `steps::pause` is a safe function whose contract (`unmask_interrupts`'s `# Safety`) says no spinlock is held,
  and only the test kernel asserts it (`sync::held`, in `steps::pause`). A path that no test reaches with a lock held is
  not caught. Counting locks in every build was not done (one relaxed add per lock), to keep driver 4's "nil".
- *Q2 note, deviations from point 4, stated plainly:* (1) `reap()` in `schedule()` does more than append: it drops a
  dead thread and with it the small rest of its process, as before. (2) The reaper frees 1024 frames a turn, not 64
  (above). Neither is a new ADR yet; both stay open questions for the architect, and INV-0270-1's row says that
  `reap()` is not moved, so that it does not read as done.
- *Q2 note (review of Q2):* the work of a teardown is counted across the whole exit, not per object: `steps::before`
  pauses when the units since the last pause plus the next object's would pass a step. The exit, `channel::close` and
  the dropping of mappings call it once per handle, message or mapping with the frames the object frees on the spot
  (at most a step); an object over a step, or a contiguous run over a step, goes to the reaper. The reaper's queue
  doubles when full (outside its lock). If the heap cannot grow it (exhaustion), the drop frees on the spot, masked.
  Before the reaper starts, a drop frees on the spot too: no user process exists then.
- *Q2 note, contiguous memory:* `memory_create(CONTIGUOUS)` is capped at 4 MiB (1024 frames; `frames::MAX_CONTIG_FRAMES`),
  and a larger request fails with `NoMemory`. Users: `fsd`'s 2 MiB data area and the drivers' queues (a few pages),
  `t_devices` and `t_loop`. The run is zeroed in steps of 64 frames with interrupts on and freed through the reaper
  when over a step; the search `help()` loop makes at most 17 rounds (enough to free a run of the cap). The scan of the frame map for a run is still one
  masked pass whose cost depends on the machine's memory, not on the request (about 250 us on the 128 MiB test guest
  under HVF); a resumable scan is not done. The right to ask for it (ADR-0051: "for a device's queues") is not
  checked either; the cap is what bounds a job without one.
- *Q2 note, how the time bound is checked on a slow host:* 2 ms is `[assumption]` under HVF. The first candidate run
  measured 2.7 to 17 ms for a 24 MiB `memory_create` on the CI runners (TCG, or a loaded host). `t_bound` therefore
  checks two things. (a) On every host: the test kernel counts units of work (frames allocated or freed, messages,
  handles, mappings) in each interval with IRQ masked, and the test asserts no interval holds more than 64 + 16
  (`MASKED_MAX_UNITS`). (b) The time: scaled by a calibration (the best of five fills of 256 KiB against 10 us, the
  figure under HVF; the factor is clamped to 1..64 and printed), and the large cases are the best of three runs, so
  that a pause of the host is not taken for the kernel. Under HVF the factor is 1 and the bound is 2 ms.
- *Q6 note (CI), the time off HVF:* the scaling of (b) did not hold. A CI run of `tcg-smp4-gicv2` (factor 64, the
  clamp) had every memory case 20 to 40 times slower than the same job in green runs (`memory_map` of a step 14.5 ms
  against 0.47 ms; the contiguous step 244 ms against 5.9 ms, its 4 MiB case 287 ms against the scaled 128 ms) with the
  units within their bound: the host slowed the vCPU after the calibration, and the case with the largest fixed cost
  (the scan of the frame map, above) crossed first. As `t_cpu` does since Q4, `t_bound` now checks the time against
  2 ms only when the factor is 1 (HVF) and off HVF only that no interval lasted 1 s; the units, which no host speed
  changes, are the bound on the work between step points there. The scan of the frame map is not counted in units, so
  off HVF only the 1 s limit covers it; it is bounded by the machine's memory, not by the request. `t_bound` prints the
  test kernel's diagnosis when the units bound fails, and `frames::alloc` counts at its caller's line, so the
  diagnosis names the allocating path. Two paths can put more than a step of frames in one interval and are not
  changed: the heap's growth (`GROW_SIZE`, 256 frames under the heap's lock) and `reaper::help` from `frames::alloc`
  and `frames::alloc_contig` (64 frames freed in the caller's interval, up to 17 rounds for a run).
- *Q6 note (agent, ADR-0001 rule 6), the paths of the two notes above, now bounded:* `t_bound` failed twice locally
  on `auto-el2` under a loaded host (`units > 80`, no log kept), and three CI runs failed its contiguous case alone on
  time (287 ms at factor 64, 345 ms against 30 ms at factor 15, 279 ms; the other memory cases 1 to 18 ms).
  Test-kernel tests now force each path; measured before the change: one `frames::alloc` with no free frame and a dead
  object's frames queued, 65 units (64 freed by `reaper::help` plus the frame); a 128-frame contiguous run that had to
  wait for the reaper, 129 units (two rounds of help and the run); an allocation 512 KiB larger than every free span of
  the heap, 267 units (its growth mapped in one interval). The search of the frame map for a run was one masked pass
  that read the map bit by bit and counted no units. Changes: (1) `frames::alloc` takes one of the reaper's queued
  frames (`reaper::take`) instead of freeing a step of them and trying again: the frame keeps the dead object's charge
  until its item is emptied, and the new owner pays for it as for any frame (ADR-0261). (2) The search for a run reads
  the map a word at a time and in steps of 48 words (`zoraya_pmm::FrameAllocator::alloc_contig_step`, a resumable
  search; a run is taken only once all of it was read free under the lock, and one freed behind the search is not seen
  by that pass), each word counted as a unit, with a step point between steps, around each round of `reaper::help`,
  and before the first zeroing step; its only caller, `memory_create`, holds no lock there. The 4 MiB contiguous case
  of `t_bound` went from about 250 us masked under HVF to 20 us. (3) The kernel heap's least growth is 64 KiB, one
  chunk (`Backing::LEAST_GROWTH`, new in `zoraya-heap`; user heaps keep `GROW_SIZE`, 1 MiB), so a request of up to
  that maps at most 16 pages in its interval, the fixed work beside a step; it was 1 MiB (256 frames). The heap grows
  more often by less, so the tests that compared free frames across a program (`t_image`, `t_quota`, the test kernel's
  `*_from_a_process`) compare free frames plus the heap's, as `t_bound` did. (4) An allocation whose size grows with
  an argument or with what a process holds is prepared: `heap::prepare` maps the pages ahead in steps of 64 with a step
  point after each, only when no free span fits it (`Heap::fits`); the window keeps them mapped past its `next`, and
  `heap::stats().backing` counts them. A growth's check that the frames exist counts those the reaper holds
  (`frames::available`), which an allocation now takes: with growths of 64 KiB, free frames alone refused one while a
  closed 24 MiB object was still queued (`t_bound`'s create loop got `NoMemory`). Callers: `memory_create`'s list of frames, and the list of frames of an address
  space before each step of `spawn`'s image and stack, of a snapshot's copy and of its `finish`, and of `memory_map`
  (its doubling was a growth of the heap proportional to the space: 64 pages for a 64 MiB process). After the change
  the tests measure at most 1, 65 and 80 units, and the fragmented-map search at most 80 in any interval. With the scan
  in steps, the 1 s of the note above stayed (see the next note: the scaled bound was reverted again).
- *Q6 note (agent), review fixes of the note above:* (1) The list of frames grows by `GlobalAlloc::realloc` (the
  kernel heap does not override it): allocate, copy `len * 8` bytes, free. The copy is work, counted as
  `len * 8 / 4096` units (`UserMemory::count_list_copy`, in `AddressSpace::reserve_step` and in `adopt` when a frame
  fills the list), so that the test kernel sees it; and `AddressSpace::prepare_step` (no lock held: `spawn`, a
  snapshot's child) ends its growth with a step point, so the copy is an interval of its own and the step's pages are
  not added to it. `spawn` also prepares the list before the startup block, and counts its steps across segments:
  the last pages of one segment and the first of the next were one interval of up to 64 + 16 frames (measured at 80
  units by `a_spawn_with_a_big_space_is_bounded`, which spawns `hello` with 12,670 pages more of bss through the
  test kernel's step points, `steps::set_kernel_steps`). (2) `t_bound`'s time bound is 2 ms only at factor 1 and a
  sanity limit of 1 s off HVF again, as in `68a22cb`: a run of CI (43176131236) had every memory case 20 to 40 times
  slower than in green runs with the same units, and a 30 ms bound at factor 15 fails on one stall of the host. The
  units are the check on every host. (3) New mutants for the room made ahead of each step, the first zeroing step of
  a contiguous object, `available`, the reaper's counter and the rescan (`tools/mutants/kernel-objects.toml`). (4)
  The reaper keeps the frames it holds in a counter (`held`, updated by `defer`, `free_some` and `take`) and a cursor
  to the first item with frames: `held`, which a heap growth reads under the heap's lock, and the walks of `free_some`
  and `take` no longer visit the emptied items, which stay queued until the reaper drops them. (5) The stepped search
  for a run is not atomic: a run freed behind it is not seen by that pass. `FrameAllocator::generation` counts the
  calls that freed frames; a pass that found none and saw it change is made once more, once, counted as a round
  (`a_run_freed_behind_the_scan_is_found_by_a_second_pass`, and a host test of the generation). A run that exists at
  every instant is found; one freed and taken again between two steps may still be missed, which is a refusal at
  exhaustion and not a bug. (6) The step points of that search are `steps::between`, so a `job_kill` ends a
  `memory_create` of a contiguous object during its scan (`Cancelled`; the run was not taken yet). (7) The kernel's
  page-table frames (one level-3 table per 2 MiB of the heap's window) are counted in `heap::stats().backing`
  (`paging::table_frames`), so that the sum of free frames and the heap's, which `t_image`, `t_quota` and the test
  kernel compare, does not read one frame low across a 2 MiB boundary of the window.
  Still inline, recorded here for the architect: (a) the copy of the list under the space's lock in `memory_map`
  (`reserve_step` with the lock held; the list holds every frame of the space, so a process that holds N frames
  copies `N * 8` bytes, N / 512 units of a page, in one interval: 32 units at 64 MiB, 512 at 1 GiB; moving it out of
  the lock needs a list in chunks, or a swap that copies under the lock all the same); (b) the same copy in `adopt`
  when a single frame outside these loops fills the list (a page table of a one-page mapping), bounded as in (a);
  (c) eight mutants survive, and the list says why: a snapshot's `prepare_step` and its `finish`'s, and `memory_map`'s
  `heap::prepare` and `reserve_step`, need a user process with a space of at least 16,384 frames (the test kernel's
  `spawn` test covers `process::create` only; a crafted image run by `t_bound` would cover them); the `prepare_step`
  of `spawn`'s stack and of its startup block are reached only if the list passes a power of two there, which
  `hello`'s 16 stack pages and one page of block do not allow (a startup block of 4 pages or more, from a `Start`
  with long arguments, would); counting steps across segments needs a segment that ends more than 16 pages past a
  step boundary; and the kernel's page-table frames come from the boot pool (64 tables) until it is empty, so the
  count of them in `backing` shows only past 128 MiB of window; (d) `reaper::free_some` still frees up
  to a step under the queue's lock, as before.
- *Q7 review note (agent, ADR-0001 rule 6), the job tree walks are sites:* the review of the system calls
  (`docs/tasks/TASK-26-syscall-review.md`, error 1) found `job_signal`, `job_suspend`, `job_resume` and `job_kill` walking a
  whole subtree with IRQ masked and `TREE` held; a limited room can keep tens of thousands of jobs. They are sites of
  point 1 now. `Job::walk` goes in steps of `steps::STEP` visits (a job, or one of its processes, is one): inside a step
  it holds `TREE`; between steps it holds nothing and calls `steps::pause`, which cannot fail, so a walk runs to its end
  whatever the caller was asked (as a teardown does). The way through is a path of at most 32 frames (the depth limit)
  with the counts of children and of processes not yet visited, counted down from the end of the lists; a process that
  leaves (`swap_remove`) is either behind the walk or still ahead of it, and a dropped job leaves its dead slot in its
  parent's list while any walk runs (`WALKS`), so no index moves; a list drops dead slots when it is about to grow, with
  no walk running. What joins during a walk is not reached by it: `job_kill` closes each job before its children, so a
  job or process made later is refused; `job_suspend` and `job_resume` leave a job created or joined later in the state
  its parent has; `job_signal` is for what was there when it started. Two walks may now interleave, so a `job_resume`
  that lands between the steps of a `job_suspend` of the same tree leaves its later jobs suspended: the same outcome as
  the two calls in the other order. `t_bound` builds a tree of 700 jobs under one top job and measures the four calls
  against a tree of 8 (units on every host, time at factor 1). Not changed: `Job::drop`'s `retain` and the growth of
  `children` are linear in the parent's children, in one interval each (bounded by the memory limit; a later task);
  the drop of a long chain of jobs (each holds its parent) runs in whichever call drops the last handle.
- *Q7 review note (agent), subscriptions:* `port_subscribe`, `Events::set`, closing a port and freeing a source are linear in
  the subscriptions of a port or a source, under `EVENTS` and in interrupt handlers, where a walk cannot pause (review
  error 2). They are bounded, not stepped: ADR-0299 (proposed) limits both to 1024, so each is a fixed amount; the frees
  of up to 1024 `Paid` are not counted in units, only timed (`t_bound`).
- *Q2 note:* the INV-0261-2 test compares the heap with 4 KiB of allowance: the kernel heap keeps what it grew by.
  A refusal part way through the walk of `snapshot_process` (the charge of `copy_page`) is not tested yet.

## Revisit triggers
- SMP (interrupts masked on one CPU stop less, but locks across steps matter more).
- A new call whose work grows with an argument.

## Architect's decision

Accepted 2026-11-10. Decided by the architect; recorded by an agent at the architect's request. The numbers marked `[assumption]` are defaults until measured: each Q task of `docs/research/quotas/plan.md` measures the ones it implements and proposes changes with the numbers.
