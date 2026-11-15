# ADR-0436: The 2 GiB guest as built (plan part F8): the bound of one frame allocation, `make run-browser`, and what `top` shows

- **Status:** proposed
- **Date:** 2026-11-16
- **Depends on:** ADR-0020, ADR-0021, ADR-0267, ADR-0270, ADR-0415
- **Evidence:** CP-05, GR-005

## Context

ADR-0415 item 5 says the browser's guest has 2 GiB, `make run-browser` passes `--mem 2G`, and "the frame allocator is
tested at 2 GiB". Plan part F8 adds the check "`make run-browser` boots a 2 GiB guest; `top --session` shows its
memory". What exists: `tools/run-qemu.sh --mem` (default 128M), steps that already boot a 2 GiB guest (`test-qemu-auto-smp4-2g`,
`tcg-smp4-gicv3`, `boot-*-2g`), and the frame allocator's tests at a few hundred frames. **Measured by reading the code:** the
boot tables identity-map the first 4 GiB with 1 GiB blocks (ADR-0020), the direct map is built from blocks, the frame map is
two bits per frame outside RAM (128 KiB at 2 GiB), the heap grows on demand; none states a RAM size. What scales with RAM or with
what a process may hold on a 2 GiB guest is more than one loop. **F8 bounds one:** `FrameAllocator::alloc` reads the frame map
word by word from the last frame it gave, and when memory is nearly full and the free frame is behind the search it reads the
whole map, with the allocator's lock held and interrupts masked (ADR-0270, INV-0270-1): 512 words at 128 MiB, 8192 at 2 GiB,
counted as one unit of work. **F8 does not bound, and records as known limitations:** (a) `memory_map`'s `reserve_step` and
`adopt` copy the space's whole frame list under the space's lock (ADR-0270 Q6 note, "512 units at 1 GiB"): at 128 MiB no room
reaches that, on this guest a room may be 1200M, about 300,000 frames and 2.4 MB copied in one masked interval, not measured
(a `t_bound` case at `--mem 2G` would; left to the architect); (b) the heap's growth takes up to 256 frames in one interval
(`GROW_SIZE`), so an interval may hold 256 allocations, each a scan when other CPUs free frames behind the search. Also:
there is no browser to start yet (plan part E2), `top --session` (services and their accounts, ADR-0267) is not built, and `top`
prints the rooms' use and a `session` row. **For the architect:** ADR-0415's decision says "there is no `make run-browser`
default of 2 GiB", while its item 5 and the plan say `make run-browser` passes `--mem 2G`.

## Decision drivers

1. **No loop under a masked lock grows without a stated bound** (INV-0270-1, CP-05); a limit stated for one size is not a limit (GR-005).
2. **No RAM size in the range is assumed in the kernel**: the range is 128 MiB to 2 GiB (ADR-0021), and a size above it
   must fail a test, not slow the system.
3. **ADR-0415's acceptance** (item 5): the default of `make run` and `make run-gui` stays; the 2 GiB guest is for the browser.
4. **Small:** F8 is 100 lines in the plan.

## Considered options

### Option A: keep the next-fit word scan, bound it by the range, check it on the machine

The scan reads at most one word per 256 KiB of RAM (`FrameAllocator::alloc_scan_bound`); its worst case is measured on the
host at 2 GiB and in the guest as masked time; a test fails above 4 GiB (16384 words).

### Option B: a second level of the frame map (a bit per full word)

The scan reads at most 1/64 of the words (128 at 2 GiB). Every state change of the map must update the level, including the
run search, the reservations and frees.

### Option C: take the scan in steps with the lock released, as the run search does

Allocation of one frame then has step points, and so can fail with a kill; every caller (page tables, the heap, objects) holds
no spinlock only by luck.

### Option D: the Linux baseline, a buddy allocator with per-CPU lists

Constant-time single frames; a rewrite of ADR-0021 and of the metadata.

## Recommendation

**Option A.** At 2 GiB the worst scan is 8192 reads of one word, 94 us under HVF (measured, 2 GiB, 4 CPUs) and 1.2 ms under TCG, against ADR-0270's 2 ms; B is the next step when RAM passes 4 GiB, and the test that fails there says so.
For the guest: `make run-browser` is `RUN_GUI_CMD` (desktop, network, disk) with `--mem $(BROWSER_MEM)`, default `2G`; no step boots
that command line (it is the GUI test's line at another size), and there is no browser to start yet. I read ADR-0415's sentence as
saying that `make run` and `make run-gui` keep their sizes, which they do; if the architect meant that `run-browser` must not
default to 2 GiB, `BROWSER_MEM` loses its default and the plan's check passes it. The in-guest check is `make test-keeper`'s
`browser-guest` script at `--mem 2G` (ordinary kernel, serial console, no window; HVF, and TCG on the full set): the keeper's
session is sized from the frames (a room of 1200M is accepted, 1800M is refused as above the session's, and the refusal
names the session's size, which the script expects as 1452930K) and `top` shows the room's row and the `session` row.
**`top --session` is not built and stays so** (deviation, for the architect): ADR-0267 gives it services' accounts, which F8 does
not build. Options: (a) accept `top`'s `session` row plus the keeper's refusal above the session's ceiling as the plan's check
(done here); (b) build `top --session` now (the keeper would report a session ceiling and the services their use).

## Consequences

- Positive: the 2 GiB guest's allocator is tested on the host at its size and in the kernel at the machine's size; a RAM
  size beyond the tested range fails a test instead of a deadline.
- Negative: a machine above 4 GiB needs option B first.
- Follow-up: the browser's test step passes `--mem 2G` itself (plan part E2).

## Invariants

- **INV-0436-1.** One `FrameAllocator::alloc` reads at most `alloc_scan_bound` words of the frame map (one per 256 KiB of RAM, 8192 at 2 GiB), and with one frame free just behind the search it reads nearly all of them; with that scan INV-0270-1's 2 ms holds under HVF on a machine of at most 4 GiB. *Check:* `cargo test -p zoraya-pmm` (`tests/two_gib.rs`: words, every host), and the in-QEMU test `the_worst_single_allocation_is_bounded_by_the_machines_ram` (`make test-qemu`, at 128 MiB and at 2 GiB): the words read against the bound on every host; the masked time, on this CPU's counter, against 2 ms only where a word of an array is read in 50 ns or less (HVF), against ADR-0270 Q6's 1 s elsewhere (TCG: no time check of 2 ms).
- **INV-0436-2.** A 2 GiB guest boots and its room keeper sizes the session from the frames it has. *Check:* `make test-keeper` (`tests/shell/browser-guest.script` at `--mem 2G`).

## Open questions

- Whether the 2 ms bound holds under TCG: 1.2 ms was measured once at 2 GiB, the in-guest check keeps 2 ms for HVF-speed hosts only (a word of an array in 50 ns or less) and ADR-0270 Q6's 1 s elsewhere. `steps::masked::*` is one counter for all CPUs and can under-report on SMP; the test reads its own CPU's counter around the call.
- Known limitation of the in-guest check: the time is one measurement, not the best of three as `t_bound` takes, so a vCPU stall on an HVF-speed host during the call can fail it with nothing wrong; the words check does not depend on the host. The worst case cannot be repeated cheaply (the search moves with each allocation).
- Whether `top --session` should list the session's services' accounts (ADR-0267); not before a service reports its use.

## Revisit triggers

- A guest above 4 GiB, or a masked interval of the allocator above 200 us under HVF.

## Architect's decision

